# Phase C — FlexibilityPolicy

## Context

Phase B delivered an explicit `ReservationLayer` in `controller/reservation.rs`.
The planner now calls `reservations.available_cap(asset_id, phys_cap, t)` per asset per
planning step alongside `OadrCapacityState` for site-level capacity limits. The
`available_cap()` method is already fully implemented (per spec §2) — Phase B completed it.

Phase C is **additive**: a new `FlexibilityPolicy` module generates `Reservation` records
from profile YAML config. The planner is unchanged — it already calls `available_cap()`,
which will now receive policy reservations in addition to VTN FIRM event reservations.

**Architecture reference:** `docs/architecture/ven_planning_architecture.md` §6
**Interface spec:** `docs/architecture/ven_asset_interface_spec.md` §5
**Prerequisite:** Phase B CP1 + CP2 + CP3 complete, all BDD scenarios green.
**Gate per CP:** stated at end of each checkpoint.
**Final gate:** New BDD scenarios for policy-constrained planning green.

---

## What changes and what stays

| Element | Current state (after Phase B) | After Phase C |
|---|---|---|
| `controller/flexibility_policy.rs` | Does not exist | New module: `FlexibilityPolicy`, `DefaultReserve`, `ScheduledWindow` |
| `Profile` (profile.rs) | No policy field | `flexibility_policy: FlexibilityPolicy` (with `#[serde(default)]`) |
| `spawn_planning()` | Builds `ReservationLayer` from SIMPLE FIRM events only | Also merges policy reservations |
| `openadr_interface.rs` `parse_firm_reservations()` | Excludes future-period intervals | Extended to include pre-announced future intervals (Layer 3) |
| BDD suite | 123 scenarios | 123 + new policy scenarios |

`ReservationLayer`, `available_cap()`, `insert()`, `query_asset()` — unchanged.
Physics models, route handlers, plan entity — unchanged.

---

## Reservation semantics (recap from Phase B / spec §2)

`Reservation.kw` is always a **reduction magnitude** (≥ 0), never an absolute ceiling.
`asset_id = None` = site-level: `query_asset()` applies the reservation to every asset
queried, which is correct — a site-level policy reserve reduces available headroom for
all controllable assets simultaneously without needing a profile parameter.

This is why `FlexibilityPolicy::generate_reservations()` does **not** take a profile
parameter: it emits site-level reservations (`asset_id: None`) and the existing
`available_cap()` distributes them to each asset automatically.

> **Note on per-asset distribution (Phase D):** The architecture §5.4 `distribute()`
> function (split a site reserve proportionally across assets by current capability) is
> Phase D scope. Site-level is the correct and spec-consistent starting point for Phase C.

---

## Checkpoint 1 — `FlexibilityPolicy` structs + YAML extension

### New file: `VEN/src/controller/flexibility_policy.rs`

Struct names and field names match spec §5 exactly. The two justified deviations from
the spec's native types are noted inline.

```rust
use chrono::{DateTime, NaiveTime, Utc, Weekday};
use serde::Deserialize;
use uuid::Uuid;

use crate::controller::reservation::{FlexDirection, Reservation, ReservationSource};

/// Proactive flexibility policy — generates Reservation records so the planner
/// always holds a configurable headroom for grid response.
///
/// Derives Default so Profile can use `#[serde(default)]`: an absent
/// `flexibility_policy:` section in YAML produces zero reserve and no windows,
/// which is behaviourally identical to Phase B.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct FlexibilityPolicy {
    /// Layer 1: always-active headroom floor. Priority 100 (lowest).
    #[serde(default)]
    pub default_reserve: DefaultReserve,
    /// Layer 2: time-windowed reserves for known DR contracts or patterns.
    #[serde(default)]
    pub scheduled_windows: Vec<ScheduledWindow>,
}

/// Always-active headroom floor. Both fields default to 0.0 (inactive).
///
/// Reservations produced are site-level (`asset_id = None`): they reduce
/// available headroom for every controllable asset via `available_cap()`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct DefaultReserve {
    /// Minimum upward flexibility headroom to keep free, kW (magnitude ≥ 0).
    #[serde(default)]
    pub up_kw: f64,
    /// Minimum downward flexibility headroom to keep free, kW (magnitude ≥ 0).
    #[serde(default)]
    pub down_kw: f64,
}

