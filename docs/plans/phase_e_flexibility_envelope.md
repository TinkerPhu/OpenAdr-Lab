# Phase E — Flexibility Envelope as First-Class Output

## Context

Phase E promotes the flexibility envelope from a plan side-effect into a
live, always-queryable site-level value. `compute_envelope()` becomes a
standalone function that reads current asset state and active reservations —
no planning cycle required. `GET /flexibility` is re-homed to return this
live value. The reporter gains handlers for `IMPORT_CAPACITY_RESERVATION`
and `EXPORT_CAPACITY_RESERVATION` obligations.

**Architecture reference:** `docs/architecture/ven_planning_architecture.md` §9
**Prerequisites:**
- Phase A complete — `AssetConfig.capability(&state)` available on all assets;
  `SimState.iter_assets()` returns `(AssetEntry, AssetConfig)` pairs.
- Phase B complete — `ReservationLayer` with `available_cap()` available;
  `parse_firm_reservations()` exists in `openadr_interface.rs`.

**Touches:** `entities/plan.rs`, new `controller/envelope.rs`, `state.rs`,
`routes/hems.rs`, `controller/reporter.rs`, `loops.rs`

**Gate:** all existing BDD scenarios green after each checkpoint; new BDD
scenario passes at CP2.

---

## What changes and what stays

| Element | Current state | After Phase E |
|---|---|---|
| `FlexibilityEnvelope` (per-packet) | `entities/plan.rs`, returned by `GET /flexibility` | **Unchanged** — stays in `plan.envelopes`, accessible via `GET /plan` |
| `SiteFlexibilityEnvelope` | Does not exist | New struct in `entities/plan.rs` |
| `compute_envelope()` | Does not exist | New standalone function in `controller/envelope.rs` |
| `AppState.site_envelope` | Does not exist | New field; updated after each planner run and each dispatcher tick |
| `GET /flexibility` | Returns `plan.envelopes: Vec<FlexibilityEnvelope>` (per-packet) | Returns `SiteFlexibilityEnvelope` (live) |
| BDD tests for `GET /flexibility` | Assert per-packet array fields | Updated to assert site-level fields |
| Reporter: `IMPORT_CAPACITY_RESERVATION` | Not handled — falls through to default | Returns `envelope.up_kw` |
| Reporter: `EXPORT_CAPACITY_RESERVATION` | Not handled — falls through to default | Returns `envelope.down_kw` |

The per-packet `FlexibilityEnvelope` structs remain in `plan.envelopes` and are
returned by `GET /plan`. Any test or UI code that needs packet-level scheduling
data reads it from there. `GET /flexibility` becomes the live site-level endpoint.

---

## New type: `SiteFlexibilityEnvelope`

Add to `entities/plan.rs`.

```rust
/// Live site-level flexibility available to the grid right now (§9).
///
/// Computed directly from current asset state and active reservations —
/// independent of the active plan. Always queryable without triggering
/// a planning cycle.
///
/// up_kw:   how much the VEN can reduce grid consumption right now (kW, ≥ 0).
/// down_kw: how much the VEN can increase grid consumption right now (kW, ≥ 0).
///
/// Duration fields estimate how long the VEN can sustain the headroom based
/// on available storage energy. None if no storage assets are present.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SiteFlexibilityEnvelope {
    pub ts:              DateTime<Utc>,
    /// Consumption-reduction headroom available right now (kW). Always ≥ 0.
    pub up_kw:           f64,
    /// Consumption-increase headroom available right now (kW). Always ≥ 0.
    pub down_kw:         f64,
    /// Estimated duration up_kw can be sustained, in seconds. None = no storage.
    pub up_duration_s:   Option<u64>,
    /// Estimated duration down_kw can be sustained, in seconds. None = no storage.
    pub down_duration_s: Option<u64>,
}
```

