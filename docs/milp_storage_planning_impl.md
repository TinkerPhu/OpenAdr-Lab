# MILP Storage Planning — Implementation Plan

Implements the minimum necessary combination from
[milp_storage_planning.md](milp_storage_planning.md), in priority order. Steps 1 and 3
are profile-only and can be deployed immediately. Steps 2, 5, and 6 are independent code
changes with no shared file conflicts.

> **Scope note:** Although this work originated from a heater planning observation, the
> changes touch core MILP infrastructure (`AssetMilpContext` trait, `dt_h` grid,
> `MilpInputs`), all storage asset contexts (heater, battery, EV), the planning loop, and
> the acceptance gate. Test impact spans ~7 files and ~20+ test functions beyond the new
> tests added per step.

### Planning horizon is not derived automatically from assets

`plan_horizon_h` and `plan_step_s` are explicit parameters in the `planner:` section of
each profile. Adding a heater asset to a VEN does **not** automatically extend the
planning horizon. If a large thermal store is added to a VEN that currently uses a 24 h
horizon, the planning quality problems described in `milp_storage_planning.md` will appear on
that VEN too, silently.

Rule of thumb when adding a thermal asset:

```
characteristic_fill_time_h = tank_capacity_kwh / max_kw
if characteristic_fill_time_h > 8 h  →  plan_horizon_h should be ≥ 48
if characteristic_fill_time_h < 2 h  →  24 h is sufficient
```

For ven-2's 2000 L tank: `93 kWh / 6 kW = 15.5 h` → 48 h required.
For ven-3's 200 L tank: `3.5 kWh / 6 kW = 0.58 h` → 24 h sufficient.

---

## Step 1 — Fix epsilon/penalty coherence and adoption threshold (Option 4)

**Branch:** `fix/heater-epsilon-coherence`
**Files:** `VEN/profiles/ven-2.yaml`
**Risk:** Low — profile-only, fully reversible. Deploy first; zero code risk.

### What this fixes

`switching_penalty_eur: 0.50` says each switch costs 0.50 EUR, but
`phase2_epsilon_eur: 0.10` gives Phase 2 a budget of only 0.10 EUR to reduce switches.
Phase 2 can afford to eliminate at most `0.10 / 0.50 = 0.2` switches per plan. Any
consolidation requiring more than 0.33 kWh of extra energy (at 0.30 EUR/kWh) fails.

`plan_adoption_threshold_eur: 0.0` (ven-2 default) means any marginal improvement in
objective — even 0.01 EUR — triggers plan replacement, causing constant churn regardless
of fragmentation changes.

### 1.1 Update profile

```yaml
planner:
  phase2_epsilon_eur: 1.00          # was 0.10; = 2× switching_penalty_eur (0.50)
  plan_adoption_threshold_eur: 0.20 # was 0.0; matches ven-1; requires real improvement
  plan_adoption_decay_s: 1500       # 5× replan_interval_s — allow eventual replacement
```

Rationale: epsilon = 1.00 EUR allows Phase 2 to eliminate up to 2 extra switches if the
extra energy cost stays below 1.00 EUR. Adoption threshold = 0.20 EUR requires meaningful
improvement before replacing the current plan, reducing churn.

### Test surface analysis

`phase2_epsilon_eur` and `plan_adoption_threshold_eur` are runtime parameters. No unit
test asserts on their values. The existing Phase 2 unit tests use their own hardcoded
epsilon. The E2E suite validates behaviour post-deploy; no test changes needed.

### 1.2 Deploy and observe

```
ssh Pi4-Server "cd /srv/docker/openadr_lab/VEN && docker compose build ven-2 && docker compose up -d ven-2"
```

Observe over 24 h:
- Single-slot pulses should disappear (epsilon gives Phase 2 budget to consolidate)
- Plan churn should reduce (threshold blocks marginal replacements)
- Verify no over-consolidation: check plan during non-monotonic tariff periods

---

## Step 2 — Auto-computed terminal energy reward (Option 2)

**Branch:** `fix/heater-terminal-reward`
**Files:**
- `VEN/src/controller/milp_planner/inputs.rs` (`build_milp_inputs` — compute avg tariff)
- `VEN/src/controller/milp_planner/asset_port.rs` (`HeaterMilpContext`, `HeaterScalars`)
- `VEN/src/assets/heater.rs` (`HeaterMilpContext::from_state`, `objective`)
- `VEN/src/profile.rs` (`HeaterConfig` — optional override field only)
- `VEN/src/entities/asset_params.rs` (`HeaterParams` — optional override field only)
- Battery asset context (same pattern as heater, separate sub-step)

**Risk:** Medium — MILP Phase 1 objective change. Test-first required.

### Design: auto-computation, no mandatory profile parameter