/// A time-windowed scheduled reserve — e.g., a known DR contract window.
///
/// `pre_load_minutes` shifts the reservation window backwards so the planner
/// begins protecting capacity before the window opens — required when battery
/// pre-charge is needed (charge from 10% to usable SoC takes time).
///
/// Note on YAML types (justified deviations from spec §5):
/// - `days`: stored as `Vec<String>` (["Mon", "Tue", …]) and parsed at runtime —
///   chrono::Weekday has no built-in "Mon"/"Tue" serde support without a custom
///   deserializer. Spec specifies `Vec<Weekday>`.
/// - `time_start` / `time_end`: stored as `String` ("HH:MM") and parsed at runtime
///   for the same reason. Spec specifies `NaiveTime`.
#[derive(Debug, Clone, Deserialize)]
pub struct ScheduledWindow {
    /// Unique identifier (used in ReservationSource::PolicySchedule).
    pub id: String,
    /// Days of week: "Mon" | "Tue" | "Wed" | "Thu" | "Fri" | "Sat" | "Sun".
    #[serde(default)]
    pub days: Vec<String>,
    /// Window start in "HH:MM" (24-hour UTC).
    pub time_start: String,
    /// Window end in "HH:MM" (24-hour UTC).
    pub time_end: String,
    /// Upward headroom to protect during the window, kW (magnitude ≥ 0).
    #[serde(default)]
    pub reserve_up_kw: f64,
    /// Downward headroom to protect during the window, kW (magnitude ≥ 0).
    #[serde(default)]
    pub reserve_down_kw: f64,
    /// Minutes before `time_start` to begin reserving. Spec type: u32.
    #[serde(default)]
    pub pre_load_minutes: u32,
}

impl FlexibilityPolicy {
    /// Materialise Reservation records covering [from, until].
    ///
    /// All reservations are site-level (asset_id = None). `available_cap()` in
    /// ReservationLayer applies them to each asset that is queried — no profile
    /// parameter needed. Matches spec §5 signature exactly.
    ///
    /// Called once per planning cycle in `spawn_planning()`.
    pub fn generate_reservations(
        &self,
        from:  DateTime<Utc>,
        until: DateTime<Utc>,
    ) -> Vec<Reservation> {
        let mut out = Vec::new();

        // Layer 1 — default reserve
        if self.default_reserve.up_kw > 0.0 {
            out.push(Reservation {
                id:        Uuid::new_v4(),
                window:    (from, until),
                asset_id:  None,
                kw:        self.default_reserve.up_kw,
                direction: FlexDirection::Up,
                source:    ReservationSource::PolicyDefault,
                priority:  100,
            });
        }
        if self.default_reserve.down_kw > 0.0 {
            out.push(Reservation {
                id:        Uuid::new_v4(),
                window:    (from, until),
                asset_id:  None,
                kw:        self.default_reserve.down_kw,
                direction: FlexDirection::Down,
                source:    ReservationSource::PolicyDefault,
                priority:  100,
            });
        }

        // Layer 2 — scheduled windows
        for window in &self.scheduled_windows {
            let Ok(t_start) = NaiveTime::parse_from_str(&window.time_start, "%H:%M") else {
                tracing::warn!(id = %window.id, "invalid time_start — window skipped");
                continue;
            };
            let Ok(t_end) = NaiveTime::parse_from_str(&window.time_end, "%H:%M") else {
                tracing::warn!(id = %window.id, "invalid time_end — window skipped");
                continue;
            };
            let active_days = parse_weekdays(&window.days);
            let pre_load = chrono::Duration::minutes(window.pre_load_minutes as i64);

            // Walk each calendar day in [from, until] and emit for matching weekdays.
            let mut cursor = from;
            while cursor <= until {
                let date = cursor.date_naive();
                if active_days.contains(&date.weekday()) {
                    let ws = date.and_time(t_start).and_utc() - pre_load;
                    let we = date.and_time(t_end).and_utc();
                    // Clamp to [from, until]; skip if window is fully outside.
                    if we > from && ws < until {
                        let clamped_ws = ws.max(from);
                        let clamped_we = we.min(until);
                        if window.reserve_up_kw > 0.0 {
                            out.push(Reservation {
                                id:        Uuid::new_v4(),
                                window:    (clamped_ws, clamped_we),
                                asset_id:  None,
                                kw:        window.reserve_up_kw,
                                direction: FlexDirection::Up,
                                source:    ReservationSource::PolicySchedule {
                                    policy_id: window.id.clone(),
                                },
                                priority:  50,
                            });
                        }
                        if window.reserve_down_kw > 0.0 {
                            out.push(Reservation {
                                id:        Uuid::new_v4(),
                                window:    (clamped_ws, clamped_we),
                                asset_id:  None,
                                kw:        window.reserve_down_kw,
                                direction: FlexDirection::Down,
                                source:    ReservationSource::PolicySchedule {
                                    policy_id: window.id.clone(),
                                },
                                priority:  50,
                            });
                        }
                    }
                }
                cursor += chrono::Duration::days(1);
            }
        }

        out
    }
}

