# Phase C — FlexibilityPolicy

## Context

Phase B delivered an explicit `ReservationLayer` in `controller/reservation.rs`.
The planner now calls `reservations.import_limit_kw(t)` and `reservations.available_cap(asset_id, phys_cap, t)`
instead of reading scalar fields from `OadrCapacityState`. The `available_cap()` method is
currently a pass-through stub with a `// Phase C will subtract asset-level reservations here` comment.

Phase C is **additive**: a new `FlexibilityPolicy` module generates `Reservation` records from
profile YAML config and from pre-announced VTN events. The planner is unchanged — it already
calls `available_cap()`, which Phase C implements properly.

**Architecture reference:** `docs/architecture/ven_planning_architecture.md` §6
**Prerequisite:** Phase B CP1 + CP2 + CP3 complete, all BDD scenarios green.
**Gate per CP:** stated at the end of each checkpoint.
**Final gate:** New BDD scenarios for policy-constrained planning green.

---

## What changes and what stays

| Element | Current state (after Phase B) | After Phase C |
|---|---|---|
| `ReservationLayer::available_cap()` | Pass-through stub | Applies per-asset Up/Down reservations |
| `controller/flexibility_policy.rs` | Does not exist | New module: Layer 1 + Layer 2 |
| `Profile` (profile.rs) | No policy field | `flexibility_policy: FlexibilityPolicyConfig` |
| `spawn_planning()` | Builds `ReservationLayer` from capacity events only | Also merges policy reservations |
| `openadr_interface.rs` | No SIMPLE-event → reservation mapping | New `parse_firm_event_reservations()` |
| BDD suite | 123 scenarios | 123 + new policy scenarios |

Nothing in the physics models, route handlers, or plan entity changes.

---

## Reservation semantics for per-asset reservations (Phase C)

Phase B site-level reservations (`asset_id = None`) use `kw` as an **absolute ceiling**:
- `import_limit_kw(t)` = `min(kw)` across all site-level Up reservations.

Phase C per-asset reservations (`asset_id = Some(...)`) use `kw` as **headroom to preserve**:
- `available_cap()` reduces `phys_cap.max_import_kw` by `kw` for each active per-asset Up reservation.

These two interpretations never overlap (`import_limit_kw()` filters `asset_id.is_none()`;
`available_cap()` filters `asset_id == Some(target_id)`). Document both in `reservation.rs` comments.

### Distribution policy (scope note)

Architecture §5.4 describes a `distribute()` function that splits a site-level headroom
requirement across assets proportionally to their current capability. This is **Phase D** scope.

For Phase C: `default_reserve.up_kw` and `scheduled_window.reserve_up_kw` are applied to
**each controllable asset independently** — i.e., every controllable asset gets the full
`kw` reservation. This is conservative (may over-reserve for multi-asset sites) but correct
for single-asset setups and safe as a first iteration. A note in the YAML documents this.

Controllable assets (get reservations): Battery, Ev, Heater.
Non-controllable (excluded): Pv, BaseLoad — their `available_cap()` is always pass-through
since `capability()` already returns a point-range for these.

---

## Checkpoint 1 — `FlexibilityPolicyConfig` structs + YAML extension

### New structs in `controller/flexibility_policy.rs`

