# Phase D — Planner Loop Refactor + PlanReason

## Context

Restructures `run_planner()` around the greedy-forward-step loop described in
§4.2 of `ven_planning_architecture.md`. The 8-phase algorithm is not discarded —
phases 2–6 become the implementation of `rules_choose()`. Every decision emits a
`PlanStep` with a `PlanReason`, providing the full audit trail required by §4.4.
`LookaheadContext` (capability trajectory + tariff lookahead) enriches rules per
asset before the loop begins.

**Prerequisites:**
- Phase A complete — `Asset` trait with `step()`, `capability()`, `simulate_forward()`
  on all assets. **`simulate_free()` and `capability_trajectory()` are NOT yet present;
  Phase D adds them as default trait impls (see CP2 — Asset trait additions).**
- Phase B complete — `ReservationLayer` with `query_asset()` and `available_cap()`
  available. `reservations: &ReservationLayer` is already a parameter of `run_planner()`
  (added in Phase B CP3).

**Touches:** `assets/mod.rs`, `controller/planner.rs`, `entities/plan.rs`, `loops.rs`

**Gate:** All existing BDD scenarios green + new scenarios asserting `PlanReason` values.

---

## What does NOT change

- `PlanTimeSlot`, `PacketAllocation`, `FlexibilityEnvelope`, `PlanWarning`, `Plan`
  struct fields — all kept. Existing BDD assertions against slot/allocation/envelope
  fields remain valid.
- Phase 1 (`build_grid`) — slot metadata grid built as before. **Exception: the
  `site_import_reduction_kw()` call added in Phase B CP3 is removed atomically with
  CP2's per-step `available_cap()` addition (see B1 note below).**
- Phase 7 (`build_envelopes`) — unchanged.
- Phase 8 (`finalize_packets`, `update_slot_flexibility`, summaries) — unchanged.
- `EnergyPacket`-based allocation logic — the math inside phases 2–6 is preserved;
  what changes is organization (unified loop vs. three separate passes) and that every
  decision now records a `PlanReason`.

### B1 — Phase B double-count fix (handled atomically in CP2)

Phase B CP3 added `reservation_layer.site_import_reduction_kw(slot.start)` inside
`build_grid()`, reducing `slot.import_cap_kw` for FIRM reservations at the slot level.

Phase D adds per-asset per-step `reservation_layer.available_cap(asset_id, phys_cap, ts)`
inside `rules_choose()`. If the Phase B reduction remained, each FIRM reservation would
be applied **twice** — once to the slot cap and once to the per-asset available cap.

**Fix (CP2, same commit as per-step check):**
- Remove the `site_import_reduction_kw()` call from `build_grid()`. Restore
  `import_cap_kw` to the raw `OadrCapacityState` value.
- The FIRM reservation effect now lives entirely in the per-step `available_cap()` call.

These two changes must land in the same commit. An intermediate state with the Phase B
reduction removed but the per-step check not yet added would leave FIRM reservations
with zero effect, breaking existing behaviour.

---

## New types

### `PlanReason` — in `entities/plan.rs`

Exact field names and variants per spec §3.2 and §4.4 of `ven_planning_architecture.md`.
`ComfortBound`, `UserOverride`, and `PolicyReserve` are included now but not fired
until Phase C / Phase F respectively.

```rust
/// The rule that fired to produce a PlanStep's setpoint (§4.4).
/// Emitted at decision time — never reconstructed after the fact.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PlanReason {
    FirmObligation  { source: ReservationSource, required_kw: f64 },
    CheapTariff     { tariff_eur_per_kwh: f64, threshold_eur_per_kwh: f64 },
    ExpensiveTariff { tariff_eur_per_kwh: f64, threshold_eur_per_kwh: f64 },
    GridImportLimit { limit_kw: f64 },
    GridExportLimit { limit_kw: f64 },
    SocCeiling      { soc_pct: f64 },
    SocFloor        { soc_pct: f64 },
    ComfortBound    { bound_type: ComfortBoundType },   // Phase C / heater
    UserOverride    { request_id: Uuid, mode: UserRequestMode },  // Phase F
    PolicyReserve   { policy_id: String },              // Phase C
    OpportunityMissed { reason: String },
    Idle,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ComfortBoundType { MinTemperature, MaxTemperature, MinSoc, MaxSoc }
```

Note: `ReservationSource` must derive `Serialize, Deserialize` to be carried inside
`FirmObligation`. Add those derives to `controller/reservation.rs`.