/// Parse ["Mon", "Tue", …] strings into chrono Weekday values.
/// Unrecognised strings are silently ignored with a warning.
fn parse_weekdays(days: &[String]) -> Vec<Weekday> {
    days.iter()
        .filter_map(|d| match d.as_str() {
            "Mon" => Some(Weekday::Mon),
            "Tue" => Some(Weekday::Tue),
            "Wed" => Some(Weekday::Wed),
            "Thu" => Some(Weekday::Thu),
            "Fri" => Some(Weekday::Fri),
            "Sat" => Some(Weekday::Sat),
            "Sun" => Some(Weekday::Sun),
            other => {
                tracing::warn!("unknown weekday string: {other}");
                None
            }
        })
        .collect()
}
```

### Wire into `controller/mod.rs`

```rust
pub mod flexibility_policy;
```

### YAML profile extension — `profile.rs`

Add to `Profile`:

```rust
use crate::controller::flexibility_policy::FlexibilityPolicy;

// in Profile struct:
#[serde(default)]
pub flexibility_policy: FlexibilityPolicy,
```

`FlexibilityPolicy` derives `Default` so an absent `flexibility_policy:` section in
YAML is behaviourally identical to Phase B (no new reservations generated).

### Reference YAML (not added to any profile in CP1 — profiles unchanged)

```yaml
flexibility_policy:
  default_reserve:
    up_kw: 3.0
    down_kw: 0.0
  scheduled_windows:
    - id: "peak_dr_weekday"
      days: ["Mon", "Tue", "Wed", "Thu", "Fri"]
      time_start: "16:00"
      time_end:   "20:00"
      reserve_up_kw: 10.0
      reserve_down_kw: 0.0
      pre_load_minutes: 60
```

**CP1 gate:** `cargo check` passes. No profile YAML files changed. No behaviour change.

---

## Checkpoint 2 — Unit tests for `FlexibilityPolicy::generate_reservations()`

All tests live in the `#[cfg(test)]` block of `controller/flexibility_policy.rs`.
`available_cap()` is already fully tested in `reservation.rs` (Phase B); no need to
re-test it here beyond the integration assertion below.

```
test_default_policy_generates_no_reservations
    // FlexibilityPolicy::default(), any [from, until]
    // → generate_reservations() returns []

test_default_reserve_up_generates_site_level_reservation
    // default_reserve { up_kw: 3.0, down_kw: 0.0 }
    // → 1 reservation: asset_id=None, direction=Up, kw=3.0, priority=100
    // → window == (from, until) exactly

test_default_reserve_both_directions
    // default_reserve { up_kw: 3.0, down_kw: 2.0 }
    // → 2 reservations: one Up, one Down, both site-level

test_default_reserve_zero_kw_not_emitted
    // default_reserve { up_kw: 0.0, down_kw: 1.0 }
    // → 1 reservation (Down only); zero-kW Up is not emitted

test_site_level_applies_to_all_assets_via_available_cap
    // Integration: insert a site-level Up reservation of 3.0 kW into ReservationLayer.
    // query_asset("battery", t) → reserved_up_kw = 3.0
    // query_asset("ev",      t) → reserved_up_kw = 3.0
    // Both assets reduced by the same reservation — no profile lookup needed.

test_scheduled_window_matching_day_emits_up_and_down
    // Tuesday window 14:00–16:00, reserve_up_kw=5.0, reserve_down_kw=2.0
    // from = Monday 10:00, until = Wednesday 10:00
    // → 2 reservations (Up + Down) covering Tuesday 14:00–16:00

test_scheduled_window_non_matching_day_skipped
    // days=["Mon"], from/until span Tuesday only → 0 reservations

test_scheduled_window_pre_load_shifts_window_start
    // time_start="16:00", pre_load_minutes=60
    // → window.0 = 15:00 on the matching day

test_scheduled_window_reserve_down_zero_not_emitted
    // reserve_down_kw=0.0 → only Up reservation emitted

test_scheduled_window_clamped_to_horizon
    // Window would start before `from` or end after `until` → clamped correctly

test_invalid_time_start_skips_window
    // time_start="25:99" → window skipped, no panic

test_multiple_windows_multiple_days
    // Two windows on different days → both emit reservations within horizon
```