```rust
use serde::Deserialize;

/// YAML-loaded configuration for the VEN's proactive flexibility policy.
/// All fields are optional — a VEN with no `flexibility_policy:` section
/// in its profile behaves identically to Phase B (no policy reservations).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct FlexibilityPolicyConfig {
    /// Layer 1: always-active headroom reserve. Lowest priority (100).
    #[serde(default)]
    pub default_reserve: Option<DefaultReserve>,
    /// Layer 2: time-windowed reserves for known DR contracts or patterns.
    #[serde(default)]
    pub scheduled_windows: Vec<ScheduledWindow>,
}

/// Always-active headroom floor. Ensures the planner never consumes all
/// available capability.
///
/// `up_kw` and `down_kw` are applied to **each controllable asset** as an
/// independent per-asset headroom reservation. Site-level proportional
/// distribution is Phase D (architecture §5.4).
#[derive(Debug, Clone, Deserialize)]
pub struct DefaultReserve {
    /// Minimum upward flexibility to keep free (import reduction headroom), kW.
    #[serde(default)]
    pub up_kw: f64,
    /// Minimum downward flexibility to keep free (export reduction headroom), kW.
    #[serde(default)]
    pub down_kw: f64,
}

/// A time-windowed scheduled reserve — e.g., a known DR contract window.
///
/// The planner protects `reserve_up_kw` of import headroom during this
/// window on each controllable asset (same distribution note as DefaultReserve).
///
/// `pre_load_minutes` extends the reservation window *backwards* so the
/// planner begins protecting capacity before the window starts — critical
/// when battery pre-charge is needed (e.g., the battery cannot charge from
/// 10% to usable in zero seconds).
#[derive(Debug, Clone, Deserialize)]
pub struct ScheduledWindow {
    /// Unique identifier for this window (used in ReservationSource).
    pub id: String,
    /// Days of the week this window applies. Accepted strings:
    /// "Mon" | "Tue" | "Wed" | "Thu" | "Fri" | "Sat" | "Sun"
    #[serde(default)]
    pub days: Vec<String>,
    /// Window start time in "HH:MM" (24-hour, local VEN time = UTC for now).
    pub time_start: String,
    /// Window end time in "HH:MM".
    pub time_end: String,
    /// Import headroom to protect during this window, kW.
    #[serde(default)]
    pub reserve_up_kw: f64,
    /// How many minutes before `time_start` to begin reserving capacity.
    /// The planner starts protecting capacity at `time_start − pre_load_minutes`.
    #[serde(default)]
    pub pre_load_minutes: u64,
}
```

Wire into `controller/mod.rs`:

```rust
pub mod flexibility_policy;
```

### YAML profile extension — `profile.rs`

Add to `Profile`:

```rust
#[serde(default)]
pub flexibility_policy: FlexibilityPolicyConfig,
```

Import:

```rust
use crate::controller::flexibility_policy::FlexibilityPolicyConfig;
```

### Example YAML (not written to any profile in CP1 — profiles remain unchanged)

```yaml
flexibility_policy:
  default_reserve:
    up_kw: 3.0    # keep 3 kW of import headroom per controllable asset, always
    down_kw: 0.0
  scheduled_windows:
    - id: "peak_dr_weekday"
      days: ["Mon", "Tue", "Wed", "Thu", "Fri"]
      time_start: "16:00"
      time_end:   "20:00"
      reserve_up_kw: 5.0
      pre_load_minutes: 60
```

**CP1 gate:** `cargo check` passes. No profile YAML files are changed. No behavior change.

---

## Checkpoint 2 — Extend `ReservationLayer::available_cap()`

This is the only change to `controller/reservation.rs` in Phase C. The stub is replaced with
real logic. Site-level reservation paths (`import_limit_kw()`, `export_limit_kw()`) are unchanged.

### Updated `available_cap()` in `reservation.rs`

