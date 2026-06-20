# Heater Planning — Implementation Plan

Steps correspond to the minimum necessary combination from `heater_planning.md`.
Steps 1 and 3 are profile-only and can be deployed immediately to validate direction
before any code changes.

### Planning horizon is not derived automatically from assets

`plan_horizon_h` and `plan_step_s` are explicit parameters in the `planner:` section of
each profile. Adding a heater asset to a VEN does **not** automatically extend the
planning horizon. If a large thermal store is added to a VEN that currently uses a 24 h
horizon, the planning quality problems described in `heater_planning.md` will appear on
that VEN too, silently.

Rule of thumb for setting the horizon when adding a thermal asset:

```
characteristic_fill_time_h = tank_capacity_kwh / max_kw
if characteristic_fill_time_h > 8 h → plan_horizon_h should be ≥ 48
if characteristic_fill_time_h < 2 h → 24 h is sufficient
```

For ven-2's 2000 L tank: `93 kWh / 6 kW = 15.5 h` → 48 h required.
For ven-3's 200 L tank: `3.5 kWh / 6 kW = 0.58 h` → 24 h sufficient.

---

## Step 1 — Extend planning horizon to 48 h at 10 min resolution (Option 3a)

**Branch:** `fix/heater-horizon-48h`
**Files:** `VEN/profiles/ven-2.yaml`
**Risk:** Low — profile-only, fully reversible.

### Scope of impact — within a VEN vs. across VENs

**Within ven-2**, all assets share a single MILP solve with a single time grid. The
power balance per slot:

```
p_imp[t] + p_pv[t] = p_base[t] + heater[t] + ev[t] + bat_charge[t] − bat_discharge[t] + p_exp[t]
```

means the heater, PV, and base load are already co-optimized. Changing `plan_step_s`
and `plan_horizon_h` changes the time grid for **all assets within ven-2 simultaneously**
— it is not heater-only.

**Across ven-1, ven-2, ven-3**, each VEN is a separate physical site with its own PV
array, its own grid connection, and its own independent MILP solve. They do not share
a PV source or a grid connection:

| VEN | Site type | Own PV | Own grid |
|-----|-----------|--------|----------|
| ven-1 | Residential prosumer | 8 kW | yes |
| ven-2 | Commercial building | 12 kW | yes |
| ven-3 | Full mix installation | 6 kW | yes |

Inter-VEN coordination happens only through VTN DR signals (capacity limits, price
events), not through shared MILP variables. Each VEN responds to those signals using
its own plan on its own time grid. Different `plan_step_s` values between VENs are
therefore fully valid — they reflect different asset portfolios, not a coordination gap.

**Why 48 h applies to ven-2 but not the others:**

The 48 h horizon is motivated by the characteristic timescale of ven-2's slowest asset —
the 2000 L hot water tank (93 kWh capacity, 15.5 h to fill). The other VENs have faster
assets:

| VEN | Slowest asset | Characteristic fill time | 48 h justified? |
|-----|---------------|--------------------------|-----------------|
| ven-1 | Home battery (10 kWh, 5 kW) | 2 h | No — cycles within 24 h |
| ven-2 | Heater (2000 L, 93 kWh, 6 kW) | 15.5 h | **Yes** — inter-day thermal battery |
| ven-3 | Heater (200 L, 3.5 kWh, 6 kW) | 35 min | No — within-day; 10 min steps leave only 3.5 slots per session |

ven-3 also already has `phase2_epsilon_eur: 5.0` — its fragmentation was treated
separately and correctly for its asset mix. Changing its step size to 10 min would
make its heater planning coarser, not better.

**This step touches only `ven-2.yaml`.** If a future VEN is added with a large thermal
store or multi-day storage, the same horizon analysis should be applied to that VEN's
profile at that time.

### Test surface analysis

