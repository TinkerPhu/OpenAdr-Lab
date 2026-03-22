# Phase B — Reservation Layer

## Context

Phase A delivered the `Asset` trait with `step()`, `capability()`, and per-asset
`AssetHistoryBuffer`. Phase B introduces the `ReservationLayer` struct and wires
VTN FIRM events into it. Capacity limit handling (`OadrCapacityState`) is unchanged
in this phase — that migration belongs to the Grid virtual asset, which is deferred.

**Architecture reference:** `docs/architecture/ven_planning_architecture.md` §5
**Interface spec:** `docs/architecture/ven_asset_interface_spec.md` §2
**Prerequisite:** Phase A CP1 + CP2 complete, all BDD scenarios green.
**Gate:** all existing BDD scenarios green after each checkpoint.

---

## What changes and what stays

| Element | Current state | After Phase B |
|---|---|---|
| `controller/reservation.rs` | Does not exist | New module |
| `parse_firm_reservations()` | Does not exist | New function — SIMPLE/FIRM events → `Vec<Reservation>` |
| `run_planner()` signature | Takes `capacity: &OadrCapacityState` | Gains `reservations: &ReservationLayer` alongside `capacity` |
| FIRM event planner constraint | Not enforced by planner (reactor only) | `available_cap()` called per slot per asset |
| `PlanTimeSlot.import_cap_kw` | Stamped from `OadrCapacityState` | **Unchanged** — deferred to Grid virtual asset phase |
| `PlanTimeSlot.export_cap_kw` | Same | **Unchanged** — deferred |
| `OadrCapacityState` in planner | Passed as parameter | **Kept** alongside `ReservationLayer` |

Nothing in the physics models, route handlers, or capacity limit path changes.
Capacity limits (`IMPORT_CAPACITY_LIMIT`, `EXPORT_CAPACITY_LIMIT`) are **not**
modelled as `Reservation` records — see `ven_asset_interface_spec.md` §2
"Capacity limits — design note".

---

## Checkpoint 1 — Define `reservation.rs`

### New file: `VEN/src/controller/reservation.rs`

Match the spec exactly (`ven_asset_interface_spec.md` §2). No invention.