```rust
/// Effective available capability for `asset_id` after applying per-asset
/// headroom reservations active at time `t`.
///
/// Per-asset reservations (`asset_id = Some(...)`) use `kw` as **headroom
/// to preserve** — distinct from site-level reservations which use `kw` as
/// an absolute ceiling. See module-level doc comment.
///
/// Conflict resolution for per-asset reservations:
/// - Multiple Up reservations on the same asset: sum their `kw` values
///   (each reservation independently carves out headroom).
/// - The result is clamped so max_import_kw never drops below max_export_kw.
pub fn available_cap(
    &self,
    asset_id: &str,
    phys_cap: AssetCapability,
    t: DateTime<Utc>,
) -> AssetCapability {
    let active = self.query(t);

    let reserved_up_kw: f64 = active
        .iter()
        .filter(|r| {
            r.asset_id.as_deref() == Some(asset_id)
                && r.direction == FlexDirection::Up
        })
        .map(|r| r.kw)
        .sum();

    let reserved_down_kw: f64 = active
        .iter()
        .filter(|r| {
            r.asset_id.as_deref() == Some(asset_id)
                && r.direction == FlexDirection::Down
        })
        .map(|r| r.kw)
        .sum();

    AssetCapability {
        // Reduce max import by reserved up headroom; floor at max_export_kw.
        max_import_kw: (phys_cap.max_import_kw - reserved_up_kw)
            .max(phys_cap.max_export_kw),
        // Reduce max export by reserved down headroom (make less negative); ceiling at max_import_kw.
        max_export_kw: (phys_cap.max_export_kw + reserved_down_kw)
            .min(phys_cap.max_import_kw),
    }
}
```

### Unit tests for `available_cap()` (in `reservation.rs` test module)

```
test_available_cap_no_reservations_returns_physical
    // No reservations → identical to phys_cap.

test_available_cap_up_reservation_reduces_max_import
    // 3.0 kW Up reservation on "battery" with phys_cap {max_import_kw: 5.0, max_export_kw: -5.0}
    // → available {max_import_kw: 2.0, max_export_kw: -5.0}

test_available_cap_down_reservation_reduces_max_export
    // 2.0 kW Down reservation on "battery"
    // → max_export_kw = -5.0 + 2.0 = -3.0

test_available_cap_multiple_up_reservations_are_summed
    // Two Up reservations of 2.0 and 3.0 kW → reserved_up = 5.0 kW

test_available_cap_reservation_clamps_to_zero
    // 10.0 kW Up reservation on asset with max_import_kw = 3.0 → max_import_kw = max_export_kw

test_available_cap_site_level_reservation_does_not_apply
    // Reservation with asset_id = None → not applied to "battery" (site-level path only)

test_available_cap_different_asset_does_not_apply
    // Reservation for "ev" → not applied when querying "battery"

test_available_cap_expired_reservation_does_not_apply
    // Reservation with window_end before t → not active, not applied
```

**CP2 gate:** `cargo test` (unit tests) passes. No planner change yet — `available_cap()` is called by
the planner in Phase B but currently has no per-asset reservations to apply, so BDD behavior is unchanged.

---

## Checkpoint 3 — `flexibility_policy.rs`: generate reservations

Add a `FlexibilityPolicy` struct that wraps `FlexibilityPolicyConfig` and exposes one method:
`reservations(profile, now, horizon) -> Vec<Reservation>`.

### `FlexibilityPolicy` struct