| Test suite | Profile used | Target VEN | Affected by ven-2.yaml change? |
|------------|-------------|------------|-------------------------------|
| MILP unit tests (`milp_planner/tests/`) | Hardcoded in `make_profile()` (300 s or 1800 s) | none | **No** — fully independent |
| `ven_heater_tank.feature` | `test` (3600 s/slot) | test container | **No** |
| `ven_planner.feature` | `test` (3600 s/slot) | test container | **No**, except stale description text |
| `ven_uc_normal/stress.feature` | `test` | test container | **No** |
| `ven_device_sessions.feature` | `test` | test container | **No** |
| `use_cases.feature` | — | **ven-2 by name** | **Yes** — EV scenarios |
| `ui_use_cases.feature` | — | **ven-2 by name** | **Yes** — EV scenarios |
| `ven_isolation.feature` | — | ven-2 by name | Yes — spot-check |

### 1.1 Update profile

```yaml
planner:
  plan_step_s: 600        # was 300 (5 min → 10 min)
  plan_horizon_h: 48      # was 24
```

Slot count stays at 288; binary variable count is unchanged from today.

### 1.2 Update stale test description

In `tests/features/ven_planner.feature` line 3, update the feature description:

```
# was: device sessions. The plan covers a 24-hour horizon as a unified slot
# becomes: device sessions. The plan covers the configured planning horizon as a unified slot
```

This is not an assertion — it will not cause a test failure — but it is a lie once
ven-2 moves to 48 h. Fix it in the same commit as the profile change.

### 1.3 Deploy and benchmark solver time

```
ssh Pi4-Server "cd /srv/docker/openadr_lab/VEN && docker compose build ven-2 && docker compose up -d ven-2"
curl -X POST http://Pi4-Server:8212/plan/trigger
```

Watch the `planner: plan adopted` log line for `solver_ms`.
Target: < 40 000 ms. If `solver_ms` > 50 000 ms on three consecutive replans, revert
`plan_step_s: 300` and escalate to Option 3b (non-uniform grid — see appendix).

### 1.4 Run E2E suite; triage EV scenarios against ven-2

Run the full BDD suite on Pi4:
```
bash run_all_tests.sh --e2e
```

Pay particular attention to `use_cases.feature` and `ui_use_cases.feature` scenarios
that create an EV session against ven-2 with a departure time. At 10 min resolution,
a departure deadline of e.g. "2 hours from now" maps to `t_dead = 120 * 60 / 600 = 12`
slots — which is exact. Deadlines that are not multiples of 10 min lose up to 10 min of
charging time. If any EV scenario fails or the charged energy is below the required
minimum, the precision loss is unacceptable and Step 1b (Option 3b) is required.

If all E2E tests pass, no test changes are needed — the suite already runs against the
live ven-2 container and validates behaviour end-to-end.

### 1.5 Observe plan quality over one full 24 h cycle

Capture `/plan` at three times:
- Morning trough (~09:00 UTC, heater OFF, temp near minimum)
- Mid-solar (~12:00 UTC, heater ON)
- Evening trough (~18:00 UTC, heater OFF again)

Verify all three captures show:
- ≤ 4 switches across 288 slots
- No single-slot pulses
- Phase-dependence eliminated (all three plans look structurally similar)

Temperature ceiling improvement comes in Step 2; do not require it here.

---

## Step 2 — Add terminal energy reward to Phase 1 objective (Option 2)

**Branch:** `fix/heater-terminal-reward`
**Files:**
- `VEN/profiles/ven-2.yaml`
- `VEN/src/profile.rs` (`HeaterConfig`)
- `VEN/src/entities/asset_params.rs` (`HeaterParams`)
- `VEN/src/assets/heater.rs` (`HeaterMilpContext::from_state`, `objective`)
- `VEN/src/controller/milp_planner/asset_port.rs` (`HeaterMilpContext`, `HeaterScalars`)

**Risk:** Medium — MILP objective change; requires calibration of the coefficient.

### Test surface analysis

The code change touches `HeaterMilpContext::objective()` and its data path. The new
field defaults to `0.0`, so all existing tests that do not set it explicitly remain
valid — the objective is unchanged at the default.