### `PlanStep` — in `entities/plan.rs`

Per spec §3.1. Carries `state_before` (full asset state snapshot) and `capability`
as an `AssetCapability` struct, not as flat fields.

```rust
/// One planning decision for one asset at one time step (§4.2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStep {
    pub ts:                  DateTime<Utc>,
    pub asset_id:            String,
    /// Full asset state at the start of this step (before step() is called).
    pub state_before:        AssetState,
    /// Physical capability at state_before (before reservations are applied).
    pub capability:          AssetCapability,
    pub reserved_up_kw:      f64,    // magnitude ≥ 0
    pub reserved_down_kw:    f64,    // magnitude ≥ 0
    /// Available range after reservations (avail = capability reduced by reservations).
    pub avail_max_export_kw: f64,    // ≤ 0
    pub avail_max_import_kw: f64,    // ≥ 0
    pub setpoint_kw:         f64,
    /// Actual power after physics step. May differ from setpoint_kw (e.g. SoC clamp).
    pub actual_power_kw:     f64,
    pub reason:              PlanReason,
}
```

### `LookaheadContext` — in `entities/plan.rs`

Per spec §3.3. Both tariff min/max and both ceiling/floor ETAs.

```rust
/// Pre-computed once per asset before the planning loop (§4.3).
/// Passed read-only into rules_choose(). Never modified inside the loop.
#[derive(Debug, Clone)]
pub struct LookaheadContext {
    /// Capability at each future step in free-run (from capability_trajectory()).
    pub capability_trajectory:      Vec<(DateTime<Utc>, AssetCapability)>,
    /// Cheapest import tariff in [now, now + lookahead_window].
    pub tariff_min_ahead_eur_per_kwh: f64,
    /// Most expensive import tariff in [now, now + lookahead_window].
    pub tariff_max_ahead_eur_per_kwh: f64,
    /// When asset hits its import ceiling (SoC full / comfort max) in free-run.
    /// None if not within the planning horizon.
    pub ceiling_eta: Option<DateTime<Utc>>,
    /// When asset hits its export floor (SoC empty / comfort min) in free-run.
    pub floor_eta:   Option<DateTime<Utc>>,
}
```

### `SiteContext` — in `controller/planner.rs` (internal, not serialized)

Per spec §3.4. `pv_forecast_kw` is ≤ 0 (export = negative, sign convention).

```rust
/// Running sum of already-decided setpoints for other assets at the current step.
/// Built incrementally as the inner asset loop runs (§10.4).
#[derive(Debug, Clone, Default)]
pub struct SiteContext {
    /// Sum of setpoints committed so far for other assets at this step (kW, signed).
    pub planned_others_kw: f64,
    /// Active site import limit (≥ 0). From OadrCapacityState (interim; Grid asset in future).
    pub import_limit_kw:   f64,
    /// Active site export limit (≤ 0). From OadrCapacityState.
    pub export_limit_kw:   f64,
    /// PV free-run forecast at this step (≤ 0, kW — export is negative).
    pub pv_forecast_kw:    f64,
}
```

### `Plan` — add `steps` field, update return type

Add to the existing `Plan` struct:

```rust
/// Full per-(ts × asset) audit trail. Empty until Phase D CP2 is deployed.
pub steps: Vec<PlanStep>,
```

**Note on return type:** Per spec §3.5 `run_planner()` returns `(Plan, Vec<PlanStep>)`
keeping the audit trail separate from the plan. Adopt this: return the tuple, then the
caller (loops.rs) stores steps in `plan.steps` before writing to app state, OR the
API handler reads steps from the tuple directly.
The simplest approach: return `(Plan, Vec<PlanStep>)` from `run_planner()` and
immediately assign into `plan.steps` at the call site, keeping the serialization
behaviour unchanged. This satisfies the spirit of the spec without touching `Plan`'s
existing field layout.

---

## Checkpoint 1 — Types only (additive, no behaviour change)

**What changes:**
- `entities/plan.rs`:
  - Add `PlanReason` enum (with `ComfortBoundType` helper enum)
  - Add `PlanStep` struct
  - Add `LookaheadContext` struct
  - Add `steps: Vec<PlanStep>` to `Plan`; initialize to `vec![]` at all construction sites
- `controller/planner.rs`:
  - Add `SiteContext` struct
  - Add `steps: vec![]` to the `Plan { … }` literal
