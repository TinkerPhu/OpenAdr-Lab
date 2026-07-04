# Feature Specification: Multi-Asset Deviation Absorber with Relay Wear Control

**Feature Branch**: `017-add-deviation-absorber`  
**Created**: 2026-05-01  
**Status**: Draft  
**Input**: Tier 1 real-time multi-asset deviation absorber, Tier 2 DeviceDeviation gate with raised thresholds, relay wear enforcement with min_state_linger_s, profile schema updates

## User Scenarios & Testing *(mandatory)*

<!--
  IMPORTANT: User stories should be PRIORITIZED as user journeys ordered by importance.
  Each user story/journey must be INDEPENDENTLY TESTABLE - meaning if you implement just ONE of them,
  you should still have a viable MVP (Minimum Viable Product) that delivers value.
  
  Assign priorities (P1, P2, P3, etc.) to each story, where P1 is the most critical.
  Think of each story as a standalone slice of functionality that can be:
  - Developed independently
  - Tested independently
  - Deployed independently
  - Demonstrated to users independently
-->

### User Story 1 — VEN controller absorbs transient grid deviations without replanning (Priority: P1)

A site runs under MILP plan with battery, EV, and heater. PV output drops 2 kW unexpectedly due to cloud cover, creating a temporary grid import overage. The system should absorb this via available asset flexibility (battery discharge, EV charge reduction) and return to the original plan within seconds — without triggering expensive MILP replanning.

**Why this priority**: Frequent replans cause continuous solver load and cascading thermal impacts on Pi4. By handling transient deviations at runtime, the site remains stable and responsive without computational overhead.

**Independent Test**: Deploy with absorber enabled. Inject PV deviation. Verify battery/EV setpoints adjust, no DeviceDeviation trigger fires within 2 minutes.

**Acceptance Scenarios**:

1. **Given** battery SoC is 0.50 (above min) and plan expects 0.0 kW net import, **When** PV output drops 2.0 kW, **Then** battery setpoint decreases (more discharge) within 1 tick and absorbs the deviation.
2. **Given** battery is at min_soc, EV is plugged with SoC 0.30 (below target), **When** same 2.0 kW PV drop occurs, **Then** battery stays at max discharge and EV charge setpoint reduces to cover the residual.
3. **Given** absorber encounters +0.05 kW deviation (within 0.1 kW dead-band), **When** deviation persists, **Then** absorber produces zero correction and no overlay is applied.
4. **Given** deviation clears below dead-band for 1 tick, **When** overlay is active, **Then** setpoints ramp back to clean MILP allocation within 1 second.

---

### User Story 2 — Relay wear protection prevents rapid mechanical switching (Priority: P1)

A heater with mechanical relay (rated for 1M cycles) has `min_state_linger_s=30` in the profile. Absorber should refuse to change heater setpoint more than once per 30 seconds, preventing relay wear degradation from fast cycling.

**Why this priority**: Mechanical relays in heaters, boilers, and compressors have finite cycle budgets. Absorber-driven rapid on/off can shorten relay lifespan from years to months. Linger enforcement is essential for production reliability.

**Independent Test**: Set heater linger to 5s in test profile. Apply absorber correction. Verify heater setpoint does not change again within 5 seconds.

**Acceptance Scenarios**:

1. **Given** heater has `min_state_linger_s=30` and current state is OFF, **When** absorber changes heater to ON, **Then** the last state change timestamp is recorded.
2. **Given** last state change was 25 seconds ago, **When** absorber tries to change heater state again, **Then** the change is refused and absorber moves to next priority asset.
3. **Given** last state change was 31 seconds ago, **When** absorber tries to change heater state, **Then** the change is allowed and timestamp updates.
4. **Given** battery and EV are both at limits, **When** residual deviation remains and heater linger blocks, **Then** residual propagates to Tier 2 (DeviceDeviation escalation logic).

---

### User Story 3 — EV near-departure protection prevents charging interference (Priority: P2)

EV has `ev_departure_guard_s=1800` (30 min) in absorber config. When departure is less than 30 minutes away and SOC is below target, absorber should refuse to reduce EV charging (even if positive deviation exists), protecting on-time departure.

**Why this priority**: EV customers expect the car to be fully charged by departure. Absorber reducing charging 10 minutes before departure could leave the car underpowered. Guard duration is site-specific but prevents conflicts between energy optimization and user comfort.

**Independent Test**: Inject EV session with 20-min departure, SoC=0.30 (below target). Apply positive deviation. Verify absorber skips EV and uses battery instead.

**Acceptance Scenarios**:

1. **Given** EV departure in 25 minutes and SoC 0.20 (below soc_target), **When** positive deviation occurs (reduce import), **Then** absorber skips EV and does not reduce its charging setpoint.
2. **Given** EV departure in 35 minutes, **When** same positive deviation occurs, **Then** absorber is allowed to reduce EV charging if needed.
3. **Given** EV departure imminent (< 5 min) and SoC still below target, **When** negative deviation occurs (absorb surplus), **Then** absorber is allowed to increase EV charging.

---

### User Story 4 — Tier 2 escalation fires only when Tier 1 is exhausted (Priority: P2)

Absorber residual deviation (what couldn't be absorbed) is tracked. DeviceDeviation replan trigger fires only when residual exceeds threshold for sustained ticks — not on every raw grid deviation. This reduces solver calls and allows MILP to focus on structural plan mismatches.

**Why this priority**: Reducing replan frequency from every 20 seconds to every 60–120 seconds frees Pi4 resources and improves site responsiveness.

**Independent Test**: Apply sustained 4.0 kW positive deviation. Battery and EV both at limits. Verify DeviceDeviation fires after `deviation_trigger_ticks` of sustained residual (not after `deviation_trigger_ticks` of raw deviation).

**Acceptance Scenarios**:

1. **Given** absorber has exhausted all asset flexibility, **When** sustained residual deviation > threshold for `deviation_trigger_ticks` ticks, **Then** DeviceDeviation trigger fires and planner wakes.
2. **Given** absorber absorbs the full deviation (residual = 0), **When** deviation_trigger_ticks elapses, **Then** DeviceDeviation does NOT fire.
3. **Given** absorber reduces residual to within dead-band (e.g., 0.05 kW < 0.1 kW dead-band), **When** sustained for multiple ticks, **Then** residual is treated as absorbed (no escalation).

### Edge Cases

- What happens if all absorber assets are locked by linger constraints simultaneously? → Residual propagates to Tier 2 without waiting for lock expiry.
- How does absorber handle battery at both min and max SoC? → Battery provides zero headroom; absorber skips to EV.
- What if EV is unplugged mid-deviation absorption? → EV becomes unavailable for correction; absorber shifts load to remaining assets.
- What if MILP plan slot has no flexibility envelope data? → Absorber treats slot as fully committed (zero headroom).
- What if a replan is triggered while absorber is correcting? → New plan arrives and cleanest way is to ramp absorber overlay to zero over 1 tick (settling logic).

## Requirements *(mandatory)*

<!--
  ACTION REQUIRED: The content in this section represents placeholders.
  Fill them out with the right functional requirements.
-->

### Functional Requirements

- **FR-001**: System MUST implement a real-time Tier 1 absorber that measures grid deviation (actual_net_kw − planned_net_kw) every tick.
- **FR-002**: System MUST apply absorber corrections sequentially across priority-ordered assets (battery → EV → heater) within flexibility bounds.
- **FR-003**: System MUST enforce `min_state_linger_s` (minimum time between state changes) for each asset, refusing corrections when linger blocks.
- **FR-004**: System MUST track absorber residual (uncovered deviation after all assets exhausted) and use it — not raw grid deviation — as input to Tier 2 escalation.
- **FR-005**: System MUST escalate to Tier 2 (DeviceDeviation trigger) only when residual deviation is sustained above `dead_band_kw` (default 0.1 kW) for `deviation_trigger_ticks` ticks. With `deviation_trigger_ticks: 120` (production) this creates **120-second sustained deviation detection**; test profile uses 10 ticks = **10-second detection**.
- **FR-006**: System MUST implement dead-band hysteresis (default 0.1 kW) to prevent chatter from measurement noise and transient spikes.
- **FR-007**: System MUST support settling logic: when deviation clears, ramp all active overlays to zero within 1 tick, then reset to clean MILP setpoints.
- **FR-008**: System MUST refuse EV charging reduction when time-to-departure is less than `ev_departure_guard_s` (default 1800s / 30 min) and EV is below soc_target. If departure time is unknown or unavailable, treat it as "no guard" and allow absorber to adjust EV charging freely.
- **FR-009**: System MUST update profile schema with new `AbsorberConfig` section containing `enabled`, `dead_band_kw`, `dead_band_clearing_ticks`, and list of absorber assets (each with `id`, `priority`, `min_state_linger_s`, optional `ev_departure_guard_s`).
- **FR-010**: System MUST raise default `deviation_trigger_ticks` from 30 to 120 ticks in production profiles (test profile keeps 10 for speed).
- **FR-011**: System MUST compute per-asset headroom (available correction budget) based on SoC bounds (battery, EV), temperature bounds (heater), and current asset state.
- **FR-012**: System MUST emit SSE `CorrectionActive` / `CorrectionCleared` events when absorber overlay changes (reuse existing Plan F infrastructure).
- **FR-013**: System MUST validate that all absorber asset IDs match actual assets in the profile (fail at startup if mismatch).

### Key Entities

- **AbsorberConfig**: Contains global absorber settings (`enabled`, `dead_band_kw`, `dead_band_clearing_ticks`) and list of absorber-eligible assets.
- **AbsorberAssetConfig**: Per-asset absorber config with `id`, `priority` (u8, lower = earlier), `min_state_linger_s` (u64, seconds), and optional `ev_departure_guard_s` (EV only).
- **AbsorberState**: Runtime state tracking per-tick: residual deviation ticks counter, per-asset last state change timestamps, settling tick counters, active overlay setpoints, SSE bookkeeping.
- **GridDeviation**: Signed difference between actual and planned grid power (positive = import excess, negative = export excess).
- **AssetHeadroom**: Available corrective power budget for a single asset, bounded by SoC/temperature limits and maximum asset power.

## Success Criteria *(mandatory)*

<!--
  ACTION REQUIRED: Define measurable success criteria.
  These must be technology-agnostic and measurable.
-->

### Measurable Outcomes

- **SC-001**: Planner solve frequency reduces from ~every 20s (current test: 10s replan_interval_s + DeviceDeviation @ 10 ticks = ~1 solve/10s) to ~every 120s production baseline, reducing CPU load from ~50% to ~5% on Pi4 single-VEN.
- **SC-002**: Heater relay switching count under constant absorber activity drops by at least 80% (e.g., from 10 switches/min to < 2 switches/min with min_state_linger_s=30).
- **SC-003**: 95% of small grid deviations (< 2 kW, < 60s duration) are absorbed without triggering DeviceDeviation replan.
- **SC-004**: All 42 existing BDD scenarios continue to pass (backward compatibility).
- **SC-005**: New 6 absorber-specific BDD scenarios pass: baseline deviation absorption, linger enforcement, EV departure guard, multi-asset fallback, residual escalation, settling behavior.
- **SC-006**: Absorber residual (uncovered deviation after Tier 1) averages < 0.5 kW over a 24h production run under realistic weather (PV, base-load, EV fluctuation).
- **SC-007**: Battery SoC drift caused by absorber-driven discharge does not exceed 5% over 24h; MILP replan at next cycle corrects it within 1 planning horizon.
- **SC-008**: All profile YAML files (test, ven-1, ven-2, ven-3) load successfully with new absorber config; startup validates absorber asset IDs and logs warnings for any missing assets.

---

## Assumptions

1. **Sequential priority model**: Absorber iterates battery → EV → heater in order; once an asset absorbs, loop continues to next asset (not proportional split across all assets). This simplifies implementation and testing.
2. **Per-asset linger time is fine-grained** (seconds): Heater relays benefit from 30–60s linger; EV and battery use 0s (solid-state or sufficient cycle rating).
3. **EV departure guard is opt-in**: Only applies if `ev_departure_guard_s` is set in profile; absence of field = no guard (absorber can reduce EV charging freely).
4. **Plan flexibility envelope** (`import_flexibility_kw`, `export_flexibility_kw` per slot) already exists and is accurate; absorber trusts it as the hard boundary.
5. **Thermostat emergency override**: Heater emergency (temp ≤ temp_min_c) bypasses linger constraints to preserve thermal safety (existing behavior, unchanged).
6. **Settling is 1-tick ramp**: When deviation clears, overlay goes to zero immediately next tick; no multi-tick ramp needed (simplifies state machine).
7. **Dead-band clearing ticks is fixed at 1**: Once `|deviation| < dead_band` for 1 tick, settling begins. Increasing this value deferred to production feedback.
8. **No MILP integration yet**: MILP planner ignores linger constraints (soft penalty only); absorber and dispatcher enforce linger at runtime. Option B (MILP-aware linger) deferred.

---

## Clarifications

### Session 2026-05-01

- Q: Tier 2 escalation threshold — which value determines when residual triggers DeviceDeviation? → A: Use `dead_band_kw` (~0.1 kW). Tier 2 fires when residual is sustained above 0.1 kW for 120 ticks (production = 120s) or 10 ticks (test = 10s).
- Q: EV departure guard when departure time is unknown — default behavior? → A: Treat as "no guard" (allow absorber to reduce charging). User is responsible for setting departure times if they want guard protection.