| What to test | Where |
|---|---|
| Terminal reward appears in Phase 1 objective and raises `e_tank[n-1]` | New unit test in `assets/heater.rs` (see 2.6) |
| Default `c_terminal_eur_kwh: 0.0` leaves existing unit tests unchanged | Existing tests must still pass without modification |
| Phase 2 objective is unaffected (terminal reward is Phase 1 only) | Covered by existing Phase 2 tests |
| `ven_heater_tank.feature` scenarios unaffected | They use `profile "test"` which has no `c_terminal_eur_kwh` → defaults to 0.0 |
| Temperature range increases in ven-2 live plan | Manual observation (Step 2.7) — no BDD scenario exists for this yet |

No existing tests need modification. The one new unit test (2.6) is the only required
test addition.

### 2.1 Add profile field

In `VEN/src/profile.rs`, `HeaterConfig`:

```rust
/// Forward value of stored heat [EUR/kWh]. Added to Phase 1 objective as
/// -c_terminal * e_tank[n-1]. Set to the average expected import tariff.
/// 0.0 disables the reward (default — backward compatible).
#[serde(default)]
pub c_terminal_eur_kwh: f64,
```

Propagate in the `AssetProfile::Heater` arm of `profile.rs`:

```rust
c_terminal_eur_kwh: c.c_terminal_eur_kwh,
```

### 2.2 Propagate to HeaterParams

In `VEN/src/entities/asset_params.rs`:

```rust
pub struct HeaterParams {
    // ... existing fields ...
    pub c_terminal_eur_kwh: f64,
}

impl Default for HeaterParams {
    fn default() -> Self {
        Self {
            // ...
            c_terminal_eur_kwh: 0.0,
        }
    }
}
```

### 2.3 Add to HeaterMilpContext and HeaterScalars

In `VEN/src/controller/milp_planner/asset_port.rs`:

```rust
pub struct HeaterMilpContext {
    // ... existing fields ...
    pub c_terminal_eur_kwh: f64,
}

pub struct HeaterScalars {
    // ... existing fields ...
    pub c_terminal_eur_kwh: f64,
}
```

### 2.4 Set value in from_state

In `HeaterMilpContext::from_state()` in `VEN/src/assets/heater.rs`, pass
`cfg.c_terminal_eur_kwh` into both the `MustRun` and `MayRun` construction arms.

### 2.5 Add terminal term to Phase 1 objective

In `HeaterMilpContext::objective()`, in the `c_startup_eur == 0.0` branch (Phase 1 mode):

```rust
// Terminal reward: forward value of heat stored at horizon end.
// Negative sign because we minimise: more stored heat reduces the objective.
if self.c_terminal_eur_kwh > 0.0 && n > 0 {
    obj += -self.c_terminal_eur_kwh * v.e_tank[n - 1];
}
```

### 2.6 Write unit test (test-first)

In `VEN/src/assets/heater.rs` tests:

```
test_terminal_reward_incentivises_higher_end_state
```

Solve a minimal 3-slot MILP with terminal reward enabled vs disabled on a small tank.
Assert `e_tank[2]` is higher with the reward than without, at equal energy cost.

### 2.7 Set value in profile and deploy

```yaml
# ven-2.yaml
assets:
  - type: heater
    c_terminal_eur_kwh: 0.32   # average of cheap tariff (0.30) and peak (0.38)
```

Observe plan temperature range. Expected: tank reaches 55–70 °C during solar window.
If tank hits T_max (80 °C) every cycle and stays there, reduce coefficient toward the
export tariff (0.29 EUR/kWh). If tank still barely exceeds 45 °C, increase toward 0.38.

---

## Step 3 — Fix epsilon/penalty coherence (Option 4)

**Branch:** `fix/heater-horizon-48h` (same commit as Step 1 or separate)
**Files:** `VEN/profiles/ven-2.yaml`
**Risk:** Low — profile-only.

### Test surface analysis

`phase2_epsilon_eur` is a runtime planner parameter. No unit test asserts on its value
or on the Phase 2 objective achieved. The existing Phase 2 unit tests use their own
hardcoded epsilon and are unaffected.