The coefficient is computed from inputs already available in `build_milp_inputs()`:

```
c_terminal_heater  = mean(c_imp_eur_kwh[0..n]) + c_ctrl_imp_malus_eur_kwh
c_terminal_battery = mean(c_imp_eur_kwh[0..n]) × round_trip_efficiency
c_terminal_ev      = 0.0  (deadline constraint handles EV incentive)
```

This makes the coefficient size-independent, self-consistent (solar always net-positive,
peak never net-positive), and requires no profile tuning. The profile MAY override with
an explicit `c_terminal_eur_kwh: Option<f64>` where `Some(0.0)` disables and `None`
(omitted in YAML) means auto-compute.

### Test surface analysis

**Struct literal breakage — the main mechanical cost.**
`HeaterMilpContext` is a Rust struct. Adding `c_terminal_eur_kwh: f64` to it causes a
compile error in every test that constructs the struct via a literal without listing all
fields. Two helpers are the choke points:
- `make_may_run_ctx()` in `VEN/src/controller/milp_planner/tests/heater.rs`
- `make_ctx()` in `milp_context_trait_tests` in the same file

Fix those two helpers and all downstream test functions compile unchanged. Check for any
direct `HeaterScalars { ... }` constructions and update those too.

**Coordinate with Step 5.** Step 5 also adds `anchored_kw: Vec<Option<f64>>` to
`HeaterMilpContext`. Implement both field additions on the same branch to break and fix
the struct exactly once.

**`MilpInputs` gains `avg_imp_eur_kwh: f64`.** Any test that constructs `MilpInputs`
directly needs this new field. Tests in `tests/solver.rs` and `tests/basic.rs` that call
`build_milp_inputs()` helpers go through the production builder and are unaffected, but
any literal `MilpInputs { ... }` construction must be updated.

| What to test | New or existing? |
|---|---|
| Auto-computed c_terminal raises `e_tank[n-1]` vs disabled baseline | **New** unit test in `assets/heater.rs` |
| `Some(0.0)` disables the reward | **New** unit test |
| Auto-computed value equals `avg_imp + malus` numerically | **New** unit test |
| `make_may_run_ctx` and `make_ctx` compile after struct change | **Mechanical update** to 2 helpers; downstream tests unchanged |
| Phase 2 objective unaffected (terminal term is Phase 1 only) | Existing ✓ |
| `ven_heater_tank.feature` unaffected | Existing ✓ — `profile "test"` has its own tariff array |
| Battery terminal reward raises `e_bat[n-1]` | **New** unit test in battery asset tests |

### 2.1 Compute average import tariff in build_milp_inputs

In `VEN/src/controller/milp_planner/inputs.rs`, after the per-step tariff arrays are
built, compute and expose the average:

```rust
let avg_imp_eur_kwh = c_imp.iter().sum::<f64>() / n as f64;
// Store in MilpInputs for use by asset contexts in the solve phase.
```

Add `avg_imp_eur_kwh: f64` to `MilpInputs`. It is already derivable from the
`c_imp_eur_kwh` vector but computing it once avoids repeating the iteration per asset.

### 2.2 Add optional override to HeaterConfig and HeaterParams

In `VEN/src/profile.rs`, `HeaterConfig`:

```rust
/// Optional override for auto-computed terminal reward [EUR/kWh].
/// None (omitted in YAML): auto-compute from avg import tariff + malus.
/// Some(0.0): disabled.
/// Some(x): fixed at x EUR/kWh.
#[serde(default)]
pub c_terminal_eur_kwh: Option<f64>,
```

Propagate to `HeaterParams` with the same `Option<f64>` type.

### 2.3 Resolve effective coefficient in HeaterMilpContext

In `VEN/src/controller/milp_planner/asset_port.rs`, add `c_terminal_eur_kwh: f64` to
`HeaterMilpContext` (always resolved, never Optional). In `HeaterMilpContext::from_state()`,
resolve the effective value:

```rust
let c_terminal = match cfg.c_terminal_eur_kwh {
    Some(v) => v,                                         // profile override
    None => avg_imp_eur_kwh + planner.c_ctrl_imp_malus_eur_kwh,  // auto-compute
};
```

`avg_imp_eur_kwh` must be passed into `from_state()` — add it as a parameter.
`planner.c_ctrl_imp_malus_eur_kwh` is already in scope in the call site in
`tasks/planning.rs`.

Also add `c_terminal_eur_kwh: f64` to `HeaterScalars` for the `milp_params` trait.

### 2.4 Add terminal term to Phase 1 objective

In `HeaterMilpContext::objective()`, in the `c_startup_eur == 0.0` branch (Phase 1):