Rationale for separation from per-packet `FlexibilityEnvelope`:
the per-packet struct describes scheduling opportunity windows for
`EnergyPacket` records (energy needed, budget, slot count). This struct
describes instantaneous site dispatchable headroom for grid services.
They answer different questions and must not be conflated.

---

## `compute_envelope()` — the standalone function

### New file: `VEN/src/controller/envelope.rs`

```rust
use chrono::{DateTime, Utc};

use crate::assets::AssetConfig;
use crate::controller::openadr_interface::parse_firm_reservations;
use crate::controller::reservation::ReservationLayer;
use crate::entities::plan::SiteFlexibilityEnvelope;
use crate::simulator::SimState;
use serde_json::Value;

/// Compute the site-level flexibility envelope from current state.
///
/// Algorithm (§9):
///   For each asset:
///     phys_cap  = config.capability(&entry.state)          // physics max/min
///     avail_cap = reservation_layer.available_cap(id, phys_cap, now)
///     up_kw    += (entry.last_power_kw − avail_cap.max_export_kw).max(0.0)
///     down_kw  += (avail_cap.max_import_kw − entry.last_power_kw).max(0.0)
///
/// Uncontrollable assets (PV, BaseLoad) have is_fixed() == true:
///   phys_cap.max_export_kw == phys_cap.max_import_kw == actual_power_kw
///   → contribute 0 to both up_kw and down_kw. Correct by design.
///
/// Duration is estimated from available storage energy:
///   up_duration_s   = available_discharge_kwh / up_kw × 3600
///   down_duration_s = available_charge_kwh    / down_kw × 3600
/// Both are None if no storage assets are present or the corresponding kw is 0.
pub fn compute_envelope(
    sim: &SimState,
    reservation_layer: &ReservationLayer,
    now: DateTime<Utc>,
) -> SiteFlexibilityEnvelope {
    let mut up_kw = 0.0_f64;
    let mut down_kw = 0.0_f64;

    // Storage energy accumulators for duration estimation
    let mut available_discharge_kwh = 0.0_f64;
    let mut available_charge_kwh = 0.0_f64;

    for (entry, config) in sim.iter_assets() {
        let phys_cap = config.capability(&entry.state);
        let avail_cap = reservation_layer.available_cap(&entry.id, phys_cap, now);

        // up: how much this asset can reduce its consumption from current level
        up_kw   += (entry.last_power_kw - avail_cap.max_export_kw).max(0.0);
        // down: how much this asset can increase its consumption from current level
        down_kw += (avail_cap.max_import_kw - entry.last_power_kw).max(0.0);

        // Duration estimate from storage assets
        match config {
            AssetConfig::Battery(b) => {
                let soc = match &entry.state {
                    crate::assets::AssetState::Battery(s) => s.soc_pct,
                    _ => continue,
                };
                available_discharge_kwh += (soc - b.min_soc).max(0.0) * b.capacity_kwh;
                available_charge_kwh    += (1.0_f64 - soc).max(0.0) * b.capacity_kwh;
            }
            AssetConfig::Ev(e) => {
                let (soc, plugged) = match &entry.state {
                    crate::assets::AssetState::Ev(s) => (s.soc_pct, s.plugged),
                    _ => continue,
                };
                if plugged {
                    available_discharge_kwh += (soc - e.min_soc).max(0.0) * e.battery_kwh;
                    available_charge_kwh    += (1.0_f64 - soc).max(0.0) * e.battery_kwh;
                }
            }
            _ => {}
        }
    }

    let up_duration_s = if up_kw > 1e-6 && available_discharge_kwh > 1e-6 {
        Some((available_discharge_kwh / up_kw * 3600.0) as u64)
    } else {
        None
    };

    let down_duration_s = if down_kw > 1e-6 && available_charge_kwh > 1e-6 {
        Some((available_charge_kwh / down_kw * 3600.0) as u64)
    } else {
        None
    };

    SiteFlexibilityEnvelope {
        ts: now,
        up_kw,
        down_kw,
        up_duration_s,
        down_duration_s,
    }
}

/// Build a fresh `ReservationLayer` from the current event list
/// (SIMPLE FIRM events only) and compute the site envelope.
///
/// This is the entry point for `GET /flexibility` and the dispatcher tick.
/// It does NOT modify any state — it is a pure read + compute.
pub fn compute_envelope_from_events(
    sim: &SimState,
    events: &[Value],
    now: DateTime<Utc>,
) -> SiteFlexibilityEnvelope {
    let reservations = parse_firm_reservations(events, now);
    let mut layer = ReservationLayer::new();
    for r in reservations {
        layer.insert(r);
    }
    compute_envelope(sim, &layer, now)
}
```

