# Phase D — Planner Loop Refactor + PlanReason

## Context

Restructures `run_planner()` around the greedy-forward-step loop described in
§4.2 of `ven_planning_architecture.md`. The 8-phase algorithm is not discarded —
phases 2–6 become the implementation of `rules_choose()`. Every decision emits a
`PlanStep` with a `PlanReason`, providing the full audit trail required by §4.4.
`LookaheadContext` (capability trajectory + tariff lookahead) enriches rules per
asset before the loop begins.

**Prerequisites:**
- Phase A complete — `Asset` trait with `step()`, `capability()`,
  `capability_trajectory()` available on all assets
- Phase B complete — `ReservationLayer` with `query(t) -> Vec<Reservation>` available

**Touches:** `controller/planner.rs`, `entities/plan.rs`, `loops.rs` (call site)

**Gate:** All existing BDD scenarios green + new scenarios asserting `PlanReason` values.

---

## What does NOT change

- `PlanTimeSlot`, `PacketAllocation`, `FlexibilityEnvelope`, `PlanWarning`,
  `Plan` — all kept. Existing BDD assertions against slot/allocation/envelope
  fields remain valid.
- Phase 1 (`build_grid`) — builds the slot metadata grid as before.
  Baseline, PV forecast, tariff, capacity limits remain slot-level inputs.
- Phase 7 (`build_envelopes`) — unchanged.
- Phase 8 (`finalize_packets`, `update_slot_flexibility`, summaries) — unchanged.
- `EnergyPacket`-based allocation logic — the math inside phases 2–6 is
  preserved; what changes is how it is organized (one unified loop vs.
  three separate passes) and that it now records a `PlanReason` per decision.

---

## New types (all in `entities/plan.rs`)

### `PlanReason` — audit enum

```rust
/// The rule that fired to produce a PlanStep's setpoint (§4.4).
/// Emitted at decision time — never reconstructed after the fact.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PlanReason {
    FirmObligation     { source_label: String, required_kw: f64 },
    CheapTariff        { tariff_eur_kwh: f64, threshold_eur_kwh: f64 },
    ExpensiveTariff    { tariff_eur_kwh: f64, threshold_eur_kwh: f64 },
    GridImportLimit    { limit_kw: f64 },
    GridExportLimit    { limit_kw: f64 },
    SocCeiling         { soc_pct: f64 },
    SocFloor           { soc_pct: f64 },
    PacketDeadline     { packet_id: Uuid, time_pressure: f64 },
    SurplusOpportunity { surplus_kw: f64 },
    OpportunityMissed  { reason: String },
    Idle,
}
```