The only risk is behavioural: a larger epsilon gives Phase 2 more freedom, which could
in theory consolidate blocks that should remain separate. This is caught by the E2E
suite (run after deploy) — no test changes are needed beforehand.

### 3.1 Update profile

```yaml
planner:
  phase2_epsilon_eur: 1.00   # was 0.10; set to 2x switching_penalty_eur (0.50)
```

Rationale: epsilon must allow Phase 2 to afford eliminating at least one extra switch.
With `switching_penalty_eur: 0.50`, epsilon = 1.00 EUR allows consolidating two extra
switches if the required extra energy stays below 1.00 EUR.

### 3.2 Validate no over-consolidation

Check the plan during a period with non-monotonic tariffs (e.g. cheap overnight window
sandwiched between two peak periods). Verify Phase 2 does not merge what should be two
separate blocks spanning a peak-tariff gap.

### 3.3 Validate Phase 2 solve time

A larger epsilon gives Phase 2 a larger feasible region, which is typically easier to
solve. Confirm `solver_ms` does not increase. If it does, the Phase 2 MILP is exploring
a broader space; consider adding `model.with_mip_gap(0.03)` for Phase 2 only.

---

## Step 4 — Block commitment anchor (Option 7)

**Branch:** `fix/heater-block-anchor`
**Files:**
- `VEN/src/state.rs` (`HemsState`, `AppState`)
- `VEN/src/services/planning.rs` (`adopt_if_warranted`, new helpers)
- `VEN/src/tasks/planning.rs` (read anchor before solve)
- `VEN/src/controller/milp_planner/asset_port.rs` (`HeaterMilpContext`, `HeaterScalars`)
- `VEN/src/assets/heater.rs` (`HeaterMilpContext::from_state`, `declare_vars`)

**Risk:** Medium — touches state management and MILP variable declaration.

### Test surface analysis

This is the most structurally invasive step. The anchor pins MILP binary variables to
fixed values, which changes the feasible region of the Phase 1 problem. Tests that
assert the planner will freely reschedule heater slots could fail if the anchor is
active during the test.

| What to test | Where |
|---|---|
| `heater_block_end` pure function | New unit tests in `services/planning.rs` (see 4.8) |
| `build_heater_anchor` pure function | New unit tests (see 4.8) |
| Pinned variables have fixed bounds in `declare_vars` | New unit test (see 4.8) |
| Hard trigger clears anchor | New unit test covering the anchor-clear path |
| Existing `ven_heater_tank.feature` scenarios | **Must still pass.** These run against `profile "test"` which never sets an anchor — `anchor_until` defaults to `None`, pinning is a no-op |
| Existing MILP unit tests | **Must still pass.** They construct `HeaterMilpContext` directly; `anchored_kw` must default to `vec![None; n]` so the absence of the field pins nothing |

The critical invariant: **when `anchor_until` is None, the planner behaves identically
to today.** Run the full unit + E2E suite with the anchor logic in place but
`anchor_until = None` before testing any anchor-specific scenarios.

### 4.1 Add anchor field to HemsState

In `VEN/src/state.rs`:

```rust
pub struct HemsState {
    pub active_plan: Option<Plan>,
    pub anchor_until: Option<DateTime<Utc>>,   // add
    // ... rest unchanged ...
}
```

Add accessors to `AppState`:

```rust
pub async fn anchor_until(&self) -> Option<DateTime<Utc>> {
    self.hems.read().await.anchor_until
}
pub async fn set_anchor_until(&self, t: Option<DateTime<Utc>>) {
    self.hems.write().await.anchor_until = t;
}
```

### 4.2 Add block-end helper (pure function)

In `VEN/src/services/planning.rs`:

```rust
/// Returns the end-time of the first contiguous heater on/off block in `plan`
/// starting at or after `now`. Returns None if the plan has no heater slots or
/// only one block throughout the entire horizon.
pub fn heater_block_end(plan: &Plan, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let mut iter = plan.all_slots().filter(|s| s.end > now).peekable();
    let kw0 = iter.peek()?.planned_kw_by_asset.get("heater").copied().unwrap_or(0.0);
    iter.take_while(|s| {
        let kw = s.planned_kw_by_asset.get("heater").copied().unwrap_or(0.0);
        (kw - kw0).abs() < 0.1
    })
    .last()
    .map(|s| s.end)
}
```

### 4.3 Set anchor after plan adoption

In `PlanningService::adopt_if_warranted()`, after `state.set_active_plan(Some(...))`:

```rust
let anchor = heater_block_end(&plan, now);
state.set_anchor_until(anchor).await;
```

### 4.4 Clear anchor on hard triggers

In `VEN/src/tasks/planning.rs`, before the solve, when trigger is not `Periodic`:

```rust
if !matches!(trigger, PlanTrigger::Periodic) {
    state.set_anchor_until(None).await;
}
```

### 4.5 Build per-slot anchor decisions before MILP

In `VEN/src/tasks/planning.rs`, before building asset MILP contexts:

```rust
let anchor_until = state.anchor_until().await;
let current_plan = state.active_plan().await;
// Vec<Option<f64>>: Some(kw) for anchored slots, None for free slots.
let heater_anchor: Vec<Option<f64>> = build_heater_anchor(
    current_plan.as_ref(), anchor_until, now, step_s, n_slots,
);
```

Add helper (can live in `services/planning.rs` or a new `planning_helpers.rs`):

```rust
pub fn build_heater_anchor(
    plan: Option<&Plan>,
    anchor_until: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
    step_s: u64,
    n_slots: usize,
) -> Vec<Option<f64>> {
    let mut out = vec![None; n_slots];
    let (Some(plan), Some(until)) = (plan, anchor_until) else { return out };
    for (i, slot) in plan.all_slots().filter(|s| s.end > now).take(n_slots).enumerate() {
        if slot.start >= until { break; }
        out[i] = Some(slot.planned_kw_by_asset.get("heater").copied().unwrap_or(0.0));
    }
    out
}
```

### 4.6 Add anchored decisions to HeaterMilpContext

In `VEN/src/controller/milp_planner/asset_port.rs`:

```rust
pub struct HeaterMilpContext {
    // ... existing fields ...
    /// Per-slot anchor: Some(kw) pins z_heat_* for that slot; None = free.
    pub anchored_kw: Vec<Option<f64>>,
}
```

Pass `heater_anchor` through `HeaterMilpContext::from_state()` or as a separate
parameter when constructing the context in `tasks/planning.rs`.

### 4.7 Pin variables in declare_vars

In `HeaterMilpContext::declare_vars()`, for each slot `t`:

```rust
let (fixed_mid, fixed_full) = match self.anchored_kw.get(t).copied().flatten() {
    Some(kw) => kw_to_tier_pair(kw, self.p_mid_kw, self.p_full_kw),
    None => (None, None),
};

let z_mid = match fixed_mid {
    Some(v) => vars.add(variable().min(v).max(v)),
    None if must_not => vars.add(variable().min(0.0).max(0.0)),
    None => vars.add(variable().binary()),
};
let z_full = match fixed_full {
    Some(v) => vars.add(variable().min(v).max(v)),
    None if must_not => vars.add(variable().min(0.0).max(0.0)),
    None => vars.add(variable().binary()),
};
```

Where `kw_to_tier_pair` maps the kW value to `(Some(0.0|1.0), Some(0.0|1.0))` using
the same quantisation logic as `step_inner`.

### 4.8 Write unit tests (test-first)

- `test_heater_block_end_on_block` — plan starts ON for 6 slots then OFF; verify end = slot 6 end time
- `test_heater_block_end_off_block` — plan starts OFF; verify end = end of initial OFF run
- `test_heater_block_end_no_heater` — plan with no heater allocations; verify None
- `test_build_heater_anchor_pins_within_window` — anchor_until covers first 3 slots; verify `out[0..2]` = Some, `out[3..]` = None
- `test_anchored_vars_produce_fixed_bounds` — declare_vars with anchored slots; verify those variable bounds are `[v, v]`