- `controller/reservation.rs`:
  - Add `#[derive(Serialize, Deserialize)]` to `ReservationSource` (required for
    `PlanReason::FirmObligation { source: ReservationSource }`)

**No behaviour change.** `steps` is always empty; `run_planner()` signature unchanged.

**Gate:** `cargo build` compiles without error. No BDD run needed.

---

## Checkpoint 2 — Planner loop restructure

The core of Phase D. Three allocation passes (phases 2–4) become one unified
per-step per-asset loop. Includes the B1 double-count fix and the two missing
Asset trait methods.

### Asset trait additions (same commit as planner restructure)

Add to `assets/mod.rs` as default impls on the `Asset` trait (per spec §1.1):

```rust
/// Free-run: asset follows natural idle behaviour with no external setpoint.
/// Default: simulate_forward with setpoint = 0.0 for the full duration.
/// Assets with non-zero idle behaviour (heater thermostat, PV irradiance)
/// override this.
fn simulate_free(&self, initial: &AssetState, duration: Duration) -> Trajectory {
    let now = Utc::now();
    self.simulate_forward(initial, &[(now, 0.0), (now + duration, 0.0)])
}

/// Capability at each resolution step in free-run.
/// Used by precompute_lookahead() to build LookaheadContext.
fn capability_trajectory(
    &self,
    initial:    &AssetState,
    duration:   Duration,
    resolution: Duration,
) -> Vec<(DateTime<Utc>, AssetCapability)> {
    let now  = Utc::now();
    let traj = self.simulate_free(initial, duration);
    traj.points
        .into_iter()
        .filter(|p| (p.ts - now).num_seconds() % resolution.num_seconds() == 0)
        .map(|p| (p.ts, self.capability(&p.state)))
        .collect()
}
```

Also add `AssetConfig::simulate_free()` and `AssetConfig::capability_trajectory()`
dispatch methods (same pattern as existing `AssetConfig::step()` / `::capability()`).

### Updated `run_planner()` signature

Per spec §3.5, with `&SimState` as the interim asset source (see D5 note at end):

```rust
pub fn run_planner(
    assets:       &SimState,            // interim: &[&dyn Asset] once AssetEntry impls Asset
    tariffs:      &TariffTimeSeries,
    packets:      &[EnergyPacket],
    capacity:     &OadrCapacityState,   // interim: removed once Grid virtual asset exists
    reservations: &ReservationLayer,    // already present from Phase B
    profile:      &Profile,
    now:          DateTime<Utc>,
    trigger:      PlanTrigger,
) -> (Plan, Vec<PlanStep>)
```

`asset_forecasts: &HashMap<String, TimeSeries>` is removed. PV forecast for
`build_grid()` is now derived via `cfg.simulate_free(&entry.state, horizon)` on
the PV asset entry inside `build_grid()`.

Call site in `loops.rs`:
- Remove the `asset_forecasts` HashMap construction block (the `iter_assets()` +
  `cfg.forecast()` loop)
- Pass `&*sim_guard` as `assets`
- Unpack the `(plan, steps)` tuple; assign `plan.steps = steps`

### B1 fix — remove `site_import_reduction_kw()` from `build_grid()` (this commit)

In `build_grid()`, replace:
```rust
let effective_import_cap_kw =
    (import_cap - reservations.site_import_reduction_kw(start)).max(0.0);
// …
import_cap_kw: effective_import_cap_kw,
```
with:
```rust
import_cap_kw: import_cap,   // raw OadrCapacityState value; FIRM reduction via available_cap() below
```

FIRM reservation effect now lives entirely in `rules_choose()` via `available_cap()`.

### Pre-loop: `precompute_lookahead()`

```rust
fn precompute_lookahead(
    sim:              &SimState,
    tariffs:          &TariffTimeSeries,
    now:              DateTime<Utc>,
    lookahead_window: Duration,
    resolution:       Duration,
) -> HashMap<String, LookaheadContext>
```

For each `(entry, cfg)` in `sim.iter_assets()`:
1. `cfg.capability_trajectory(&entry.state, lookahead_window, resolution)` →
   zip with timestamps `[now + resolution, now + 2×resolution, …]`
2. Query tariff series over `[now, now + lookahead_window]` → `tariff_min` + `tariff_max`
3. Walk trajectory: find first step where `cap.max_import_kw ≈ 0` → `ceiling_eta`;
   find first step where `cap.max_export_kw ≈ 0` → `floor_eta`