```rust
use chrono::{DateTime, Utc};
use uuid::Uuid;
use crate::assets::AssetCapability;

/// Direction of flexibility constraint.
///
/// Up   = hold headroom for consumption reduction. Reduces max_import_kw.
/// Down = hold headroom for consumption increase.  Reduces max_export_kw toward zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlexDirection {
    Up,
    Down,
}

/// Source that created a reservation.
///
/// Note: OpenADR IMPORT/EXPORT_CAPACITY_LIMIT events do NOT produce Reservation
/// records. They are expressed through the Grid virtual asset's capability.
/// Only SIMPLE-type FIRM demand response events use VtnFirmEvent here.
#[derive(Debug, Clone)]
pub enum ReservationSource {
    /// VTN SIMPLE-type FIRM event: "reduce consumption by kw kW during window."
    VtnFirmEvent   { event_id: String },
    /// FlexibilityPolicy scheduled window (Phase C).
    PolicySchedule { policy_id: String },
    /// FlexibilityPolicy default reserve (Phase C).
    PolicyDefault,
    /// User request (Phase F).
    UserRequest    { request_id: Uuid },
}

/// A single capacity reservation. Time-windowed, asset-scoped or site-level.
///
/// `kw` is always a *reduction magnitude* (≥ 0) — how much headroom is held.
/// It is NOT a capacity ceiling. Direction determines which end of the capability
/// range is shrunk.
#[derive(Debug, Clone)]
pub struct Reservation {
    pub id:        Uuid,
    pub window:    (DateTime<Utc>, DateTime<Utc>),
    /// None = site-level (distributed proportionally across all assets).
    pub asset_id:  Option<String>,
    /// Magnitude of reserved power. Always ≥ 0.
    pub kw:        f64,
    pub direction: FlexDirection,
    pub source:    ReservationSource,
    /// Lower = higher priority. 0 = hard constraint, 1 = FIRM event, 2+ = policy/user.
    pub priority:  u8,
}

/// Per-asset reservation totals at a specific instant.
#[derive(Debug, Clone, Default)]
pub struct AssetReservation {
    /// Total kW locked for upward flexibility (consumption reduction). Always ≥ 0.
    pub reserved_up_kw:   f64,
    /// Total kW locked for downward flexibility (consumption increase). Always ≥ 0.
    pub reserved_down_kw: f64,
}

pub struct ReservationLayer {
    reservations: Vec<Reservation>,
}

impl ReservationLayer {
    pub fn new() -> Self {
        Self { reservations: Vec::new() }
    }

    /// Add a reservation. Generates an id if the caller didn't set one.
    pub fn insert(&mut self, r: Reservation) {
        self.reservations.push(r);
    }

    /// Remove a reservation by id (e.g. when a VTN event is cancelled).
    pub fn remove(&mut self, id: Uuid) {
        self.reservations.retain(|r| r.id != id);
    }

    /// Sum of all reservations active at `t` for the given asset,
    /// including site-level reservations (asset_id == None).
    pub fn query_asset(&self, asset_id: &str, t: DateTime<Utc>) -> AssetReservation {
        let mut up = 0.0_f64;
        let mut down = 0.0_f64;
        for r in &self.reservations {
            let (ws, we) = r.window;
            if ws > t || t >= we { continue; }
            let applies = r.asset_id.is_none()
                || r.asset_id.as_deref() == Some(asset_id);
            if !applies { continue; }
            match r.direction {
                FlexDirection::Up   => up   += r.kw,
                FlexDirection::Down => down += r.kw,
            }
        }
        AssetReservation { reserved_up_kw: up, reserved_down_kw: down }
    }

    /// Shrinks `phys_cap` by active reservations for `asset_id` at time `t`.
    ///
    /// Up   reservation: avail.max_import_kw = phys_cap.max_import_kw − reserved_up_kw
    ///                   (floored at phys_cap.max_export_kw — cannot go below export floor)
    /// Down reservation: avail.max_export_kw = phys_cap.max_export_kw + reserved_down_kw
    ///                   (capped at 0 — export floor stays ≤ 0)
    pub fn available_cap(
        &self,
        asset_id: &str,
        phys_cap: AssetCapability,
        t: DateTime<Utc>,
    ) -> AssetCapability {
        let res = self.query_asset(asset_id, t);
        AssetCapability {
            max_import_kw: (phys_cap.max_import_kw - res.reserved_up_kw)
                               .max(phys_cap.max_export_kw),
            max_export_kw: (phys_cap.max_export_kw + res.reserved_down_kw)
                               .min(0.0),
        }
    }
}
```

### Wire into `controller/mod.rs`

Add `pub mod reservation;`. No other files change in CP1.

**CP1 gate:** `cargo check` passes.

---

## Checkpoint 2 — `openadr_interface.rs`: produce `Vec<Reservation>` from FIRM events

### New function `parse_firm_reservations()`

Add alongside existing functions. Nothing is removed in CP2.

```rust
/// Parse SIMPLE-type FIRM demand response events into `Reservation` records.
///
/// Each active interval of a SIMPLE event with a non-zero payload value
/// produces one Reservation:
///   - window   = interval [start, start + duration)
///   - kw       = payload value (reduction magnitude, kW)
///   - direction = Up (SIMPLE events demand consumption reduction)
///   - priority  = 1
///   - source    = VtnFirmEvent { event_id }
///   - asset_id  = None (site-level)
///
/// IMPORT_CAPACITY_LIMIT and EXPORT_CAPACITY_LIMIT payloads are NOT processed
/// here — they go through OadrCapacityState / GridState.
pub fn parse_firm_reservations(
    events: &[Value],
    now: DateTime<Utc>,
) -> Vec<Reservation> { … }
```

**Implementation notes:**
- Only process intervals where `payload.type == "SIMPLE"`.
- Skip intervals where `activePeriod.start > now` (not yet active) or
  `activePeriod.end <= now` (expired) — Phase C handles pre-announced future events.
- If `intervalPeriod` is absent, use `now` to `now + Duration::days(1)` as window.
- The `kw` value is the SIMPLE payload value (positive magnitude = kW reduction requested).

### Test coverage

```
test_parse_firm_reservations_active_simple_event
test_parse_firm_reservations_future_event_excluded
test_parse_firm_reservations_expired_event_excluded
test_parse_firm_reservations_capacity_limit_event_ignored
test_parse_firm_reservations_multiple_intervals
```