```rust
use chrono::{DateTime, Datelike, Duration, NaiveTime, Utc, Weekday};
use crate::controller::reservation::{FlexDirection, Reservation, ReservationSource};
use crate::profile::{AssetProfile, Profile};

pub struct FlexibilityPolicy {
    pub config: FlexibilityPolicyConfig,
}

impl FlexibilityPolicy {
    pub fn new(config: FlexibilityPolicyConfig) -> Self {
        Self { config }
    }

    /// Generate all policy reservations for the planning horizon [now, now+horizon].
    ///
    /// Returns an empty vec if the profile has no `flexibility_policy` section.
    /// The caller merges these into the `ReservationLayer` alongside VTN reservations.
    pub fn reservations(
        &self,
        profile:  &Profile,
        now:      DateTime<Utc>,
        horizon:  Duration,
    ) -> Vec<Reservation> {
        let mut out = Vec::new();
        let controllable_ids = controllable_asset_ids(profile);

        // Layer 1: default reserve
        if let Some(ref reserve) = self.config.default_reserve {
            for id in &controllable_ids {
                if reserve.up_kw > 0.0 {
                    out.push(Reservation {
                        window_start: now,
                        window_end:   now + horizon,
                        asset_id:     Some(id.clone()),
                        kw:           reserve.up_kw,
                        direction:    FlexDirection::Up,
                        source:       ReservationSource::PolicyDefault,
                        priority:     100, // lowest — VTN and scheduled windows always win
                    });
                }
                if reserve.down_kw > 0.0 {
                    out.push(Reservation {
                        window_start: now,
                        window_end:   now + horizon,
                        asset_id:     Some(id.clone()),
                        kw:           reserve.down_kw,
                        direction:    FlexDirection::Down,
                        source:       ReservationSource::PolicyDefault,
                        priority:     100,
                    });
                }
            }
        }

        // Layer 2: scheduled windows
        for window in &self.config.scheduled_windows {
            let Ok(t_start) = NaiveTime::parse_from_str(&window.time_start, "%H:%M") else {
                tracing::warn!(id = %window.id, "invalid time_start, skipping window");
                continue;
            };
            let Ok(t_end) = NaiveTime::parse_from_str(&window.time_end, "%H:%M") else {
                tracing::warn!(id = %window.id, "invalid time_end, skipping window");
                continue;
            };
            let pre_load = Duration::minutes(window.pre_load_minutes as i64);
            let active_weekdays = parse_weekdays(&window.days);

            // Walk each day in [now, now+horizon] and emit a reservation for matching days.
            let mut cursor = now;
            while cursor <= now + horizon {
                let date = cursor.date_naive();
                if active_weekdays.contains(&date.weekday()) {
                    let ws = date.and_time(t_start).and_utc() - pre_load;
                    let we = date.and_time(t_end).and_utc();
                    if we > now && ws < now + horizon {
                        for id in &controllable_ids {
                            if window.reserve_up_kw > 0.0 {
                                out.push(Reservation {
                                    window_start: ws.max(now),
                                    window_end:   we.min(now + horizon),
                                    asset_id:     Some(id.clone()),
                                    kw:           window.reserve_up_kw,
                                    direction:    FlexDirection::Up,
                                    source:       ReservationSource::PolicySchedule {
                                        policy_id: window.id.clone(),
                                    },
                                    priority: 50,
                                });
                            }
                        }
                    }
                }
                cursor += Duration::days(1);
            }
        }

        out
    }
}

/// Asset IDs for which the policy should generate per-asset reservations.
/// Excludes Pv and BaseLoad (fixed output — no controllable headroom).
fn controllable_asset_ids(profile: &Profile) -> Vec<String> {
    profile
        .assets
        .iter()
        .filter(|a| !matches!(a, AssetProfile::Pv(_) | AssetProfile::BaseLoad(_)))
        .map(|a| a.id().to_string())
        .collect()
}

/// Parse ["Mon", "Tue", ...] strings into chrono Weekday values.
/// Unknown strings are silently ignored.
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
            _ => None,
        })
        .collect()
}
```

### Unit tests for `FlexibilityPolicy::reservations()`

```
test_no_config_returns_empty
    // FlexibilityPolicyConfig::default() with battery + ev profile
    // → reservations() == []

test_default_reserve_up_generates_per_controllable_asset
    // default_reserve { up_kw: 3.0, down_kw: 0.0 }, profile with battery + ev + pv
    // → 2 Up reservations (battery, ev), 0 for pv
    // → each has priority=100, source=PolicyDefault, window=[now, now+horizon]

test_default_reserve_both_directions
    // default_reserve { up_kw: 3.0, down_kw: 2.0 }, profile with battery
    // → 2 reservations (one Up, one Down) for battery

test_scheduled_window_matching_day_emits_reservation
    // Tuesday, time_start="14:00", time_end="16:00", days=["Tue"], reserve_up_kw=5.0
    // now = Monday 10:00 UTC, horizon = 48 h
    // → 1 reservation for the Tuesday window

test_scheduled_window_non_matching_day_emits_nothing
    // days=["Mon"], now = Tuesday → no reservations in 24h horizon

test_scheduled_window_pre_load_shifts_window_start
    // time_start="16:00", pre_load_minutes=60
    // → window_start = 15:00 on the matching day

test_scheduled_window_applies_to_controllable_assets_only
    // profile: battery + ev + pv, window.reserve_up_kw=5.0
    // → reservations only for battery and ev

test_available_cap_integration_layer1_layer2
    // Construct a ReservationLayer with both a default reserve and a scheduled window.
    // At a time inside the window: available_cap = phys_cap - max(default_reserve, window reserve)
    // Note: both reservations are summed (not max'd) — document this behavior.
```