### Wire into `controller/mod.rs`

```rust
pub mod envelope;
```

### Unit tests (same file, `mod tests`)

```
test_compute_envelope_no_assets_returns_zero
test_compute_envelope_ev_charging_contributes_up
test_compute_envelope_battery_idle_contributes_both
test_compute_envelope_reservation_reduces_down
test_compute_envelope_pv_contributes_nothing
test_compute_envelope_duration_from_battery_soc
```

**CP1 gate:** `cargo test` unit tests in `envelope.rs` pass.
`cargo check` clean. No BDD run needed.

---

## Checkpoint 2 — `AppState` + `GET /flexibility` + BDD update

### `AppState`: add `site_envelope` field

In `state.rs`, `InnerState`:

```rust
#[serde(skip)]
pub site_envelope: Option<SiteFlexibilityEnvelope>,
```

Add accessor methods:

```rust
pub async fn site_envelope(&self) -> Option<SiteFlexibilityEnvelope> {
    self.inner.read().await.site_envelope.clone()
}

pub async fn set_site_envelope(&self, env: SiteFlexibilityEnvelope) {
    self.inner.write().await.site_envelope = Some(env);
}
```

Initialize to `None` in `AppState::new()` and in `InnerState`'s snapshot path.

### Update `loops.rs`: refresh envelope after each planner run

In the planning loop (`spawn_plan_loop`), after `state.set_active_plan(...)`:

```rust
// Refresh site envelope immediately after each plan cycle.
// Uses the same events and sim state that just fed the planner.
let env = controller::envelope::compute_envelope_from_events(
    &*sim_guard,
    &events,
    now,
);
state.set_site_envelope(env).await;
```

Also refresh in the dispatcher sim tick (1-second loop), after each tick update:

```rust
// Cheap: capability() + reservation query, no allocation. ~1 µs.
let env = controller::envelope::compute_envelope_from_events(
    &*sim_guard,
    &state.events().await,
    Utc::now(),
);
state.set_site_envelope(env).await;
```

This keeps `site_envelope` current between planner cycles — important for
operator dashboards and reporter obligations that fire on independent timers.

### Update `GET /flexibility` route handler

In `routes/hems.rs`, replace the existing handler:

```rust
/// GET /flexibility — returns the live site-level flexibility envelope (Phase E).
///
/// Returns the most recently computed SiteFlexibilityEnvelope.
/// Updated every dispatcher tick (~1s) and after every planner cycle.
/// Returns 204 No Content if the envelope has not been computed yet
/// (e.g., VEN just started and no dispatcher tick has run).
pub async fn get_flexibility(State(ctx): State<AppCtx>) -> impl IntoResponse {
    match ctx.state.site_envelope().await {
        Some(env) => Json(env).into_response(),
        None => StatusCode::NO_CONTENT.into_response(),
    }
}
```

The per-packet `FlexibilityEnvelope` array remains accessible via `GET /plan`
in the `envelopes` field. No route change needed there.

### Update existing BDD tests for `GET /flexibility`

Locate all step definitions and scenarios that call `GET /flexibility` and
assert per-packet fields (e.g., `packet_id`, `energy_needed_kwh`,
`power_min_kw`). Those assertions are now against the wrong endpoint.