```rust
// Terminal reward: forward value of heat stored at horizon end.
// Negative sign because we minimise: more stored energy lowers the objective.
if self.c_terminal_eur_kwh > 0.0 && n > 0 {
    obj += -self.c_terminal_eur_kwh * v.e_tank[n - 1];
}
```

### 2.5 Write unit tests (test-first)

In `VEN/src/assets/heater.rs` tests:

- `test_terminal_reward_raises_end_state` — 3-slot MILP, c_terminal enabled;
  assert `e_tank[2]` is strictly higher than with c_terminal disabled.
- `test_terminal_reward_disabled_at_zero` — c_terminal = 0.0; assert `e_tank[2]`
  matches the no-terminal baseline exactly.
- `test_terminal_reward_auto_equals_avg_plus_malus` — verify that the auto-computed
  value equals `avg_imp + malus` numerically.

### 2.6 Battery equivalent

The battery MILP context (in `VEN/src/assets/battery.rs` or equivalent) receives the
same treatment. The terminal reward for battery is:

```rust
let c_terminal_battery = match cfg.c_terminal_eur_kwh {
    Some(v) => v,
    None => avg_imp_eur_kwh * cfg.round_trip_efficiency,
};
```

Applied to `e_bat[n-1]` (stored energy at horizon end) in Phase 1 objective. The
`c_ctrl_imp_malus` is intentionally excluded for battery because it penalises import
to charge, not the value of stored energy when discharging.

Add parallel unit test: `test_battery_terminal_reward_raises_end_soc`.

### 2.7 Verify on live system

After deploy, capture `/plan` during and after the solar window. Expected:
- Tank reaches 55–70 °C during solar window (vs ~44 °C previously)
- No overnight top-up patches (tank stays above T_min without assistance)
- Heater runs at full tier (6 kW) when PV surplus is large enough

If tank consistently hits T_max (80 °C) at every solar cycle, the auto-computed
coefficient may be slightly too high for this tariff profile. Override in ven-2.yaml:
```yaml
assets:
  - type: heater
    c_terminal_eur_kwh: 0.32  # explicit: between cheap (0.30) and peak (0.38)
```

---

## Step 3 — Horizon extension benchmark (Option 3a)

**Branch:** `fix/heater-horizon-48h`
**Files:** `VEN/profiles/ven-2.yaml`
**Risk:** Low — profile-only, fully reversible. Benchmark solver time before keeping.

### Scope of impact — within ven-2, not across VENs

**Within ven-2**, all assets share a single MILP solve on a single time grid. Changing
`plan_step_s` and `plan_horizon_h` changes the grid for **all assets simultaneously** —
heater, PV, and base load. It is not heater-only.

**Across ven-1, ven-2, ven-3**: each VEN is a separate physical site with its own PV
and grid connection. They solve independently. Different `plan_step_s` values between
VENs are valid:

| VEN | Slowest asset | Fill time | 48 h justified? |
|-----|--------------|-----------|-----------------|
| ven-1 | Battery (10 kWh, 5 kW) | 2 h | No — cycles within 24 h |
| ven-2 | Heater (2000 L, 93 kWh, 6 kW) | 15.5 h | **Yes** — inter-day thermal |
| ven-3 | Heater (200 L, 3.5 kWh, 6 kW) | 35 min | No — within-day |

This step touches only `ven-2.yaml`.

### Test surface analysis

| Test suite | Profile used | Affected? | Detail |
|---|---|---|---|
| MILP unit tests (`milp_planner/tests/`) | Hardcoded 300 s or 1800 s | **No** | Never load ven-2.yaml |
| `ven_heater_tank.feature` | `test` profile (3600 s/slot) | **No** | |
| `ven_planner.feature` | `test` profile | **No** | Stale description text — see 3.2 |
| `ven_timeline.feature` | `test` or ven-2 | **No** | Timeline API resamples plan data to its own grid via the `resolution` query param — independent of `plan_step_s` |
| `timeline_grid.feature` | `test` or ven-2 | **No** | Uses explicit `resolution=10` / `resolution=30` (seconds); does not reference plan slot width |
| `use_cases.feature`, `ui_use_cases.feature` | — | **Yes** | EV departure deadlines quantised to ±10 min (see 3.4) |
| `controller/timeline.rs` unit tests | Hardcoded fixtures | **No** | Fixtures construct `Plan` directly with `step_size_s: 300`; do not load profiles; pass unchanged |
| `VEN/ui/src/pages/Controller.tsx` | — | **Yes — code change** | `hoursForward = expanded ? 24.0 : 1.0` hardcodes 24h; expanded view silently shows only half the plan (see 3.2) |
| `VEN/ui/src/__tests__/Controller.test.tsx` | — | **No** | No test asserts on the tooltip string or chart extent |