**CP3 gate:** `cargo test` (unit tests) passes. No wire-up yet — `FlexibilityPolicy` is not called
from any production path. BDD behavior unchanged.

---

## Checkpoint 4 — Wire `FlexibilityPolicy` into `spawn_planning()`

This is the behavioral change. The planner now receives a `ReservationLayer` that includes
both VTN capacity reservations (Phase B) and policy reservations (Phase C Layer 1 + 2).

### Changes to `spawn_planning()` in `loops.rs`

Add at the top of the planning loop body (after reading state, before `run_planner()`):

```rust
// Build ReservationLayer: VTN capacity reservations (Phase B) + policy reservations (Phase C).
let events = state.events().await;
let mut reservations = controller::reservation::ReservationLayer::new();

// Phase B: VTN IMPORT/EXPORT_CAPACITY_LIMIT events → site-level reservations
for r in controller::openadr_interface::parse_capacity_reservations(&events, now) {
    reservations.add(r);
}

// Phase C: FlexibilityPolicy → per-asset headroom reservations
let policy = controller::flexibility_policy::FlexibilityPolicy::new(
    profile.flexibility_policy.clone(),
);
let planning_horizon = chrono::Duration::hours(profile.planner.plan_horizon_h as i64);
for r in policy.reservations(&profile, now, planning_horizon) {
    reservations.add(r);
}
```

Pass `&reservations` to `run_planner()` instead of `&capacity`.

> **Note:** `state.events()` is already called in the existing planning loop body — deduplicate
> this read rather than calling it twice. Check the actual order in the current `spawn_planning()`
> implementation and consolidate.

### Verify no regression

Before wiring, add one BDD assertion scenario (or cargo integration test):

```
Scenario: Default reserve reduces planner's available import capacity
  Given the VEN profile has default_reserve up_kw 3.0
  And the battery has max_charge_kw 5.0
  When the planner runs with no VTN events
  Then no battery charging slot exceeds 2.0 kW
```

This scenario should FAIL before CP4 (no policy, full 5 kW is used) and PASS after CP4.

### Update test profiles

Add `flexibility_policy` section to `VEN/profiles/test.yaml` with conservative defaults
(or none, to preserve existing BDD behavior — see below).

**Critical:** The existing 123 BDD scenarios were written without policy reserves. Adding
`default_reserve` to `test.yaml` would cause every planner assertion to fail (reserved
headroom changes all setpoints).

**Resolution:** Leave `test.yaml` unchanged (no `flexibility_policy` section). The existing
scenarios continue to pass because `FlexibilityPolicyConfig::default()` has `None` for
`default_reserve` and empty `scheduled_windows`.

New BDD scenarios for policy behavior use a dedicated test fixture (see §BDD scenarios).

**CP4 gate:** All 123 existing BDD scenarios green. At least one new policy scenario passes.
`cargo clippy -- -D warnings` clean. `cargo fmt --check` clean.

---

## Checkpoint 5 — Pre-announced VTN events (Layer 3)

Layer 3 handles VTN events with a future `activePeriod.start`. The planner should begin
protecting capacity as soon as the event is received, not when it becomes active.

This is distinct from Phase B's `parse_capacity_reservations()` which already handles
IMPORT_CAPACITY_LIMIT events with future `intervalPeriod` dates. Layer 3 extends this to
**SIMPLE events** (demand-response level signals), which don't carry a kW value directly.

### Design: SIMPLE event → kW mapping