**Profile extension:** Add `lookahead_h: f64` to `PlannerConfig` in `profile.rs`.
Default: `2.0`. Existing profiles without this field get the default via `#[serde(default)]`.

### Asset processing order within each time step

Per spec §3.4 — uncontrollable assets first, Grid second, controllable last:

1. **PV** — `cfg.step(&state, 0.0, slot_dur)` → `site_ctx.pv_forecast_kw = actual_kw` (≤ 0)
2. **BaseLoad** — `cfg.step(&state, 0.0, slot_dur)` → add to `site_ctx.planned_others_kw`
3. **Grid** — not yet a real asset; `site_ctx.import_limit_kw` / `export_limit_kw` taken
   from `slot.import_cap_kw` / `slot.export_cap_kw` (legacy OadrCapacityState path)
4. **EV** — `rules_choose()` called
5. **Battery** — `rules_choose()` called
6. **Heater** — `rules_choose()` called

### `rules_choose()` — unified decision function

```rust
fn rules_choose(
    asset_id:   &str,
    phys_cap:   AssetCapability,
    avail_cap:  AssetCapability,   // phys_cap reduced by reservation_layer.available_cap()
    tariff_t:   f64,               // import tariff at this step (€/kWh)
    slot:       &PlanTimeSlot,
    packets:    &[EnergyPacket],
    allocated:  &HashMap<Uuid, f64>,
    site_ctx:   &SiteContext,
    lookahead:  &LookaheadContext,
    now:        DateTime<Utc>,
) -> (f64 /* setpoint_kw */, PlanReason)
```

Rules fire in priority order (first match wins):

| Priority | Rule | Condition | Setpoint | Reason variant |
|---|---|---|---|---|
| 1 | Firm obligation | `avail_cap` fully zeroed by reservation | 0.0 | `FirmObligation { source, required_kw }` |
| 2 | Grid import limit | `site_ctx.planned_others_kw + desired_kw > site_ctx.import_limit_kw` | clamped | `GridImportLimit` |
| 3 | Grid export limit | net export would exceed limit | clamped | `GridExportLimit` |
| 4 | SoC / comfort ceiling | `avail_cap.max_import_kw ≈ 0` (storage full or comfort max) | 0.0 | `SocCeiling` / `ComfortBound` |
| 5 | SoC / comfort floor | discharging would breach floor | 0.0 | `SocFloor` / `ComfortBound` |
| 6 | Cheap tariff + packet | `tariff_t ≤ lookahead.tariff_min_ahead_eur_per_kwh × eff && comfort_bid ≥ tariff_t` | `desired_power_kw` | `CheapTariff` |
| 7 | Deadline pressure | `time_pressure ≥ 2.0` for active packet | `desired_power_kw` | `FirmObligation { source: UserRequest, … }` |
| 8 | Surplus opportunity | `slot.surplus_available_kw > 0 && comfort_bid ≥ export_tariff` | surplus-capped | `CheapTariff` |
| 9 | Battery arb cheap | `tariff_t < median × sqrt(eff)` | charge power | `CheapTariff` |
| 10 | Battery arb expensive | `tariff_t > median / sqrt(eff)` | discharge power | `ExpensiveTariff` |
| 11 | Opportunity missed | packet active but no eligible slot | 0.0 | `OpportunityMissed` |
| 12 | Idle | no active packet, no arbitrage trigger | 0.0 | `Idle` |

Rules 6–8 replicate Phase 2+3 (`allocate_consumption`) logic.
Rules 9–10 replicate Phase 4 (`allocate_battery`) logic.
Rules 1–5 are new gates from the reservation layer and physics caps.

Note on Rule 1: call `reservation_layer.available_cap(asset_id, phys_cap, ts)` to
get `avail_cap`. Read the highest-priority matching `Reservation.source` for the
`FirmObligation.source` field. If `avail_cap.max_import_kw ≤ 0` and
`avail_cap.max_export_kw ≥ 0`, the asset is fully reserved — Rule 1 fires.

Note on Rule 7: deadline-pressure charging is modelled as a user obligation, so
`FirmObligation { source: UserRequest { request_id }, required_kw }` is the correct
variant (not a separate `PacketDeadline` — that variant is not in the spec).

### Loop structure in `run_planner()`