**CP2 gate:** `cargo test` (unit tests) passes. No production wiring yet — behaviour unchanged.

---

## Checkpoint 3 — Wire `FlexibilityPolicy` into `spawn_planning()`

This is the behavioural change. The `ReservationLayer` passed to `run_planner()` now
contains both SIMPLE FIRM event reservations (Phase B) and policy reservations (Phase C).

### Changes to `spawn_planning()` in `loops.rs`

The existing Phase B code builds `reservation_layer` from `parse_firm_reservations()`.
Add policy reservation injection immediately after:

```rust
// Existing Phase B code (unchanged):
let events = state.events().await;
let mut reservation_layer = controller::reservation::ReservationLayer::new();
for r in controller::openadr_interface::parse_firm_reservations(&events, now) {
    reservation_layer.insert(r);
}

// Phase C addition — policy reservations:
let planning_horizon = chrono::Duration::hours(profile.planner.plan_horizon_h as i64);
for r in profile.flexibility_policy.generate_reservations(now, now + planning_horizon) {
    reservation_layer.insert(r);
}
```

No other changes. `run_planner()` signature is unchanged.

### Regression protection

`test.yaml` has no `flexibility_policy:` section →
`FlexibilityPolicy::default()` → `generate_reservations()` returns `[]` →
existing 123 BDD scenarios are unaffected by definition.

**CP3 gate:** All 123 existing BDD scenarios green. At least one new policy scenario
green (see §BDD scenarios). `cargo clippy -- -D warnings` clean. `cargo fmt --check` clean.

---

## Checkpoint 4 — Layer 3: extend `parse_firm_reservations()` for pre-announced events

The arch doc §6 Layer 3 requires that VTN SIMPLE events with a **future** `activePeriod`
create reservations immediately on receipt, so the planner protects capacity before the
window opens. Phase B explicitly excluded future intervals with this comment:

> *"Intervals where end ≤ now (expired) or start > now (future, not yet active) are
> excluded. Phase C handles pre-announced future events."*

### Change to `parse_firm_reservations()` in `openadr_interface.rs`

Remove only the `window_start > now` exclusion. Keep the `window_end <= now` exclusion
(expired intervals remain irrelevant).

```rust
// Before (Phase B — remove this condition):
if window_end <= now || window_start > now {
    continue;
}

// After (Phase C):
if window_end <= now {
    continue;
}
// Future windows (window_start > now) are intentionally included.
// ReservationLayer::query_asset() gates activation by window: a future reservation
// is inactive at t=now but correctly active when the planner queries t=now+Nh.
```

No other changes. The SIMPLE payload value is already read directly as kW (no
level-mapping config needed — consistent with Phase B and the arch doc).

### Unit tests

```
test_future_simple_interval_now_included
    // interval window_start = now + 6h → reservation IS emitted (was excluded in Phase B)

test_expired_interval_still_excluded
    // window_end <= now → 0 reservations (unchanged)

test_currently_active_interval_still_included
    // window overlaps now → reservation emitted (unchanged)

test_future_reservation_inactive_at_now_via_query_asset
    // Insert future reservation into ReservationLayer.
    // query_asset(asset, now)       → reserved_up_kw = 0.0  (not yet active)
    // query_asset(asset, now + 7h)  → reserved_up_kw = kw   (active)
```

**CP4 gate:** Unit tests pass. All 123 BDD scenarios green.
New pre-announced event BDD scenario green (see §BDD scenarios).

---

## BDD scenarios (new)

New feature file: `tests/features/phase_c_flexibility_policy.feature`

`test.yaml` remains unchanged. New scenarios use a dedicated profile with a
`flexibility_policy:` section, loaded via `POST /sim/reset/:id` or a
`PROFILE_PATH` override in the test compose configuration.

