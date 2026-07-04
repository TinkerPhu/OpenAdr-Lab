# Phase 0 Research: Multi-Asset Deviation Absorber

**Status**: Complete (all design decisions documented in spec and clarification session)

## Summary

No Phase 0 research tasks required. All technical decisions were explicitly made and documented during `/speckit.specify` and `/speckit.clarify` phases. This research document consolidates that context for implementers.

---

## Design Decisions (Consolidated from Spec & Clarifications)

### 1. Absorber Algorithm: Sequential Priority Model

**Decision**: Sequential priority iteration (battery → EV → heater), not proportional split.

**Rationale**: 
- Simplicity in implementation and testing.
- Priority order is explicit in profile; clear precedence.
- Proportional would require simultaneous adjustment of multiple assets, adding algorithm complexity and test coverage burden.

**Reference**: Spec Assumption 1, FR-002.

---

### 2. Tier 2 Escalation Threshold & Duration

**Decision**: Tier 2 fires when residual deviation is sustained **above 0.1 kW** (the `dead_band_kw` value) **for 120 seconds (production) or 10 seconds (test)**.

**Rationale**:
- Reuses dead-band threshold (already used for Tier 1 correction decision).
- 120-second window (120 ticks × 1s) provides natural baseline for production; filters transient PV ramps and load spikes.
- 10-second window in test enables fast BDD verification without artificial waits.
- Clear mapping: `deviation_trigger_ticks` × 1s/tick = duration.

**Reference**: Spec FR-005 (clarified), FR-010, Clarifications Session Q1.

---

### 3. Relay Wear Protection: Minimum State Linger Time

**Decision**: Per-asset `min_state_linger_s` enforced at runtime; absorber refuses state change if insufficient time has elapsed since last change.

**Rationale**:
- Mechanical relays have finite cycle budgets (~1M cycles, degrading to months under rapid cycling).
- Linger time is asset-specific: 0s for electronics (battery, EV), 30–60s for heater/boiler relays.
- Absorbed into absorber's sequential loop: if linger blocks an asset, loop continues to next asset.
- Dispatcher and other control layers also check linger (redundant safety).

**Reference**: Spec FR-003, Assumption 2, Deviation Control Suggestions § Relay Wear.

---

### 4. EV Departure Guard: Behavior When Departure Unknown

**Decision**: When departure time is unknown or unavailable, treat as "no guard" — absorber may reduce EV charging freely.

**Rationale**:
- User is responsible for setting departure time via EV session API if they want guard protection.
- Unknown departure time likely means no explicit user urgency; system should optimize grid flexibility.
- Conservative default (infinite guard) would waste grid coordination opportunity.
- Spec design already states guard is "opt-in" (Assumption 3).

**Reference**: Spec FR-008 (clarified), Clarifications Session Q2.

---

### 5. Settling Logic: 1-Tick Ramp

**Decision**: When deviation clears below dead-band, overlay ramps to zero over 1 tick, then asset returns to clean MILP setpoint.

**Rationale**:
- 1 second ramp is natural given 1s tick interval.
- Prevents counter-deviation spikes (immediate snap could reverse the correction).
- Settling tick counter per-asset handles cases where one asset is still correcting while another settles.

**Reference**: Spec FR-007, Assumption 6.

---

### 6. Profile Schema: AbsorberConfig Structure

**Decision**: New top-level `absorber:` section in profile YAML with global settings and per-asset priority list.

**Rationale**:
- Mirrors existing `planner:` and `simulator:` sections (familiar structure).
- Per-asset config allows heterogeneous linger times (0 for EV, 30+ for heater).
- EV-specific field isolates domain-specific logic.

**Reference**: Spec FR-009.

---

### 7. Observability: SSE Events (Existing Plan F Infrastructure)

**Decision**: Reuse existing `CorrectionActive` / `CorrectionCleared` SSE events from Plan F.

**Rationale**:
- Events already flow through planner event system; no new infrastructure needed.
- Structured payload includes deviation, correction magnitude, asset details.

**Reference**: Spec FR-012.

---

## Dependencies Verified

All required systems exist and are integrated:
- `Plan.current_slot()` method ✅
- `SiteFlexibilityEnvelope` struct ✅  
- `PlannerEvent` SSE system ✅
- BDD test framework (behave + Docker) ✅
- Asset config system ✅

---

## Ready for Phase 1 Design

Proceeding to data-model.md generation.