Migration:
1. Find each scenario: `grep -r "GET /flexibility" tests/features/`
2. Redirect per-packet assertions to `GET /plan` → `response["envelopes"]`
3. Add new assertion for `GET /flexibility` → `up_kw`, `down_kw` shape

New BDD scenario (add to `tests/features/hems_controller.feature` or
`tests/features/plan_reasons.feature`):

```gherkin
@phase-e
Scenario: GET /flexibility reflects available headroom, not physical max

  Given the VEN is running with a battery at 50% SoC
  And a VTN FIRM SIMPLE event reserving 3 kW from the site
  When I GET /flexibility
  Then the response contains field "up_kw"
  And the response contains field "down_kw"
  And response field "up_kw" is greater than 0.0
  And the response field "up_kw" is less than the battery's physical max discharge (unreserved headroom only)
```

Core assertion: `up_kw` must reflect the reservation subtraction, not raw
physical capability. This is the Phase E gate criterion from the architecture doc.

**CP2 gate:** all existing BDD scenarios green (old `/flexibility` tests
migrated to `/plan`). New `@phase-e` scenario green.

---

## Checkpoint 3 — Reporter: `IMPORT_CAPACITY_RESERVATION` and `EXPORT_CAPACITY_RESERVATION`

### What these obligation types mean

| Obligation type | Value source | Units |
|---|---|---|
| `IMPORT_CAPACITY_RESERVATION` | `envelope.up_kw` (consumption-reduction headroom) | W |
| `EXPORT_CAPACITY_RESERVATION` | `envelope.down_kw` (consumption-increase headroom) | W |

These are offered flexibility values — what the VEN can deliver on demand —
not current consumption. They are reported in Watts (same convention as `USAGE`).

### Update `build_measurement_report_for_obligation()`

The function signature gains one parameter:

```rust
pub fn build_measurement_report_for_obligation(
    obligation: &OadrReportObligation,
    sim: &SimState,
    ven_name: &str,
    site_envelope: Option<&SiteFlexibilityEnvelope>,  // new
) -> Option<Value>
```

Inside, extend the `match payload_type.as_str()` dispatch:

```rust
"IMPORT_CAPACITY_RESERVATION" => {
    let up_w = site_envelope.map(|e| e.up_kw * 1000.0).unwrap_or(0.0);
    vec![json!({
        "id": 0,
        "payloads": [
            { "type": "IMPORT_CAPACITY_RESERVATION", "values": [up_w] },
            { "type": "OPERATING_STATE", "values": ["ACTIVE"] }
        ]
    })]
}
"EXPORT_CAPACITY_RESERVATION" => {
    let down_w = site_envelope.map(|e| e.down_kw * 1000.0).unwrap_or(0.0);
    vec![json!({
        "id": 0,
        "payloads": [
            { "type": "EXPORT_CAPACITY_RESERVATION", "values": [down_w] },
            { "type": "OPERATING_STATE", "values": ["ACTIVE"] }
        ]
    })]
}
```

Note: these obligation types produce a single-interval report (point-in-time),
not a multi-interval history. The `intervals` array has exactly one entry.

### Update call sites of `build_measurement_report_for_obligation()`

```
grep -r "build_measurement_report_for_obligation" VEN/src/
```

Each call site in `loops.rs` must pass `site_envelope`:

```rust
let env = state.site_envelope().await;
let report_opt = {
    let sim_guard = sim.lock().await;
    controller::reporter::build_measurement_report_for_obligation(
        &ob,
        &*sim_guard,
        &ven_name,
        env.as_ref(),        // ← new
    )
};
```

### Unit tests

```
test_reporter_import_capacity_reservation_from_envelope
test_reporter_export_capacity_reservation_from_envelope
test_reporter_capacity_reservation_no_envelope_returns_zero
```

### New BDD scenario

```gherkin
@phase-e
Scenario: Reporter sends IMPORT_CAPACITY_RESERVATION with correct value

  Given the VEN has 5 kW of upward flexibility (battery can discharge)
  And an active event with reportDescriptor type "IMPORT_CAPACITY_RESERVATION"
  When the report obligation fires
  Then the report payload "IMPORT_CAPACITY_RESERVATION" value is approximately 5000 W
```