### 4.9 Deploy and verify

Trigger two consecutive replans 5 min apart while the heater is mid-block. Confirm:
- The heater decision for the current slot does not change between the two plans
- The plan beyond the anchor_until boundary can still change freely
- A hard trigger (e.g. `POST /plan/trigger`) clears the anchor and produces a fresh solve

---

## Step 5 — Gate switch-count guard (Option 6)

**Branch:** `fix/heater-gate-guard`
**Files:**
- `VEN/src/entities/planner_params.rs`
- `VEN/src/profile.rs`
- `VEN/src/services/planning.rs`
- `VEN/src/tasks/planning.rs`

**Risk:** Low — additive gate logic; new parameter defaults to 0.0 (backward compatible).

### Test surface analysis

`evaluate_acceptance_gate` already has comprehensive unit tests in `services/planning.rs`.
The new switch-count surcharge is an additive branch gated on `gate_switch_penalty_eur > 0.0`.

| What to test | Where |
|---|---|
| `count_heater_switches` helper | New unit tests (see 5.6) |
| Gate rejects noisier plan below surcharge | New unit test (see 5.6) |
| Gate accepts noisier plan above surcharge | New unit test (see 5.6) |
| Gate still accepts cleaner plan at zero surcharge | New unit test (see 5.6) |
| Hard trigger and decay still bypass the surcharge | New unit tests (see 5.6) |
| All existing gate tests pass unchanged | `gate_switch_penalty_eur` defaults to 0.0 → surcharge = 0 → existing behaviour unchanged |

The only test file that needs updating is `services/planning.rs` (adding new tests).
Existing tests must not be changed — they validate the zero-penalty default path. The
`evaluate_acceptance_gate` signature change (new `gate_switch_penalty_eur` parameter)
must propagate to all existing call sites in tests with value `0.0`.

### 5.1 Add parameter to planner config

In `VEN/src/entities/planner_params.rs`:

```rust
/// Per-extra-switch surcharge applied to the acceptance gate [EUR/switch].
/// If a new periodic plan has more heater switches than the current plan,
/// this amount per extra switch is added to the required cost improvement.
/// 0.0 disables (default).
pub gate_switch_penalty_eur: f64,
```

Default: `0.0`. In `VEN/src/profile.rs`, `PlannerConfig`:

```rust
#[serde(default)]
pub gate_switch_penalty_eur: f64,
```

Propagate into `PlannerParams` in `main.rs`.

### 5.2 Add switch-counting helper

In `VEN/src/services/planning.rs`:

```rust
/// Count heater on/off transitions in future slots of `plan` (start >= now).
pub fn count_heater_switches(plan: &Plan, now: DateTime<Utc>) -> usize {
    let mut count = 0usize;
    let mut prev: Option<f64> = None;
    for slot in plan.all_slots().filter(|s| s.start >= now) {
        let kw = slot.planned_kw_by_asset.get("heater").copied().unwrap_or(0.0);
        if prev.is_some_and(|p| (p - kw).abs() > 0.1) {
            count += 1;
        }
        prev = Some(kw);
    }
    count
}
```

### 5.3 Extend evaluate_acceptance_gate

Add `gate_switch_penalty_eur: f64` to the function signature. Inside, after computing
`improvement` and before the final comparison:

```rust
let switch_surcharge = if gate_switch_penalty_eur > 0.0 {
    if let Some(cur) = current {
        let cur_sw = count_heater_switches(cur, now);
        let new_sw = count_heater_switches(new_plan, now);
        new_sw.saturating_sub(cur_sw) as f64 * gate_switch_penalty_eur
    } else {
        0.0
    }
} else {
    0.0
};

if fully_decayed || improvement > effective_threshold + switch_surcharge {
    true
} else {
    debug!(
        improvement_eur = improvement,
        effective_threshold_eur = effective_threshold,
        switch_surcharge_eur = switch_surcharge,
        "periodic plan rejected"
    );
    false
}
```