```
// Pre-loop
lookaheads = precompute_lookahead(&sim, tariffs, now, lookahead_window, step_dur)
build_grid → firm_slots + flexible_slots
             (B1 fix: import_cap_kw is raw OadrCapacityState value here)
terminal_pkts, pkts (filter as before)
median_tariff (over all firm_slots for battery arbitrage threshold)

// Per-step state — initialized from sim.iter_assets()
asset_states: HashMap<String, AssetState>   (mutable, propagates across steps)
asset_cfgs:   HashMap<String, &AssetConfig> (read-only, built once)

slot_allocated: HashMap<Uuid, f64>
plan_steps:     Vec<PlanStep>

for (step_idx, slot) in firm_slots.iter_mut().enumerate():
    ts = slot.start

    site_ctx = SiteContext {
        planned_others_kw: 0.0,
        import_limit_kw:   slot.import_cap_kw,   // raw capacity, not reservation-reduced
        export_limit_kw:   slot.export_cap_kw,
        pv_forecast_kw:    0.0,
    }

    for asset_id in [pv, base_load, ev, battery, heater]:
        if not in profile: continue

        state    = asset_states[asset_id]
        cfg      = asset_cfgs[asset_id]
        phys_cap = cfg.capability(&state)
        avail_cap = reservation_layer.available_cap(asset_id, phys_cap, ts)
        res       = reservation_layer.query_asset(asset_id, ts)

        (setpoint_kw, reason) = if is_uncontrollable(asset_id):
            (_, power) = cfg.step(&state, 0.0, slot_dur)
            if asset_id == "pv": site_ctx.pv_forecast_kw = power  // already ≤ 0
            (power, PlanReason::Idle)
        else:
            rules_choose(asset_id, phys_cap, avail_cap,
                         slot.import_tariff_eur_kwh, slot, &pkts,
                         &slot_allocated, &site_ctx, &lookaheads[asset_id], ts)

        (next_state, actual_kw) = cfg.step(&state, setpoint_kw, slot_dur)
        asset_states[asset_id] = next_state

        plan_steps.push(PlanStep {
            ts,
            asset_id: asset_id.to_string(),
            state_before: state,
            capability: phys_cap,
            reserved_up_kw:   res.reserved_up_kw,
            reserved_down_kw: res.reserved_down_kw,
            avail_max_export_kw: avail_cap.max_export_kw,
            avail_max_import_kw: avail_cap.max_import_kw,
            setpoint_kw,
            actual_power_kw: actual_kw,
            reason,
        })

        update_slot_from_step(slot, asset_id, actual_kw, &pkts, &mut slot_allocated, slot_h)
        site_ctx.planned_others_kw += actual_kw

// Flexible slots: same loop structure (no PlanStep emitted initially)

// Phase 7 + Phase 8: unchanged
envelopes = build_envelopes(...)
finalize_packets(...)
update_slot_flexibility(...)

return (plan, plan_steps)    // caller assigns plan.steps = plan_steps
```

### `update_slot_from_step()` — slot bookkeeping helper

Encapsulates slot mutation currently done inline in `allocate_consumption` and
`allocate_battery`. Appends a `PacketAllocation` for packet-driven assets; appends
a battery allocation (packet_id = Uuid::nil()) for the battery. Updates
`slot.net_import_kw`, `slot.net_export_kw`, `slot.surplus_available_kw`.

**Gate (CP2):** `cargo build` compiles + `docker compose run --build test-runner`
all existing BDD scenarios green. Numeric outcomes must match — same math, restructured.

---

## Checkpoint 3 — API exposure + new BDD scenarios

### Expose `plan.steps` in `GET /plan`

`plan.steps` is in the `Plan` struct and serializes automatically.
For `GET /plan?summary`, explicitly skip `steps` (return a trimmed struct or
set `steps` to `vec![]` in the summary path).

### New BDD scenarios (`tests/features/plan_reasons.feature`)

**Scenario: Battery charges on cheap tariff**
```gherkin
Given import tariff is set below the median for the planning window
When a plan is triggered
Then at least one PlanStep for asset "battery" has reason kind "CHEAP_TARIFF"
And that step's setpoint_kw is greater than 0.0
```

**Scenario: Battery discharges on expensive tariff**
```gherkin
Given import tariff is set above the median for the planning window
When a plan is triggered
Then at least one PlanStep for asset "battery" has reason kind "EXPENSIVE_TARIFF"
And that step's setpoint_kw is less than 0.0
```