**CP2 gate:** `cargo test` unit tests pass. BDD suite unchanged and green.

---

## Checkpoint 3 — `planner.rs`: add `ReservationLayer`, apply FIRM constraints

### Changes to `run_planner()` signature

`OadrCapacityState` stays. `ReservationLayer` is added alongside it.

```rust
pub fn run_planner(
    tariffs:      &TariffTimeSeries,
    packets:      &[EnergyPacket],
    capacity:     &OadrCapacityState,   // unchanged — capacity limits still live here
    reservations: &ReservationLayer,    // new — FIRM event constraints
    profile:      &Profile,
    now:          DateTime<Utc>,
    trigger:      PlanTrigger,
    asset_forecasts: &HashMap<String, TimeSeries>,
) -> Plan
```

### Changes to `allocate_consumption()` and `allocate_battery()`

Pass `reservations: &ReservationLayer` into both functions. Inside each function,
when computing headroom for an asset, call `available_cap()` per slot:

```rust
// In allocate_consumption(), where import headroom is computed:
let avail = reservations.available_cap(asset_id, phys_cap, slot.start);
let import_head = (slot.import_cap_kw
    .min(avail.max_import_kw)       // ← FIRM reservation applied here
    - slot.net_import_kw).max(0.0);
```

`slot.import_cap_kw` continues to carry the OAdr capacity limit (from
`OadrCapacityState`). The FIRM reservation further narrows it via `min()`.
No field is removed from `PlanTimeSlot`.

> **Why not remove `import_cap_kw`/`export_cap_kw` from `PlanTimeSlot` now?**
> These fields carry the VTN capacity limits from `OadrCapacityState`. The correct
> long-term owner is the Grid virtual asset's capability (see spec §1.3, §10.2).
> Until that asset exists, the fields stay. Removing them before the Grid asset is
> ready would just scatter the logic again. This is a tracked deferral, not an
> oversight.

### Update callers of `run_planner()`

```
grep -r "run_planner" VEN/src/
```

For each call site: build a `ReservationLayer` from
`parse_firm_reservations(events, now)` and pass it in. `OadrCapacityState` call
unchanged.

### Regression anchor

Before changing any code in CP3, confirm the existing BDD suite passes as-is.
After the change, run the full suite. Also add one new BDD scenario:

```
Given a SIMPLE FIRM event requiring 5 kW reduction is active
When the planner runs with a 10 kW charging packet
Then the allocated power per slot does not exceed (import_cap_kw - 5) kW
```

This is new behaviour (the planner previously ignored SIMPLE events). All other
scenarios are unaffected because their profiles have no active SIMPLE events.

**CP3 gate:** all 123 existing BDD scenarios green. New FIRM constraint scenario
green. `cargo clippy -- -D warnings` clean. `cargo fmt --check` clean.

---

## Summary

| CP | Changes | Risk | Gate |
|---|---|---|---|
| 1 | New `reservation.rs` — structs + `ReservationLayer` impl matching spec | Minimal — additive | `cargo check` |
| 2 | `openadr_interface.rs` — `parse_firm_reservations()` + unit tests | Low — additive | `cargo test` unit |
| 3 | `planner.rs` — add `ReservationLayer` parameter; apply FIRM constraints in allocation | Low — OadrCapacityState untouched | All BDD green + 1 new scenario |

Total scope: ~150 lines new code, ~20 lines changed in planner.
Sets the foundation for Phase C (FlexibilityPolicy inserts reservations; planner
loop unchanged) and the Grid virtual asset (which will eventually take over
`import_cap_kw`/`export_cap_kw` and allow their removal from `PlanTimeSlot`).

---

## Out of scope for Phase B

- Capacity limit migration to Grid virtual asset — deferred until `assets/grid.rs`
  is implemented (arch doc §10.2); tracked as a prerequisite for removing
  `PlanTimeSlot.import_cap_kw` / `.export_cap_kw`
- Pre-announced future FIRM events (Phase C — reservation created at event receipt,
  not at activation)
- Per-asset reservations (Phase C/D)
- `FlexibilityPolicy` (Phase C)
- `PlanReason` audit trail (Phase D)
- `IMPORT_CAPACITY_SUBSCRIPTION` and `IMPORT_CAPACITY_RESERVATION` — outbound
  reporting obligations, not inbound planner constraints