Notes:
- `UserOverride` and `PolicyReserve` variants are prepared but not fired in
  Phase D (those sources land in Phase B's `ReservationLayer`). They appear
  as `FirmObligation { source_label: "VTN_FIRM" | "POLICY" | "USER_REQUEST" }`.
- `serde(tag = "kind")` makes the JSON discriminant explicit for UI consumption.

### `PlanStep` — per-(ts × asset) audit record

```rust
/// One planning decision for one asset at one time step (§4.2).
/// The full audit trail — every setpoint has a matching PlanStep.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStep {
    pub ts:               DateTime<Utc>,
    pub asset_id:         String,

    // Capability at this step (physics, not reduced by reservations)
    pub phys_max_import_kw: f64,
    pub phys_max_export_kw: f64,

    // After reservation layer subtraction
    pub avail_max_import_kw: f64,
    pub avail_max_export_kw: f64,

    // Amount reserved by the reservation layer at this step
    pub reserved_up_kw:   f64,    // holds back import capacity (≥ 0)
    pub reserved_down_kw: f64,    // holds back export capacity (≥ 0)

    // Decision
    pub setpoint_kw:      f64,    // setpoint sent to asset.step()
    pub actual_power_kw:  f64,    // power returned by asset.step() (may differ)
    pub reason:           PlanReason,
}
```

### `LookaheadContext` — per-asset read-only enrichment

```rust
/// Pre-computed before the planning loop. Passed to rules_choose() as context.
/// Never modified inside the loop (§4.3).
#[derive(Debug, Clone)]
pub struct LookaheadContext {
    /// capability_trajectory() in free-run for [now, now + lookahead_window]
    pub capability_trajectory: Vec<(DateTime<Utc>, AssetCapability)>,
    /// Minimum tariff in [now, now + lookahead_window]
    pub tariff_minimum_ahead_eur_kwh: f64,
    /// When does storage SoC hit ceiling at idle? None if not a storage asset.
    pub soc_ceiling_eta: Option<DateTime<Utc>>,
}
```

### `SiteContext` — accumulated within each time step

```rust
/// Running sum of already-decided setpoints for other assets at the current
/// time step. Built incrementally as the inner asset loop runs (§10.4).
#[derive(Debug, Clone, Default)]
pub struct SiteContext {
    /// Sum of setpoints decided so far for other assets at this step (kW)
    pub planned_others_kw: f64,
    /// Effective site import limit (from capacity state / VTN events)
    pub import_limit_kw:   f64,
    /// Effective site export limit (from capacity state / VTN events)
    pub export_limit_kw:   f64,
    /// PV generation forecast for this step (kW, positive = generation)
    pub pv_forecast_kw:    f64,
}
```

### `Plan` — add `steps` field

Add to the existing `Plan` struct:

```rust
/// Full per-(ts × asset) audit trail. One entry per (time step × controllable asset).
/// Empty until Phase D is deployed.
pub steps: Vec<PlanStep>,
```

Serialize as `"steps"` in JSON. Default-initialize to `vec![]` in any code
that constructs `Plan` before Phase D CP2 wires it up.

---

## Checkpoint 1 — Types only (additive, no behaviour change)

**What changes:**
- `entities/plan.rs`:
  - Add `PlanReason` enum
  - Add `PlanStep` struct
  - Add `LookaheadContext` struct (can live here or in `controller/planner.rs`
    — keep in `plan.rs` for serialization proximity)
  - Add `SiteContext` struct (internal to planner, no serialization needed —
    keep in `controller/planner.rs`)
  - Add `steps: Vec<PlanStep>` field to `Plan`; initialize to `vec![]`
  - Update any `Plan { ... }` construction sites to include `steps: vec![]`

**No behaviour change.** The planner still runs identically; `steps` is always empty.

**Gate:** `cargo build` compiles without error. No BDD run needed.

---

## Checkpoint 2 — Planner loop restructure

This is the core of Phase D. The three separate allocation passes (phases 2–4)
are replaced by one unified per-step per-asset loop. The math is the same;
the structure changes.

### New `run_planner()` signature

```rust
pub fn run_planner(
    tariffs:           &TariffTimeSeries,
    packets:           &[EnergyPacket],
    capacity:          &OadrCapacityState,
    profile:           &Profile,
    now:               DateTime<Utc>,
    trigger:           PlanTrigger,
    assets:            &SimState,          // replaces asset_forecasts HashMap
    reservation_layer: &ReservationLayer,  // Phase B output
) -> Plan
```

`asset_forecasts: &HashMap<String, TimeSeries>` is removed. Asset forecasts
are now derived directly via `asset.simulate_free()` and
`asset.capability_trajectory()`. The PV forecast for the slot grid still uses
the history buffer's last-known value extended by `simulate_free()`.

The call site in `loops.rs` is updated to pass `&app_state.sim` and
`&app_state.reservation_layer`.

### Pre-loop: `precompute_lookahead()`

```rust
fn precompute_lookahead(
    assets: &SimState,
    tariffs: &TariffTimeSeries,
    now: DateTime<Utc>,
    lookahead_window: Duration,     // from profile.planner.lookahead_h
) -> HashMap<String, LookaheadContext>
```

For each asset in `SimState`:
1. Call `asset_config.capability_trajectory(current_state, lookahead_window, step_dur)`
   to get `capability_trajectory`.
2. Query tariff series for minimum import tariff in `[now, now + lookahead_window]`
   → `tariff_minimum_ahead_eur_kwh`.
3. For storage assets (Battery, EV): estimate when SoC hits ceiling in free-run
   → `soc_ceiling_eta`.
4. For non-storage assets: `soc_ceiling_eta = None`.

**Profile extension:** Add `lookahead_h: f64` to the `PlannerConfig` struct in
`profile.rs`. Default: `2.0` (2 h lookahead). Existing profiles without this
field get the default.

### Asset processing order within each step

Process assets in this order at every time step to populate `SiteContext`:

1. **BaseLoad** (uncontrollable, fixed profile) — setpoint = actual output from
   `simulate_free()`. `SiteContext.planned_others_kw` updated.
2. **PV** (uncontrollable, irradiance model) — setpoint = current forecast.
   `SiteContext.pv_forecast_kw` set. `SiteContext.planned_others_kw` updated.
3. **EV** (controllable, packet-driven) — `rules_choose()` called.
4. **Battery** (controllable, arbitrage) — `rules_choose()` called.
5. **Heater** (controllable, comfort-bound) — `rules_choose()` called.

Assets not present in the profile are skipped silently.

### `rules_choose()` — unified decision function

```rust
fn rules_choose(
    asset_id:    &str,
    phys_cap:    AssetCapability,
    avail_cap:   AssetCapability,   // phys_cap reduced by reservations
    tariff_t:    f64,               // import tariff at this step (€/kWh)
    slot:        &PlanTimeSlot,     // full slot context (surplus, caps, etc.)
    packets:     &[EnergyPacket],   // active non-terminal packets for this asset
    allocated:   &HashMap<Uuid, f64>,  // energy already allocated this cycle
    site_ctx:    &SiteContext,
    lookahead:   &LookaheadContext,
    now:         DateTime<Utc>,
) -> (f64 /* setpoint_kw */, PlanReason)
```

Rules fire in priority order (first match wins):

| Priority | Rule | Condition | Setpoint | Reason |
|---|---|---|---|---|
| 1 | Firm obligation | `avail_cap` is zero (fully reserved by reservation layer) | 0.0 or reserved level | `FirmObligation` |
| 2 | Grid import limit | `site_ctx.planned_others_kw + desired_kw > site_ctx.import_limit_kw` | clamp to headroom | `GridImportLimit` |
| 3 | Grid export limit | net export would exceed limit | clamp | `GridExportLimit` |
| 4 | SoC ceiling (storage) | `state.soc_pct >= max_soc` | 0.0 | `SocCeiling` |
| 5 | SoC floor (storage) | discharging would breach `min_soc` | 0.0 | `SocFloor` |
| 6 | Packet deadline pressure | `time_pressure >= 2.0` for any active packet | `desired_power_kw` | `PacketDeadline` |
| 7 | Cheap tariff + packet | `tariff_t <= tariff_minimum_ahead * eff && comfort_bid >= tariff_t` | `desired_power_kw` | `CheapTariff` |
| 8 | Surplus opportunity | `slot.surplus_available_kw > 0 && comfort_bid >= export_tariff` | surplus-capped power | `SurplusOpportunity` |
| 9 | Battery arbitrage cheap | `tariff_t < median_tariff * sqrt(efficiency)` | charge power | `CheapTariff` |
| 10 | Battery arbitrage expensive | `tariff_t > median_tariff / sqrt(efficiency)` | discharge power | `ExpensiveTariff` |
| 11 | Opportunity missed | Packet active but no eligible slot | 0.0 | `OpportunityMissed` |
| 12 | Idle | No active packet, no arbitrage trigger | 0.0 | `Idle` |

Rules 6–8 replicate the current Phase 2+3 (allocate_consumption) logic.
Rules 9–10 replicate Phase 4 (allocate_battery) logic.
Rules 1–5 are new gates from the reservation layer.

**Note on `FirmObligation`:** The reservation layer (Phase B) provides
`reservation_layer.query(t)`. If a reservation covers this asset at step `t`,
subtract its reserved magnitude from `phys_cap` to produce `avail_cap`.
If `avail_cap.max_import_kw <= 0.0` and `avail_cap.max_export_kw >= 0.0`
(both clamped to zero), Rule 1 fires immediately.

### Loop structure in `run_planner()`

```
// Pre-loop
lookaheads = precompute_lookahead(assets, tariffs, now, lookahead_window)
build_grid → firm_slots + flexible_slots (unchanged from Phase 1)
terminal_pkts, pkts (filter as before)
median_tariff (computed over all firm_slots for battery arbitrage threshold)

// Per-step state: SoC / temperature etc. — initialized from asset.current_state()
asset_states: HashMap<String, AssetState>  (mutable, propagates across steps)

// Slot-level allocation trackers (replaces the separate allocated HashMap)
slot_allocated: HashMap<Uuid, f64>

// Audit trail
plan_steps: Vec<PlanStep>

for (step_idx, slot) in firm_slots.iter_mut().enumerate():
    ts = slot.start
    reservations_t = reservation_layer.query(ts)

    site_ctx = SiteContext {
        planned_others_kw: 0.0,
        import_limit_kw: slot.import_cap_kw,
        export_limit_kw: slot.export_cap_kw,
        pv_forecast_kw: 0.0,
    }

    for asset_id in ASSET_ORDER:        // [base_load, pv, ev, battery, heater]
        if not in profile: continue

        state = asset_states[asset_id]
        phys_cap = asset_config.capability(&state)
        reserved = reservations_t.for_asset(asset_id)
        avail_cap = reduce(phys_cap, reserved)
        lookahead = &lookaheads[asset_id]

        (setpoint_kw, reason) = if is_uncontrollable(asset_id):
            // PV / BaseLoad: use simulate_free output
            power = asset_config.simulate_free(&state, slot_dur).last_power_kw()
            update site_ctx (pv_forecast_kw or planned_others_kw)
            (power, PlanReason::Idle)
        else:
            rules_choose(asset_id, phys_cap, avail_cap, slot.import_tariff_eur_kwh,
                         slot, &pkts, &slot_allocated, &site_ctx, lookahead, ts)

        (next_state, actual_kw) = asset_config.step(&state, setpoint_kw, slot_dur)
        asset_states[asset_id] = next_state

        // Record audit step
        plan_steps.push(PlanStep {
            ts, asset_id,
            phys_max_import_kw: phys_cap.max_import_kw,
            phys_max_export_kw: phys_cap.max_export_kw,
            avail_max_import_kw: avail_cap.max_import_kw,
            avail_max_export_kw: avail_cap.max_export_kw,
            reserved_up_kw: reserved.up_kw,
            reserved_down_kw: reserved.down_kw,
            setpoint_kw,
            actual_power_kw: actual_kw,
            reason,
        })

        // Update slot totals (mirrors what allocate_consumption/battery did)
        update_slot_from_step(slot, asset_id, actual_kw, setpoint_kw, &pkts, ...)
        update_slot_allocated(&mut slot_allocated, ...)
        site_ctx.planned_others_kw += actual_kw

// Flexible slots: run rules_choose as above but in FLEXIBLE slot context
// (no PlanStep emitted for flexible slots initially — can be added later)

// Phase 7 + Phase 8: unchanged
envelopes = build_envelopes(...)
finalize_packets(...)
update_slot_flexibility(...)
```

### `update_slot_from_step()` — slot bookkeeping

Encapsulates the slot mutation currently done inline in `allocate_consumption`
and `allocate_battery`. Appends a `PacketAllocation` to `slot.allocations` when
the step produces a nonzero setpoint for a packet-driven asset. For battery,
appends a battery allocation as before (packet_id = Uuid::nil()).

This function is new but small. It ensures `slot.net_import_kw`,
`slot.net_export_kw`, and `slot.surplus_available_kw` are updated after each
asset step — so the `SiteContext` accumulated within the step reflects actual
decisions, not just setpoints.

### Handling `asset_forecasts` removal from the call site

`loops.rs` currently calls `run_planner(..., asset_forecasts)` where
`asset_forecasts` is built from `sim.assets` history. After this change:
- Remove the `asset_forecasts` HashMap construction.
- Pass `&*sim_guard` (the `SimState`) and `&*reservation_layer` directly.
- `build_grid` now gets PV forecast from `assets.asset("pv").map(|e| e.last_power_kw)`;
  the simulate_free path is used for future slots.

**Gate (CP2):** `cargo build` compiles + `docker compose run --build test-runner`
runs all existing BDD scenarios green. No scenario should change numeric
outcome — the math is identical, only the call structure changed.

---

## Checkpoint 3 — API exposure + new BDD scenarios

### Expose `plan.steps` in `GET /plan`

`plan.steps` is already in the `Plan` struct (serializes to JSON automatically).
The full `GET /plan` response now includes the `"steps"` array.

For the summary endpoint (`GET /plan?summary`), exclude `steps` from the
summary response (keep it lean — steps can be many rows).

Optionally add `GET /plan/steps` as an alias returning only the steps array if
needed for the UI. Defer to Phase E/F if not immediately required.

### New BDD scenarios

Add to `tests/features/hems_controller.feature` (or a new
`tests/features/plan_reasons.feature`):

**Scenario: Battery charges on cheap tariff — CheapTariff reason**
```gherkin
Given a plan is triggered with cheap import tariff below median
When I GET /plan
Then at least one PlanStep for asset "battery" has reason kind "CHEAP_TARIFF"
And that step's setpoint_kw is positive
```

**Scenario: Battery discharges on expensive tariff — ExpensiveTariff reason**
```gherkin
Given a plan is triggered with expensive import tariff above median
When I GET /plan
Then at least one PlanStep for asset "battery" has reason kind "EXPENSIVE_TARIFF"
And that step's setpoint_kw is negative
```

**Scenario: EV charges under deadline pressure — PacketDeadline reason**
```gherkin
Given an EV packet with time_pressure >= 2.0 (tight deadline, low SoC)
When I GET /plan
Then at least one PlanStep for asset "ev" has reason kind "PACKET_DEADLINE"
```

**Scenario: No active packets — Idle reason**
```gherkin
Given no active EnergyPackets exist
When a plan is triggered
Then all PlanSteps for asset "battery" have reason kind "IDLE" or "CHEAP_TARIFF" or "EXPENSIVE_TARIFF"
And no PlanStep has reason kind "PACKET_DEADLINE"
```

**Scenario: Firm reservation blocks asset — FirmObligation reason**
```gherkin
Given a VTN FIRM event reserving full battery capacity at 14:00
When a plan is generated covering 14:00
Then at least one PlanStep for asset "battery" at 14:00 has reason kind "FIRM_OBLIGATION"
And that step's setpoint_kw is 0.0
```
*(This scenario requires Phase B to be complete and the reservation layer wired.)*

**Gate (CP3):** All existing BDD scenarios green + new reason scenarios green.

---

## Risk register

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| Asset processing order changes allocation outcome | Medium | Breaking BDD | Run BDD suite after every sub-change; adjust order if needed |
| `simulate_free()` for PV gives different forecast than old HashMap | Low | Slot-level numeric drift | Compare `pv_forecast_kw` values before/after; adjust sim_free resolution |
| `rules_choose()` fires rules in different order than old phases | Medium | Numeric parity | Add assertion: existing BDD allocations must match to within 0.01 kW |
| `asset_forecasts` removal breaks `loops.rs` compilation | Low | Build error | Fix call site in same commit as signature change |
| FirmObligation scenario requires Phase B before CP3 can run | Known | CP3 partial | Mark scenario `@wip` until Phase B is merged |

---

## Files changed

| File | CP | Change |
|---|---|---|
| `entities/plan.rs` | CP1 | Add `PlanReason`, `PlanStep`, `LookaheadContext`; add `steps` field to `Plan` |
| `controller/planner.rs` | CP1 | Add `SiteContext` struct; add `steps: vec![]` to `Plan` construction |
| `controller/planner.rs` | CP2 | New `run_planner()` signature; add `precompute_lookahead()`, `rules_choose()`, `update_slot_from_step()`; restructure main loop |
| `profile.rs` | CP2 | Add `lookahead_h: f64` to `PlannerConfig` (default 2.0) |
| `loops.rs` | CP2 | Update `run_planner()` call: remove `asset_forecasts`, pass `&sim` + `&reservation_layer` |
| `tests/features/plan_reasons.feature` | CP3 | New BDD feature file with PlanReason scenarios |
| `tests/steps/plan_reason_steps.py` | CP3 | Step implementations for reason assertions |

---

## Success criteria

- `cargo build` compiles without error after each checkpoint
- After CP2: all 123 existing BDD scenarios pass unchanged
- After CP3: all scenarios pass including 5 new `PlanReason` scenarios
- Single commit per checkpoint; combined tag: `refactor(ven): Phase D — planner loop + PlanReason`