**Scenario: EV charges under deadline pressure**
```gherkin
Given an EV packet with a tight deadline and low SoC (time_pressure >= 2.0)
When a plan is triggered
Then at least one PlanStep for asset "ev" has reason kind "FIRM_OBLIGATION"
And that step's setpoint_kw is greater than 0.0
```

**Scenario: No active packets yields Idle**
```gherkin
Given no active EnergyPackets exist and tariff is at median
When a plan is triggered
Then all PlanSteps for asset "battery" have reason kind "IDLE"
```

**Scenario: Firm reservation blocks asset**
```gherkin
Given a VTN FIRM event reserving full battery capacity covering the plan window
When a plan is triggered
Then at least one PlanStep for asset "battery" has reason kind "FIRM_OBLIGATION"
And that step's setpoint_kw is 0.0
```
*(Phase B is complete — `parse_firm_reservations()` is wired. No blocker.)*

**Gate (CP3):** All scenarios green including 5 new reason scenarios.

---

## D5 — deliberate deviations from spec §3.5

| Spec | This plan | Reason |
|---|---|---|
| `assets: &[&dyn Asset]` | `assets: &SimState` | `AssetConfig` does not implement `Asset` (no `id()` / `current_state()`); `AssetEntry` would need to implement `Asset`. Deferred to a future Phase A extension. |
| `capacity` param absent | `capacity: &OadrCapacityState` kept | Grid virtual asset not yet implemented. Per spec §2 note, `OadrCapacityState` → `slot.import_cap_kw` is the documented interim path. |
| Returns `(Plan, Vec<PlanStep>)` | Same — adopted | Callers assign `plan.steps = steps` after the call. |

The `ven_asset_interface_spec.md` §3.5 should be annotated with a note that
`&SimState` is the current interim signature and `&[&dyn Asset]` is the target
once `AssetEntry` implements the `Asset` trait.

---

## Risk register

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| B1 removal + per-step check not atomic → FIRM reservations have zero effect | High if split | Regression | Enforce in single commit; gated by BDD FirmObligation scenario |
| Asset processing order changes allocation outcome vs. current phases 2–4 | Medium | Failing BDD | Run BDD after every sub-step; reorder if needed |
| `simulate_free()` default (setpoint=0) differs from `cfg.forecast()` for PV/heater | Low–Medium | Slot pv_forecast_kw drift | Compare values before/after; PV/heater override `simulate_free()` if needed |
| `rules_choose()` fires rules in different order than old phases → numeric parity | Medium | Failing BDD | BDD suite is the gate; ensure exact math equivalence in rules 6–10 |
| `asset_forecasts` removal breaks `loops.rs` build | Low | Compile error | Fix call site in same commit as signature change |

---

## Files changed

| File | CP | Change |
|---|---|---|
| `entities/plan.rs` | CP1 | Add `PlanReason`, `ComfortBoundType`, `PlanStep`, `LookaheadContext`; add `steps: Vec<PlanStep>` to `Plan` |
| `controller/planner.rs` | CP1 | Add `SiteContext`; add `steps: vec![]` to `Plan` construction |
| `controller/reservation.rs` | CP1 | Add `Serialize, Deserialize` derives to `ReservationSource` |
| `assets/mod.rs` | CP2 | Add `simulate_free()` and `capability_trajectory()` default impls to `Asset` trait; add dispatch methods to `AssetConfig` |
| `profile.rs` | CP2 | Add `lookahead_h: f64` to `PlannerConfig` (default 2.0 via `#[serde(default)]`) |
| `controller/planner.rs` | CP2 | Updated `run_planner()` signature; remove `site_import_reduction_kw()` (B1); add `precompute_lookahead()`, `rules_choose()`, `update_slot_from_step()`; restructure main loop; return `(Plan, Vec<PlanStep>)` |
| `loops.rs` | CP2 | Remove `asset_forecasts` build; pass `&*sim_guard`; unpack `(plan, steps)` tuple |
| `docs/architecture/ven_asset_interface_spec.md` | CP2 | Add interim-signature note to §3.5 |
| `tests/features/plan_reasons.feature` | CP3 | New BDD feature with 5 PlanReason scenarios |
| `tests/steps/plan_reason_steps.py` | CP3 | Step implementations |

---

## Success criteria

- `cargo build` compiles without error after each checkpoint
- After CP2: all existing BDD scenarios pass unchanged
- After CP3: all scenarios pass including 5 new `PlanReason` scenarios
- Single commit per checkpoint; tag: `refactor(ven): Phase D — planner loop + PlanReason`