Note: `fully_decayed` still forces acceptance even with a surcharge — decay is an
escape hatch for stale plans and must not be blocked.

### 5.4 Thread parameter through call site

In `VEN/src/tasks/planning.rs`, pass `planner.gate_switch_penalty_eur` into
`adopt_if_warranted`. Update `adopt_if_warranted` signature to accept and forward it to
`evaluate_acceptance_gate`.

### 5.5 Set value in profile

```yaml
# ven-2.yaml
planner:
  gate_switch_penalty_eur: 0.50   # matches switching_penalty_eur
```

### 5.6 Write unit tests (test-first)

- `test_count_switches_empty_plan` — no heater slots → 0
- `test_count_switches_one_block` — ON then OFF → 2 (rise + fall)
- `test_count_switches_filters_past_slots` — past slots before `now` are ignored
- `test_gate_rejects_noisier_plan_below_surcharge` — new plan has 2 extra switches
  (surcharge 1.00 EUR), improvement 0.60 EUR → rejected
- `test_gate_accepts_noisier_plan_above_surcharge` — same but improvement 1.20 EUR → accepted
- `test_gate_accepts_cleaner_plan_at_zero_surcharge` — new plan has fewer switches → no
  surcharge, normal threshold applies
- `test_gate_hard_trigger_ignores_surcharge` — hard trigger always accepts regardless of
  switch count
- `test_gate_decayed_accepts_despite_surcharge` — fully decayed plan accepts unconditionally

---

## Sequencing and Dependencies

```
Step 1 (profile: horizon)   ──────────── deploy + 24h observation
Step 3 (profile: epsilon)   ──┘ same PR  deploy + 24h observation

Step 2 (code: terminal)     ────────────────── after Step 1 stable, test-first
Step 4 (code: anchor)       ────────────────── independent, test-first
Step 5 (code: gate guard)   ────────────────── independent, test-first
```

Steps 2, 4, and 5 have no shared file conflicts and can be developed in parallel on
separate branches. Steps 1 and 3 should be deployed and confirmed stable (solver time
OK, plan quality improved) before adding code complexity.

Each step must pass the full test suite before merging:
- `wsl cargo test -p ven` locally
- E2E BDD suite on Pi4 (`bash run_all_tests.sh --e2e`)

Steps 1 and 3 additionally require a ≥ 24 h observation period to verify plan quality
across the full daily cycle before the next step is started.

---

## Appendix: Option 3b (non-uniform grid) if Step 1 times out

If Step 1 exceeds 50 000 ms solver time, the uniform 10 min / 48 h approach is not
viable on Pi4. Proceed with the non-uniform grid refactor instead.

**Core change:** `MilpInputs.dt_h: f64` → `MilpInputs.dt_h: Vec<f64>`.

Files affected:
- `VEN/src/controller/milp_planner/inputs.rs` — build `dt_h` as a Vec; populate with
  step widths (e.g. `5 min` for slots 0–95, `30 min` for slots 96–191)
- `VEN/src/controller/milp_planner/solver_phase1.rs` — replace `dt_h` scalar with
  `inputs.dt_h[t]` at every `× dt_h` term in objective and power balance
- `VEN/src/controller/milp_planner/solver_phase2.rs` — same
- `VEN/src/assets/heater.rs` — C2 dynamics: `(P[t] - q_dem) * dt_h[t]` per slot
- Battery and EV asset constraint/objective methods — same pattern
- `VEN/src/controller/milp_planner/results.rs` — compute slot start/end from cumulative
  step-time grid rather than `now + t * step_s`
- All MILP unit tests that assert exact constraint counts using the `9n` formula —
  counts are unchanged but fixtures must use a `Vec<f64>` for dt_h

Profile representation: add `plan_step_s_far: u64` and `plan_near_horizon_h: u64` fields
to `PlannerConfig`; derive the per-slot dt_h vector in `build_milp_inputs`.