```gherkin
Feature: FlexibilityPolicy — Layer 1 default reserve

  Scenario: Default reserve reduces available import headroom for all assets
    Given the VEN profile has default_reserve up_kw 3.0
    And the battery has max_charge_kw 5.0
    And there are no VTN events
    When the planner runs
    Then all battery charging slots have setpoint_kw at most 2.0
    # site-level 3.0 kW Up reservation → available_cap.max_import_kw = 5.0 − 3.0 = 2.0

  Scenario: Default reserve of 0.0 has no effect
    Given the VEN profile has default_reserve up_kw 0.0 and down_kw 0.0
    And the battery has max_charge_kw 5.0
    When the planner runs
    Then battery charging is not limited by policy

Feature: FlexibilityPolicy — Layer 2 scheduled windows

  Scenario: Scheduled window reserves headroom including pre-load period
    Given the VEN profile has a scheduled window "peak_dr" on today from 16:00 to 20:00
    And the window has pre_load_minutes 60 and reserve_up_kw 5.0
    And the battery has max_charge_kw 8.0
    When the planner runs at 14:00
    Then battery charging during 15:00–20:00 does not exceed 3.0 kW
    # 8.0 − 5.0 = 3.0 kW available during pre-load + window
    And battery charging before 15:00 is not constrained by the window

  Scenario: Scheduled window on non-matching day has no effect
    Given the VEN profile has a scheduled window only on Monday
    And today is Tuesday
    When the planner runs
    Then battery charging is not limited by the scheduled window

Feature: FlexibilityPolicy — Layer 3 pre-announced VTN events

  Scenario: Future SIMPLE event creates a reservation for the event window
    Given a VTN SIMPLE event with value 5.0 and intervalPeriod starting in 4 hours
    And the battery has max_charge_kw 5.0
    When the planner runs
    Then battery setpoints during the event window are at most 0.0 kW
    # site-level 5.0 kW Up reservation → available = 5.0 − 5.0 = 0.0

  Scenario: Expired SIMPLE event interval produces no reservation
    Given a VTN SIMPLE event whose intervalPeriod ended 1 hour ago
    When the planner runs
    Then no reservation exists for the expired interval
```

---

## Files changed

| File | Change |
|---|---|
| `controller/flexibility_policy.rs` | New — `FlexibilityPolicy`, `DefaultReserve`, `ScheduledWindow`, `generate_reservations()`, `parse_weekdays()` |
| `controller/mod.rs` | Add `pub mod flexibility_policy;` |
| `profile.rs` | Add `flexibility_policy: FlexibilityPolicy` field + import |
| `loops.rs` | `spawn_planning()`: inject policy reservations into `ReservationLayer` |
| `controller/openadr_interface.rs` | Remove future-interval exclusion from `parse_firm_reservations()` |
| `tests/features/phase_c_flexibility_policy.feature` | New BDD feature file |

---

## Summary

| CP | Changes | Risk | Gate |
|---|---|---|---|
| 1 | New structs + YAML field (additive, no behaviour change) | Minimal | `cargo check` |
| 2 | `generate_reservations()` unit tests | Minimal — logic not yet wired | `cargo test` unit |
| 3 | Wire into `spawn_planning()` | Low — `test.yaml` has no policy; existing BDD unaffected | 123 BDD green + new policy scenario |
| 4 | Extend `parse_firm_reservations()` for future intervals | Low — additive filter relaxation | unit tests + 123 BDD green + pre-announced scenario |

Total scope: ~200 lines new in `flexibility_policy.rs`, ~6 lines in `loops.rs`,
~5 lines changed in `openadr_interface.rs`.

No behaviour change for VENs without a `flexibility_policy:` section in their YAML.
Sets the foundation for Phase D (planner loop refactor + `PlanReason` audit trail,
where `PolicyReserve` will surface these reservations on every `PlanStep`).

---

## Out of scope for Phase C

- Per-asset reservation distribution (`distribute()` policy — arch doc §5.4, Phase D)
- `PlanReason::PolicyReserve` on every `PlanStep` (Phase D)
- `LookaheadContext` enrichment for proactive pre-loading (Phase D)
- `FlexibilityEnvelope` as first-class output (Phase E)
- `ComfortBound` reservations from asset profile (heater `temp_min_c`/`temp_max_c` — Phase D)
- Adaptive policy layer (learned patterns → auto-generated `ScheduledWindow`s — future)
