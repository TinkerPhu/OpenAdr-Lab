# Feature Specification: Planner State Forecast in Timeline API

**Feature Branch**: `015-planner-state-forecast`  
**Created**: 2026-04-27  
**Status**: Draft  
**Input**: User description: "soc for battery and ev as well as T_tank for heater suppose to be in the timeline api but it appears, they are only in past. I would like to see the future values, the ones the milp planner is using to decide the charge / heating intervals. is this possible? if possible, plan for it. make sure to implement it in the asset as this is asset data that should come from the asset and should be ready for the timeline api (even it gets created during milp planning)"

## Context

The VEN timeline API (`GET /timeline/all` and `GET /timeline/:asset_id`) returns historical and future data for each asset. Historical points include rich state values — battery and EV include `soc`, heater includes `temp_c` (T_tank). However, future points (derived from the MILP plan) currently only expose `power_kw`, `cost_rate_eur_h`, and `co2_rate_g_h`. The MILP planner already internally computes the full state trajectory for each asset as part of its optimisation (battery SoC trajectory, heater tank energy trajectory), and can derive the EV SoC trajectory from its per-slot power schedule. These trajectories drive the planner's charge/heating interval decisions but are currently not surfaced to the API consumer.

This feature closes that gap: each asset becomes responsible for producing its own future state values from the plan, and those values flow through to the timeline API.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - See Battery SoC Forecast in Timeline (Priority: P1)

An operator opens the VEN dashboard and looks at the battery asset's timeline chart. The chart currently shows historical SoC on the left of "now" and only power on the right. The operator wants to see the projected SoC curve extending into the future — the same curve the planner used to decide when to charge and when to discharge the battery.

**Why this priority**: Battery SoC is the single most critical piece of information for verifying planner decisions. Without it, the operator cannot tell whether the planner scheduled a charge cycle because the battery is running low, or whether it will hit its limits mid-day.

**Independent Test**: Call `GET /timeline/battery` with `hours_forward > 0`. Future points must include a `soc` key alongside `power_kw`. The `soc` value at the first future point should be consistent with the `soc` at the now-point.

**Acceptance Scenarios**:

1. **Given** the MILP planner has produced a plan with a battery charge schedule, **When** the timeline API is queried with a forward window, **Then** future battery points include a `soc` value (0.0–1.0) that reflects the start-of-slot state of charge entering the slot (before the slot's planned power is applied).
2. **Given** the battery is at 80% SoC and the plan charges it to 95% over two hours, **When** the timeline is fetched, **Then** the future `soc` values rise monotonically across charging slots from ~0.80 toward ~0.95.
3. **Given** no plan is active (planner has not yet run), **When** the timeline is queried, **Then** future battery points have `power_kw` only — no `soc` key — and the response is still valid.

---

### User Story 2 - See EV SoC Forecast in Timeline (Priority: P1)

An operator looks at the EV charger's timeline. The EV is plugged in with a departure deadline and the planner has scheduled smart charging intervals. The operator wants to see the projected SoC curve — when will the battery reach the target SoC, and how does it evolve across the charging windows the planner selected.

**Why this priority**: Operators and users need to verify that the EV will be charged to the requested level by the departure time. Without future SoC, the timeline shows only on/off charging intervals with no confidence indicator.

**Independent Test**: Call `GET /timeline/ev` with `hours_forward > 0` while an EV session is active. Future points must include a `soc` key derived by integrating the planned charging power forward from the live SoC.

**Acceptance Scenarios**:

1. **Given** an EV is plugged in with a target SoC of 0.80 and current SoC of 0.50, **When** the timeline is queried, **Then** future EV points include a `soc` value starting near 0.50 and ending at or near 0.80 by the departure slot.
2. **Given** the EV is unplugged (no active session), **When** the timeline is queried, **Then** future EV points do not include a `soc` key (or include last-known SoC unchanged), and no error occurs.
3. **Given** the plan schedules non-contiguous charging intervals, **When** the timeline is fetched, **Then** the `soc` values increase only during charging intervals and hold steady in gaps.

---

### User Story 3 - See Heater T_tank Forecast in Timeline (Priority: P2)

An operator looks at the hot-water tank heater's timeline. The historical view shows `temp_c` oscillating around the setpoint. The operator wants to see the projected tank temperature across the planning horizon — where the planner predicts the temperature will go, which heating slots it selected, and whether the tank will stay within its comfort band.

**Why this priority**: Temperature trajectory is the direct output of the heater's thermal model inside the planner. Without it, the future view only shows which slots are on or off, not whether those decisions keep the temperature in the acceptable range.

**Independent Test**: Call `GET /timeline/heater` with `hours_forward > 0`. Future points must include a `temp_c` key. Values must stay within `temp_min_c` and `temp_max_c` bounds (or only briefly breach them, matching the planner's feasibility).

**Acceptance Scenarios**:

1. **Given** a heater with `temp_min_c = 45°C` and `temp_max_c = 65°C` and the planner has scheduled heating cycles, **When** the timeline is queried, **Then** future heater points include a `temp_c` value that reflects the tank temperature at the start of each planning slot.
2. **Given** the tank starts at 52°C and the planner schedules two heating intervals, **When** the timeline is fetched, **Then** `temp_c` values rise during heating slots and decay between them in a physically plausible pattern.
3. **Given** no heater is configured in the VEN profile, **When** the timeline is queried, **Then** the heater asset is absent from the response (unchanged behaviour).

---

### Edge Cases

- **No plan available**: If the MILP planner has not yet run (e.g., on first boot), future points should still be returned with `power_kw` only — no state values — without error.
- **Plan covers a shorter horizon than the query window**: Future points beyond the end of the plan have `values: null` (existing behaviour). No state values are emitted for those points.
- **Battery not present**: No `soc` key appears on any non-battery asset's future points.
- **EV disconnects mid-plan**: After a disconnect event triggers a replan, the new plan's SoC trajectory reflects the new state. Old trajectory is discarded with the old plan.
- **SoC out of bounds**: If planner arithmetic yields a SoC slightly outside [0.0, 1.0] due to floating-point rounding, values must be clamped before emission.
- **Heater temperature conversion**: The tank energy stored by the planner is relative to `T_min`. If `T_min` or `thermal_mass` change between plan and query, the conversion must use the values at plan time (stored with the plan) to ensure consistency.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: Each storage asset (battery, EV) MUST contribute a `soc` value to its future timeline points when a plan is active.
- **FR-002**: The heater asset MUST contribute a `temp_c` value to its future timeline points when a plan is active.
- **FR-003**: Future state values (SoC, T_tank) MUST be computed by each asset from the MILP planner's output — they are asset-owned data and MUST NOT be hardcoded in the timeline controller.
- **FR-004**: The SoC for battery MUST be derived from the planner's per-slot energy trajectory (end-of-slot value), expressed as a fraction of nameplate capacity (0.0–1.0).
- **FR-005**: The SoC for EV MUST be derived by integrating the planner's per-slot charge power forward from the live SoC at plan time, expressed as a fraction of EV battery capacity (0.0–1.0).
- **FR-006**: The tank temperature for heater (`temp_c`) MUST be derived from the planner's per-slot tank energy trajectory using the thermal model parameters captured at plan time.
- **FR-007**: State values MUST be stored per planning slot in the plan data structure so that the timeline controller can retrieve them by asset and slot without re-running physics.
- **FR-008**: Future state values MUST be consistent with the future `power_kw` values in the same timeline points — both must come from the same MILP solution.
- **FR-009**: When no plan is active or the current plan slot does not cover the queried future bucket, the timeline point MUST have `values: null` (existing behaviour preserved).
- **FR-010**: SoC values emitted in the API MUST be clamped to [0.0, 1.0].
- **FR-011**: The feature MUST NOT change the timeline API response shape or break existing consumers — it only adds new keys within the existing `values` map.

### Key Entities

- **Plan Slot**: A single time step in the active MILP plan. Extended to carry per-asset state values (SoC, temperature) computed from the solver output during plan assembly.
- **Asset State Forecast**: A set of asset-specific state key-value pairs (e.g., `{"soc": 0.75}` for battery/EV, `{"temp_c": 55.0}` for heater) produced by each asset during plan translation and attached to the corresponding plan slot.
- **Timeline Point (Future)**: An API response element for a grid-aligned future timestamp, whose `values` map now includes both power/cost fields (existing) and asset state fields (new).

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: After a successful MILP planning cycle, 100% of future timeline points for battery and EV include a `soc` key, and 100% of future heater points include a `temp_c` key, when a plan covers that time bucket.
- **SC-002**: All emitted future `soc` values are within [0.0, 1.0] and all `temp_c` values are physically plausible (within the configured temperature band ± 5°C tolerance for extreme edge cases).
- **SC-003**: The timeline API response time is unchanged — adding state values to plan slots does not measurably increase query latency (< 5% change measured over 100 consecutive queries).
- **SC-004**: Existing timeline API consumers (VEN UI charts for `power_kw`, `cost_rate_eur_h`, `co2_rate_g_h`) continue to work unchanged — no regression in current chart rendering.
- **SC-005**: A new MILP plan replaces all future state values atomically — partial or inconsistent state (e.g., old SoC with new power) never appears in a single API response.

## Assumptions

- The MILP planner already computes a complete battery energy trajectory (`e_bat_kwh`, n+1 points) and heater tank energy trajectory (`e_heat_tank_kwh`, n points) as part of its solve. No additional solver variables are required for battery or heater.
- For EV, a SoC trajectory is not a direct MILP output, but can be computed accurately by integrating the planner's per-slot EV power (`p_ev_kw`) forward from the initial SoC captured at plan time. The initial SoC is deterministically available at plan assembly time.
- The thermal model parameters needed to convert tank energy to temperature (`temp_min_c`, `thermal_mass_kwh_per_c`) are stable for the lifetime of a single plan and available from the VEN profile at plan assembly time.
- The `PlanTimeSlot` data structure can be extended to carry a map of per-asset state values without breaking serialisation compatibility (new field uses `#[serde(default)]`).
- The timeline controller already processes per-asset data from plan slots (it reads `slot.allocations` and `slot.planned_kw_by_asset`). Adding state lookup to this path is straightforward.