**Timeline API and UI charts: mostly not a risk.**
The `/timeline/:asset_id` endpoint resamples plan data onto a uniform grid driven by the
`resolution` query parameter, not by `plan_step_s`. Plan steps of 10 min appear as wider
rectangles in the UI timeline chart — a visual change, not a breakage. No BDD test
asserts on plan step width.

**`plan.horizon` field changes — verify UI display.**
The plan JSON response changes: `step_size_s` goes from 300 → 600, and `far_horizon`
goes from `now+24h` → `now+48h`. If the UI reads and displays these values (e.g. "planning
horizon: 24 h"), they will now show 48 h — which is correct. Verify the UI plan summary
panel after deploy.

**Stale production comment — update alongside the profile change.**
`VEN/src/controller/timeline.rs` line 85 contains the comment:
```
// plan allocations are valid for the full 5-minute slot duration
```
Update to `10-minute` (or remove the hardcoded duration and say "one plan slot").

**Heater target precision — not in the plan.**
`t_dead = secs / step_s` is also used for heater target `ready_by` deadlines
(not just EV). At 600 s steps, a heater target loses up to 10 min of deadline precision.
No BDD test for heater targets runs against ven-2 (they use `profile "test"`), so no test
failure — but this is an operational impact to document in the ven-2 profile comment.

### 3.1 Update profile

```yaml
planner:
  plan_step_s: 600        # was 300 (5 min → 10 min)
  plan_horizon_h: 48      # was 24
```

Slot count stays at 288; binary variable count is unchanged from today.

### 3.2 Fix Controller tab horizon and update stale text

**`VEN/ui/src/pages/Controller.tsx` — code change required.**

The expanded timeline view and its tooltip are hardcoded to 24h:

```tsx
// line 30 — change to match new horizon:
const hoursForward = expanded ? 48.0 : 1.0;   // was 24.0

// line 189 — update tooltip:
"Expand to 48h planning horizon"               // was "24h planning horizon"
```

Without this fix, clicking "Expand" on the Controller tab shows only the first 24h
of the plan — the second solar window (hours 24–48) is invisible, defeating the
purpose of the 48h horizon for the user. No UI unit test asserts on this string or
chart extent, so there is no test failure — it is a silent UX defect.

After the change, build and verify the UI locally:
```
cd VEN/ui && npm run build && npm test
```

Confirm the expanded Controller chart reaches the second solar window.

**Stale text in two other files (same commit):**

In `tests/features/ven_planner.feature` line 3:
```
# was:    The plan covers a 24-hour horizon as a unified slot
# becomes: The plan covers the configured planning horizon as a unified slot
```

In `VEN/src/controller/timeline.rs` line ~85:
```rust
// was:    plan allocations are valid for the full 5-minute slot duration
// becomes: plan allocations are valid for the full plan slot duration (plan_step_s)
```

Neither causes a test failure, but both are lies after the profile change.

### 3.3 Deploy and benchmark solver time

```
ssh Pi4-Server "cd /srv/docker/openadr_lab/VEN && docker compose build ven-2 && docker compose up -d ven-2"
curl -X POST http://Pi4-Server:8212/plan/trigger
```

Watch the `planner: plan adopted` log line for `solver_ms`. Target: < 40 000 ms on three
consecutive replans. If `solver_ms` > 50 000 ms, revert and proceed directly to Step 4
(3-tier grid).

### 3.4 Run E2E suite; triage EV scenarios

```
bash run_all_tests.sh --e2e
```

At 10 min steps, EV departure deadlines are quantised to ±10 min. Check
`use_cases.feature` EV scenarios against ven-2. If an EV scenario fails or charged
energy is materially below the required minimum, the precision loss is unacceptable;
proceed to Step 4 (3-tier grid preserves 5 min precision in Zone A).

### 3.5 Observe plan quality over one full 24 h cycle

Capture `/plan` at morning trough (~09:00), mid-solar (~12:00), and evening trough
(~18:00). Verify all three captures show:
- ≤ 4 switches across 288 slots
- No single-slot pulses
- All three plans have structurally similar block layout (phase-dependence gone)
- Temperature range ≥ 55 °C (from Step 2 c_terminal effect)

---

## Step 4 — 3-Tier variable-step grid (Option 3b)

**Branch:** `refactor/heater-3tier-grid`
**Files:**
- `VEN/src/controller/milp_planner/inputs.rs` — build `dt_h` as `Vec<f64>`
- `VEN/src/controller/milp_planner/solver_phase1.rs` — `dt_h` → `inputs.dt_h[t]`
- `VEN/src/controller/milp_planner/solver_phase2.rs` — same + switching penalty scaling
- `VEN/src/assets/heater.rs` — C2 dynamics with `dt_h[t]`
- Battery and EV asset constraint/objective methods — same pattern
- `VEN/src/controller/milp_planner/results.rs` — slot times from cumulative step grid
- `VEN/profiles/ven-2.yaml` — revert `plan_step_s: 300`, remove `plan_horizon_h: 48`
  (the 3-tier grid replaces the uniform 10 min horizon from Step 3)
- All MILP unit tests using `dt_h` or `9n` constraint counts — update fixtures

**Risk:** High scope but bounded. Self-contained within the `milp_planner` module;
no interface changes visible to callers outside it.

This step supersedes Step 3 (Option 3a). If Step 3 is already deployed, revert the
profile changes — Step 4 takes over horizon management.

### The 3-tier grid

Three zones with different step widths, totalling 288 slots and 48 h:

```
Zone A (0 –  8 h):  96 slots × 5 min  — near-future relay control (current precision)
Zone B (8 – 24 h):  96 slots × 10 min — overnight and next-morning scheduling
Zone C (24 – 48 h): 96 slots × 15 min — inter-day thermal strategy
```

All assets are planned in all 288 slots. Zone B and C produce complete plans for all
assets — only the timing precision of block boundaries is coarser. The `dt_h` vector:

```rust
// In build_milp_inputs():
const ZONE_A_SLOTS: usize = 96;   // 8 h at 5 min
const ZONE_B_SLOTS: usize = 96;   // 16 h at 10 min
const ZONE_C_SLOTS: usize = 96;   // 24 h at 15 min

let dt_h: Vec<f64> = (0..n).map(|t| {
    if t < ZONE_A_SLOTS        { 5.0 / 60.0  }
    else if t < ZONE_A_SLOTS + ZONE_B_SLOTS { 10.0 / 60.0 }
    else                       { 15.0 / 60.0 }
}).collect();
```

Hardcode the zone constants — they are not profile parameters. They are chosen to give
288 slots and 48 h, matching the current MILP size.

### 4.1 Core change: dt_h scalar → Vec

In `MilpInputs`, change:
```rust
// before:
pub dt_h: f64,
// after:
pub dt_h: Vec<f64>,   // len = n, one entry per slot
```

Every `× dt_h` term in `solver_phase1.rs` and `solver_phase2.rs` becomes `× inputs.dt_h[t]`.
The power balance constraint, energy cost terms, import penalty, and capacity violation
terms all use `dt_h[t]`.

### 4.2 Heater tank dynamics with per-slot dt_h

In `HeaterMilpContext::constraints()`, C2 (tank dynamics):

```rust
// before:
let p_full_dt = self.p_full_kw * dt_h;
let net_const = -self.q_dem_kw * dt_h;
// after:
let p_full_dt = self.p_full_kw * dt_h_t;   // where dt_h_t = inputs.dt_h[t]
let net_const = -self.q_dem_kw * dt_h_t;
```

The `constraints()` method receives `dt_h: f64` today; change to `dt_h: &[f64]` (slice)
and index by slot `t`. Same change for battery and EV dynamics.

### 4.3 Switching penalty scaled by dt_h[t] — mandatory companion

In `HeaterMilpContext::objective()`, Phase 2 switching term:

```rust
// before:
obj += lambda_sw_eur * v.sw[t];
// after:
obj += lambda_sw_eur * dt_h_t * v.sw[t];  // dt_h_t = dt_h[t] passed in
```

Without this scaling, Phase 2 places block boundaries preferentially near zone
transitions (Zone C switches are 3× cheaper per minute committed). The `dt_h[t]` scaling
makes each switch cost proportional to the time slot it commits — physically correct and
zone-boundary neutral.

`lambda_sw_eur` already has units EUR/switch-event. After scaling: EUR/(switch × h) × h
= EUR/switch at Zone A; EUR/(switch × h) × 3h = 3× EUR/switch at Zone C. This is the
desired asymmetry.

The `objective()` method currently receives `lambda_sw_eur: f64` and `n: usize`. Add
`dt_h: &[f64]` to the signature.

### 4.4 Plan slot start/end times from cumulative grid

In `VEN/src/controller/milp_planner/results.rs`, slot times are currently computed as
`now + t × step_s`. Replace with cumulative sum:

```rust
let mut slot_start = now;
for t in 0..n {
    let slot_end = slot_start + Duration::seconds((dt_h[t] * 3600.0) as i64);
    // build slot with [slot_start, slot_end)
    slot_start = slot_end;
}
```

### 4.5 Profile changes

Revert `plan_step_s` and `plan_horizon_h` to their defaults (or remove from ven-2.yaml)
since the 3-tier grid replaces them. The profile no longer drives the time grid directly
— the MILP module constructs it from the hardcoded zone constants.

If `plan_step_s` is still used elsewhere (e.g. for replan interval or EV deadline
calculation), those callers must be updated to use the grid-derived minimum step
(`ZONE_A_STEP_S = 300`) or a new `plan_step_s_near` parameter.

### 4.6 Test updates

**This is the largest test impact of all six steps.** The `AssetMilpContext` trait
has `dt_h: f64` in two method signatures:

```rust
fn constraints(&self, pool: &MilpVarPool, n: usize, dt_h: f64) -> Vec<Constraint>;
fn objective(&self, pool: &MilpVarPool, n: usize, dt_h: f64, ...) -> Expression;
```

Changing these to `dt_h: &[f64]` is a **breaking change to a public trait**. Every
implementor must be updated in the same compilation unit or the build fails. Confirmed
implementors:

| Implementor | File | Notes |
|---|---|---|
| `HeaterMilpContext` | `assets/heater.rs` | Primary target |
| `BatteryMilpContext` | `assets/battery.rs` | 21 dt_h occurrences in this file |
| `EvMilpContext` | `assets/ev.rs` or similar | Must be found and updated |
| `MockBatteryCtx` | `test_support/milp_mocks.rs` | 37 affected symbols in this file |
| `MockEvCtx` | `test_support/milp_mocks.rs` | Same file — update together |
| `MockHeaterCtx` | `test_support/milp_mocks.rs` | Same file — update together |
| `InfeasibleBatCtx` | `tests/planner.rs` (local) | Local test-only impl — easy to miss |

All seven must be updated atomically. The build will not compile until all match
the new trait signature. Do not merge this step until the full test suite compiles.

**Energy assertion updates in `tests/solver.rs`:** Six `dt_h` usages in this file
treat `inputs.dt_h` as a scalar (e.g. `sum * inputs.dt_h`). Change to per-slot sums:
```rust
// before:
let ev_energy: f64 = out.p_ev_kw.iter().sum::<f64>() * inputs.dt_h;
// after:
let ev_energy: f64 = out.p_ev_kw.iter().zip(inputs.dt_h.iter()).map(|(p, &dt)| p * dt).sum();
```
Approximately 3 test functions in `tests/solver.rs` need this mechanical change.

**`9n` constraint count formula is unaffected** — it depends on `n` (slot count), not
`dt_h`. Existing constraint count assertions remain valid without changes.

**`MilpInputs.dt_h` field change** — any test that constructs `MilpInputs` via a struct
literal (not through `build_milp_inputs()`) needs the field updated to a `Vec<f64>`.
Tests that go through the production `build_milp_inputs()` helper are unaffected.

Estimated mechanical changes: **~15–20 test functions across 5 files**, plus the mock
implementations in `milp_mocks.rs`. Plan a half-day for this step's test work.

New unit tests (3):
- `test_3tier_grid_has_correct_durations` — verify slot durations: first 96 = 5 min,
  next 96 = 10 min, last 96 = 15 min.
- `test_3tier_total_horizon_is_48h` — verify cumulative slot times sum to 48 × 3600 s.
- `test_switching_penalty_scales_with_dt_h` — verify Zone C penalty is 3× Zone A for
  the same switch event.

### 4.7 Deploy and verify

After deploy, the plan should show:
- 5 min resolution in Zone A (first 96 slots = first 8 h)
- Structurally identical to Option 3a quality (phase-dependence gone, ≤ 4 switches)
- EV deadline precision: ±5 min in Zone A, ±10 min in Zone B — verify with E2E suite

---

## Step 5 — Block commitment anchor (Option 7)

**Branch:** `fix/heater-block-anchor`
**Files:**
- `VEN/src/state.rs` (`HemsState`, `AppState`)
- `VEN/src/services/planning.rs` (`adopt_if_warranted`, new helpers)
- `VEN/src/tasks/planning.rs` (read anchor before solve)
- `VEN/src/controller/milp_planner/asset_port.rs` (`HeaterMilpContext`, `HeaterScalars`)
- `VEN/src/assets/heater.rs` (`HeaterMilpContext::from_state`, `declare_vars`)

**Risk:** Medium — touches state management and MILP variable declaration.

### Test surface analysis

**Struct literal breakage — coordinate with Step 2.**
Step 5 adds `anchored_kw: Vec<Option<f64>>` to `HeaterMilpContext`, the same struct
that Step 2 adds `c_terminal_eur_kwh: f64` to. If implemented on separate branches, the
struct breaks twice and is fixed twice. **Merge Steps 2 and 5 onto one branch** so the
struct is broken and fixed exactly once — update `make_may_run_ctx()` and `make_ctx()`
once with all new fields.

Critical invariant: **when `anchor_until` is None, the planner behaves identically to
today.** Run full unit + E2E suite with anchor logic present but `anchor_until = None`
before testing anchor-specific scenarios.

| What to test | New or existing? |
|---|---|
| `heater_block_end` pure function | **New** (5 tests, see 5.7) |
| `build_heater_anchor` pure function | **New** |
| Pinned variables have fixed bounds in `declare_vars` | **New** |
| Hard trigger clears anchor | **New** |
| `make_may_run_ctx` / `make_ctx` helpers compile after struct change | **Mechanical update** (if not already done in Step 2) |
| Existing `ven_heater_tank.feature` | Existing ✓ — `anchor_until = None` → no-op |
| Existing MILP unit tests | Existing ✓ — `anchored_kw = vec![None; n]` → no pinning |

### 5.1 Add anchor field to HemsState

```rust
// VEN/src/state.rs
pub struct HemsState {
    pub active_plan: Option<Plan>,
    pub anchor_until: Option<DateTime<Utc>>,   // add
    // ...
}
```

Accessors on `AppState`:
```rust
pub async fn anchor_until(&self) -> Option<DateTime<Utc>> {
    self.hems.read().await.anchor_until
}
pub async fn set_anchor_until(&self, t: Option<DateTime<Utc>>) {
    self.hems.write().await.anchor_until = t;
}
```

### 5.2 Block-end helper (pure function)

```rust
// VEN/src/services/planning.rs
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

### 5.3 Set anchor after plan adoption

In `PlanningService::adopt_if_warranted()`, after `state.set_active_plan(Some(...))`:

```rust
let anchor = heater_block_end(&plan, now);
state.set_anchor_until(anchor).await;
```

### 5.4 Clear anchor on hard triggers

In `VEN/src/tasks/planning.rs`, before the solve:

```rust
if !matches!(trigger, PlanTrigger::Periodic) {
    state.set_anchor_until(None).await;
}
```

### 5.5 Build per-slot anchor before MILP

```rust
let anchor_until = state.anchor_until().await;
let current_plan = state.active_plan().await;
let heater_anchor: Vec<Option<f64>> = build_heater_anchor(
    current_plan.as_ref(), anchor_until, now, step_s, n_slots,
);
```

Helper:
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

### 5.6 Pin variables in declare_vars

Add `anchored_kw: Vec<Option<f64>>` to `HeaterMilpContext`. In `declare_vars()`:

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
// same for z_full
```

`kw_to_tier_pair` maps kW to `(Some(0.0|1.0), Some(0.0|1.0))` using the same
quantisation as `step_inner`.

### 5.7 Unit tests (test-first)

- `test_heater_block_end_on_block`
- `test_heater_block_end_off_block`
- `test_heater_block_end_no_heater`
- `test_build_heater_anchor_pins_within_window`
- `test_anchored_vars_produce_fixed_bounds`

### 5.8 Deploy and verify

Trigger two consecutive replans 5 min apart during an active heating block. Confirm:
- Heater decision for Zone 1 (current block) does not change
- Plan beyond `anchor_until` changes freely
- Hard trigger clears anchor and produces a fresh solve

---

## Step 6 — Gate switch-count guard (Option 6)

**Branch:** `fix/heater-gate-guard`
**Files:**
- `VEN/src/entities/planner_params.rs`
- `VEN/src/profile.rs`
- `VEN/src/services/planning.rs`
- `VEN/src/tasks/planning.rs`

**Risk:** Low — additive gate logic; new parameter defaults to 0.0 (backward compatible).

### Test surface analysis

**`evaluate_acceptance_gate` signature change — mechanical update required.**
`VEN/src/services/planning.rs` contains at least 7 existing test functions that call
`evaluate_acceptance_gate` directly:
- `test_gate_rejects_below_threshold_on_periodic`
- `test_gate_accepts_when_no_current_plan`
- `test_gate_accepts_after_decay_window`
- `test_gate_accepts_epsilon_improvement`
- `test_gate_adopts_when_current_plan_slots_all_expired`
- Any others calling the function directly

All must be updated with the new `gate_switch_penalty_eur: 0.0` argument to preserve
existing semantics. The `adopt_if_warranted` function in `services/planning.rs` also
calls the gate — its signature gains the parameter and passes it through from
`PlannerParams`. The call site in `tasks/planning.rs` passes `planner.gate_switch_penalty_eur`.

| What to test | New or existing? |
|---|---|
| `count_heater_switches` helper | **New** (3 tests, see 6.5) |
| Gate rejects noisier plan below surcharge | **New** |
| Gate accepts noisier plan above surcharge | **New** |
| Cleaner plan accepted at zero surcharge | **New** |
| Hard trigger and decay bypass surcharge | **New** (2 tests) |
| Existing 7+ gate tests pass at `gate_switch_penalty_eur: 0.0` | **Mechanical update** — add `0.0` argument to each call |

### 6.1 Add parameter

In `VEN/src/entities/planner_params.rs`:
```rust
pub gate_switch_penalty_eur: f64,  // 0.0 = disabled (default)
```

In `VEN/src/profile.rs`, `PlannerConfig`:
```rust
#[serde(default)]
pub gate_switch_penalty_eur: f64,
```

### 6.2 Switch-counting helper

```rust
// VEN/src/services/planning.rs
pub fn count_heater_switches(plan: &Plan, now: DateTime<Utc>) -> usize {
    let mut count = 0usize;
    let mut prev: Option<f64> = None;
    for slot in plan.all_slots().filter(|s| s.start >= now) {
        let kw = slot.planned_kw_by_asset.get("heater").copied().unwrap_or(0.0);
        if prev.is_some_and(|p| (p - kw).abs() > 0.1) { count += 1; }
        prev = Some(kw);
    }
    count
}
```

### 6.3 Extend evaluate_acceptance_gate

Add `gate_switch_penalty_eur: f64` parameter. After computing `improvement`:

```rust
let switch_surcharge = if gate_switch_penalty_eur > 0.0 {
    if let Some(cur) = current {
        let extra = count_heater_switches(new_plan, now)
            .saturating_sub(count_heater_switches(cur, now)) as f64;
        extra * gate_switch_penalty_eur
    } else { 0.0 }
} else { 0.0 };

if fully_decayed || improvement > effective_threshold + switch_surcharge {
    true
} else { false }
```

`fully_decayed` still bypasses — decay is an escape hatch for stale plans.

### 6.4 Set value in profile

```yaml
# ven-2.yaml
planner:
  gate_switch_penalty_eur: 0.50   # matches switching_penalty_eur
```

### 6.5 Unit tests (test-first)

- `test_count_switches_empty_plan`
- `test_count_switches_one_block`
- `test_count_switches_filters_past_slots`
- `test_gate_rejects_noisier_plan_below_surcharge`
- `test_gate_accepts_noisier_plan_above_surcharge`
- `test_gate_accepts_cleaner_plan_at_zero_surcharge`
- `test_gate_hard_trigger_ignores_surcharge`
- `test_gate_decayed_accepts_despite_surcharge`

---

## Sequencing and Dependencies

```
Step 1 (config: epsilon + threshold)  ──── deploy immediately, 24 h observation

Step 2 (code: c_terminal heater+bat)  ──── test-first, independent of Step 1
Step 3 (config: 3a benchmark)         ──── after Step 1 confirmed stable
Step 4 (refactor: 3-tier grid)        ──── after Step 3 benchmarked; supersedes Step 3
Step 5 (code: block anchor)           ──── independent of Steps 1–4
Step 6 (code: gate guard)             ──── independent of Steps 1–5
```

Steps 2, 5, 6 have no shared file conflicts and can be developed in parallel.
Step 4 supersedes Step 3: if Step 3 solver timing is acceptable, Step 4 can still
proceed as the long-term architecture and Step 3 becomes a transient state.

Each step must pass the full test suite before merging:
- `wsl cargo test -p ven` locally
- E2E BDD suite on Pi4 (`bash run_all_tests.sh --e2e`)

Steps 1 and 3 additionally require a ≥ 24 h observation period to verify plan quality
across the full daily cycle before the next dependent step is started.

---

## Future Step — OpenADR VTN Flexibility Reporting

Not yet a concrete implementation task, but the three-zone stability model maps
directly onto defined OpenADR 3.1.0 report types (User Guide §§ 8.6, 8.7, 8.8):

| Zone | Information | OpenADR payload | Update trigger |
|------|-------------|-----------------|----------------|
| Real-time | Tank SOC and power limits | `STORAGE_USABLE_CAPACITY`, `STORAGE_CHARGE_LEVEL` (§8.6) | Temperature change > 2 °C |
| Zone 1 + 2 (0–8 h) | Expected power profile | `USAGE` operational forecast (§8.8), 1 h intervals | Zone 2 plan changes materially |
| Zone 3 (8–48 h) | Available load flexibility | `LOAD_SHED_DELTA_AVAILABLE` (§8.7), 1 h intervals, 48 h ahead | Hourly or temp change > 2 °C |

The §8.7 capability forecast is physics-derived — it does not depend on the MILP plan
and is stable regardless of replan frequency. It is the correct mechanism for day-ahead
DR planning by the VTN without requiring plan timing precision.

The §8.8 operational forecast is MILP-derived but should only be pushed to the VTN when
Zone 2 changes materially (new block appears, or block boundary shifts > 30 min) — not
on every 5-minute replan cycle. This decouples VTN update rate from MILP replan rate.

When this step is implemented, create a feature spec for the reporting capability rather
than adding it here.