OpenADR SIMPLE events use integer levels (0, 1, 2, 3). The VEN must map these to kW.
Add a configurable mapping to `FlexibilityPolicyConfig`:

```rust
/// kW response per SIMPLE signal level (index = level, value = import reduction kW).
/// Level 0 = no action. Defaults: [0.0, 2.0, 5.0, 10.0].
#[serde(default = "default_simple_level_kw")]
pub simple_event_response_kw: Vec<f64>,
```

```rust
fn default_simple_level_kw() -> Vec<f64> {
    vec![0.0, 2.0, 5.0, 10.0]
}
```

YAML:
```yaml
flexibility_policy:
  simple_event_response_kw: [0.0, 2.0, 5.0, 10.0]
```

### New function: `parse_firm_event_reservations()` in `openadr_interface.rs`

```rust
/// Parse SIMPLE events with a future activePeriod into Reservation records.
///
/// Creates one per-controllable-asset Up reservation per SIMPLE event
/// that has activePeriod.start in the future. The kW value is derived
/// from the SIMPLE level and the VEN's `simple_event_response_kw` mapping.
///
/// Events with activePeriod.start <= now are already active — they are
/// handled by the reactor, not by pre-announced reservations.
///
/// Note: IMPORT_CAPACITY_LIMIT events with future intervalPeriod are already
/// handled by parse_capacity_reservations() (Phase B). This function is
/// specifically for SIMPLE-level events.
pub fn parse_firm_event_reservations(
    events: &[Value],
    profile: &Profile,
    now: DateTime<Utc>,
) -> Vec<Reservation> { … }
```

**Implementation notes:**
- For each event: check `event.activePeriod.start` (ISO 8601 string) > now.
- Extract first SIMPLE payload value from first interval → `level: usize`.
- Look up `profile.flexibility_policy.simple_event_response_kw[level]` → `kw_per_asset`.
- If `kw_per_asset == 0.0` → skip.
- Parse `activePeriod.duration` to determine `window_end`.
- Emit one `Reservation { asset_id: Some(id), kw, direction: Up, source: VtnFirmEvent, priority: 1 }`
  per controllable asset.

### Wire into `spawn_planning()` (addition to CP4)

Add alongside the other reservation builders:

```rust
// Phase C Layer 3: pre-announced SIMPLE events → per-asset reservations
for r in controller::openadr_interface::parse_firm_event_reservations(&events, &profile, now) {
    reservations.add(r);
}
```

### Unit tests for `parse_firm_event_reservations()`

```
test_future_simple_event_creates_reservation
    // Event with activePeriod.start = now + 6h, SIMPLE level 2
    // Profile with battery + ev, simple_event_response_kw = [0.0, 2.0, 5.0, 10.0]
    // → 2 reservations (battery, ev), kw = 5.0, window = [now+6h, now+8h]

test_active_simple_event_not_repeated_here
    // Event with activePeriod.start = now - 1h (already active)
    // → 0 reservations (not pre-announced)

test_simple_level_0_skipped
    // SIMPLE level 0 → kw = 0.0 → no reservation

test_non_simple_event_ignored
    // Event with PRICE payload → 0 reservations

test_no_simple_events_returns_empty
    // Empty events list → []
```

**CP5 gate:** `cargo test` unit tests pass. All 123 existing BDD scenarios green.

---

## BDD scenarios (new)

New feature file: `tests/features/phase_c_flexibility_policy.feature`

These scenarios require a test VEN profile with `flexibility_policy` configured.
Either use a dedicated test endpoint (`POST /sim/reset/:id` with a policy-enabled profile)
or add a `test_policy.yaml` profile with `PROFILE_PATH` override in the test compose.