**CP3 gate:** all existing BDD scenarios green + reporter unit tests pass +
new reporter BDD scenario green. `cargo clippy -- -D warnings` clean.
`cargo fmt --check` clean.

---

## Risk register

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| Existing BDD tests break due to `GET /flexibility` format change | Medium | Breaking tests | Migrate per-packet assertions to `GET /plan` in same PR; add regression anchor before making the change |
| Duration estimate wrong for heater/PV profiles (no storage) | Low | Wrong None vs. value | Unit test with profile that has no storage — expect None |
| Dispatcher tick adds lock contention on `InnerState` | Low | Performance | Benchmark shows `capability()` is µs-level; `RwLock` read is sub-µs overhead |
| `site_envelope` stale when VEN starts before first planner run | Low | 204 on `GET /flexibility` | Handler returns 204 cleanly; compute on first dispatcher tick (within 1s of boot) |
| Reporter call sites missed (grep misses a path) | Low | Compile error | `cargo build` will fail at each site — easy to fix |
| Phase C `FlexibilityPolicy` reservations not yet included | Known | Envelope slightly optimistic | This is by design — Phase C will insert its reservations into the layer before `compute_envelope()` is called |

---

## Files changed

| File | CP | Change |
|---|---|---|
| `entities/plan.rs` | CP1 | Add `SiteFlexibilityEnvelope` struct |
| `controller/envelope.rs` | CP1 | New: `compute_envelope()`, `compute_envelope_from_events()`, unit tests |
| `controller/mod.rs` | CP1 | Add `pub mod envelope;` |
| `state.rs` | CP2 | Add `site_envelope: Option<SiteFlexibilityEnvelope>` to `InnerState`; add `site_envelope()` / `set_site_envelope()` accessors |
| `loops.rs` | CP2 | Call `compute_envelope_from_events()` after plan cycle + in dispatcher tick; store via `set_site_envelope()` |
| `routes/hems.rs` | CP2 | Replace `get_flexibility` body: return `site_envelope` from `AppState` |
| `tests/features/*.feature` | CP2 | Migrate per-packet `/flexibility` assertions to `/plan`; add `@phase-e` site-level scenario |
| `tests/steps/*.py` | CP2 | Update/add steps for site-level envelope assertions |
| `controller/reporter.rs` | CP3 | Add `site_envelope: Option<&SiteFlexibilityEnvelope>` parameter; handle `IMPORT/EXPORT_CAPACITY_RESERVATION` |
| `loops.rs` | CP3 | Pass `env.as_ref()` to reporter call sites |

---

## Out of scope for Phase E

- `FlexibilityPolicy` reservations feeding into the envelope — Phase C inserts
  `PolicyDefault` / `PolicySchedule` reservations; `compute_envelope()` will
  automatically include them once the layer is populated by Phase C.
- `UserRequest` leeway feeding into the envelope — Phase F adds
  `tolerance_min`, `budget_eur`, and interruptible packet contributions.
- Duration accuracy improvements (e.g., adjusting for ongoing loads during
  discharge) — the current estimate is first-order and sufficient for reporting.
- `PlanWarning::LowFlexibility` emission — the planner can compare its
  computed envelopes against a threshold once Phase D's loop is in place.

---

## Success criteria

- `cargo build` compiles without error after each checkpoint
- After CP1: `cargo test` unit tests in `envelope.rs` pass
- After CP2: all existing BDD scenarios pass; new `@phase-e` scenario passes;
  `GET /flexibility` returns `{ ts, up_kw, down_kw, up_duration_s, down_duration_s }`
- After CP3: reporter sends correct `IMPORT/EXPORT_CAPACITY_RESERVATION`
  payloads; reporter unit tests pass; no existing scenario regresses
- Tag: `feat(ven): Phase E — live flexibility envelope`