```gherkin
Feature: FlexibilityPolicy — Layer 1 default reserve

  Background:
    Given the VEN profile has default_reserve up_kw 3.0
    And the battery has max_charge_kw 5.0 and initial_soc 0.10
    And there are no VTN events

  Scenario: Default reserve caps battery charging below physical max
    When the planner runs
    Then all battery charging slots have setpoint_kw <= 2.0

  Scenario: Default reserve does not apply to PV or BaseLoad
    When the planner runs
    Then pv and base_load are not affected by policy reservations

Feature: FlexibilityPolicy — Layer 2 scheduled windows

  Scenario: Scheduled DR window reserves capacity before window start
    Given the VEN profile has a scheduled window "peak_dr" on today from 16:00 to 20:00
    And the window has pre_load_minutes 60 and reserve_up_kw 5.0
    And the battery has max_charge_kw 8.0
    When the planner runs at 15:30
    Then battery charging during 15:00–20:00 does not exceed 3.0 kW
    And battery charging before 15:00 is not constrained by the window

  Scenario: Scheduled window on non-matching day has no effect
    Given the VEN profile has a scheduled window only on Monday
    And today is Tuesday
    When the planner runs
    Then battery charging is not limited by the scheduled window

Feature: FlexibilityPolicy — Layer 3 pre-announced VTN events

  Scenario: Future SIMPLE event creates pre-announced reservation
    Given a VTN SIMPLE level 2 event with activePeriod starting in 4 hours
    And the VEN profile maps SIMPLE level 2 to 5.0 kW
    When the planner runs
    Then battery setpoints during the event window are at most 0.0 kW
    # (battery max_charge = 5.0 kW, reserved 5.0 kW → available = 0.0)

  Scenario: Already-active SIMPLE event is not double-counted
    Given a VTN SIMPLE level 2 event that started 1 hour ago
    When the planner runs
    Then the pre-announced reservation path adds 0 reservations for this event
```

---

## Files changed

| File | Change |
|---|---|
| `controller/flexibility_policy.rs` | New — `FlexibilityPolicyConfig`, `FlexibilityPolicy::reservations()` |
| `controller/mod.rs` | Add `pub mod flexibility_policy;` |
| `controller/reservation.rs` | Replace `available_cap()` stub with real per-asset logic; add unit tests |
| `profile.rs` | Add `flexibility_policy: FlexibilityPolicyConfig` field to `Profile` |
| `controller/openadr_interface.rs` | Add `parse_firm_event_reservations()` + unit tests |
| `loops.rs` | `spawn_planning()`: build + merge policy reservations into `ReservationLayer` |
| `tests/features/phase_c_flexibility_policy.feature` | New BDD feature file |

---

## Summary

| CP | Changes | Risk | Gate |
|---|---|---|---|
| 1 | New structs + YAML field (no behavior change) | Minimal — additive | `cargo check` |
| 2 | `available_cap()` implementation + unit tests | Low — no callers yet emit per-asset reservations | `cargo test` unit |
| 3 | `FlexibilityPolicy::reservations()` + unit tests | Low — not wired to production yet | `cargo test` unit |
| 4 | Wire into `spawn_planning()` | Medium — changes planner input; existing tests protect against regression | 123 BDD green + new policy scenario |
| 5 | `parse_firm_event_reservations()` + wire | Low — additive function; existing tests protect main path | unit tests + 123 BDD green |
| Gate | New BDD scenarios | — | All new scenarios green |

Total scope: ~350 lines of new code, ~5 lines changed in `loops.rs`, ~10 lines changed in `reservation.rs`.
No behavior change for VENs with no `flexibility_policy` in their YAML profile.
Sets the foundation for Phase D (planner loop refactor + `PlanReason` audit trail).

---

## Out of scope for Phase C

- Site-level headroom distribution across assets (`distribute()` policy — Phase D §5.4)
- `PlanReason::PolicyReserve` on every PlanStep (Phase D)
- `LookaheadContext` enrichment for proactive pre-loading (Phase D)
- `FlexibilityEnvelope` as first-class output (`compute_envelope()` — Phase E)
- Adaptive policy layer (learned patterns → auto-generated ScheduledWindows — future)
- `ComfortBound` reservations from asset profile (heater `comfort_min_c` / `comfort_max_c` — Phase D)
