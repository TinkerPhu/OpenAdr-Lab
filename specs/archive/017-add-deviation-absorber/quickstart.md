# Quickstart: Multi-Asset Deviation Absorber Implementation

**Phase**: 1 (Design) | **Date**: 2026-05-01 | **Target Branch**: `017-add-deviation-absorber`

---

## What You're Building

A two-tier deviation control system for the VEN HEMS controller that automatically absorbs grid power deviations without replanning:

- **Tier 1**: Real-time absorber that adjusts battery, EV, and heater setpoints sequentially to compensate for PV/load mismatches
- **Tier 2**: DeviceDeviation escalation that triggers MILP replanning only when absorber's residual deviation persists for 120 seconds (production)
- **Relay Wear Protection**: Enforce minimum dwell time between asset state changes to prevent mechanical wear

**Key Metric**: Reduce planner solve frequency from ~20s to ~120s, cutting Pi4 CPU load by 90%.

---

## Architecture Overview

```
Tick Loop (1s interval)
├─ PHASE 3: apply_deviation_absorption()
│     Input: grid deviation, sim state, plan, profile config
│     Process: iterate battery → EV → heater in priority order
│              apply corrections within flexibility bounds
│              enforce relay linger time
│     Output: residual uncovered deviation
│
└─ PHASE 6: accumulate_deviation() (using residual, not raw deviation)
      Input: residual from Phase 3
      Process: count ticks where |residual| > 0.1 kW
      Output: fire trigger_tx(DeviceDeviation) after 120 ticks sustained
```

---

## Files to Modify/Create

| File | Change | Effort |
|------|--------|--------|
| `VEN/src/profile.rs` | Add `AbsorberConfig`, `AbsorberAssetConfig` structs | Small |
| `VEN/src/controller/mod.rs` | Add `pub mod absorber;` | Trivial |
| `VEN/src/controller/absorber.rs` | **NEW** — Core absorber logic | Medium (150–250 lines) |
| `VEN/src/loops.rs` | Replace Layer 1 call, update Layer 2 | Small (10–15 line changes) |
| `VEN/profiles/test.yaml` | Add absorber config section | Trivial |
| `VEN/profiles/ven-{1,2,3}.yaml` | Add absorber config sections | Trivial |
| `tests/features/deviation_absorber.feature` | **NEW** — BDD scenarios | Small (50 lines) |
| `tests/steps/deviation_absorber_steps.py` | **NEW** — Step definitions | Small (30 lines) |

---

## Implementation Sequence

### 1. Profile Schema (30 min)

**File**: `VEN/src/profile.rs`

```rust
pub struct AbsorberConfig {
    pub enabled: bool,
    pub dead_band_kw: f64,
    pub dead_band_clearing_ticks: usize,
    pub assets: Vec<AbsorberAssetConfig>,
}

pub struct AbsorberAssetConfig {
    pub id: String,
    pub priority: u8,
    pub min_state_linger_s: u64,
    pub ev_departure_guard_s: Option<u64>,
}

// Add to Profile struct:
pub absorber: AbsorberConfig,
```

Add serde defaults for backward compatibility.

**Test**: Unit test that YAML with/without absorber section deserializes correctly.

---

### 2. Absorber Module (2–3 hours)

**File**: `VEN/src/controller/absorber.rs` — **NEW**

Core function:

```rust
pub fn apply_deviation_absorption(
    state: &mut AbsorberState,
    deviation_kw: f64,
    setpoints: &mut HashMap<String, f64>,
    sim: &SimState,
    plan_snap: Option<&Plan>,
    profile: &Profile,
    now: DateTime<Utc>,
    event_tx: &PlannerEventTx,
) -> f64   // returns residual
```

**Algorithm**:
1. Check: is absorber enabled? Is `|deviation| > dead_band_kw`?
2. Iterate profile's absorber asset list in priority order
3. For each asset:
   - Check linger time: has enough time elapsed since last state change?
   - Check EV departure guard: if EV and departure < guard duration, skip
   - Compute headroom (SoC bounds, temp bounds, power limits)
   - Apply correction: `delta_kw = remaining_deviation.clamp(-headroom, headroom)`
   - Update setpoint, record state change timestamp, update settling state
   - Accumulate remaining uncovered deviation
4. Return residual uncovered deviation (for Tier 2)
5. Emit SSE `CorrectionActive` / `CorrectionCleared` on state changes

**Helper functions** (pure, no side effects):
- `compute_asset_headroom()` — per-asset power budget
- `linger_ok()` — check if state change is allowed
- `emit_correction_event()` — SSE event bookkeeping

**Tests** (unit):
- Battery absorbs positive deviation within capacity
- EV absorbs residual when battery at floor
- Heater skipped when linger blocks
- EV skipped when departure guard active
- Residual returned when all at limits
- Settling logic ramps to zero correctly

---

### 3. Integration in loops.rs (1 hour)

**File**: `VEN/src/loops.rs`

- Rename `DeviationState` → `AbsorberState` (update imports + struct definition)
- PHASE 3 (line ~788): Replace
  ```rust
  let correction_kw = apply_deviation_correction(...);
  ```
  with
  ```rust
  let residual_kw = controller::absorber::apply_deviation_absorption(...);
  ```
- PHASE 6 (line ~853): Update call to `accumulate_deviation()` to pass `residual_kw` instead of `post_net_kw`

**Tests**: BDD scenarios will validate integrated behavior.

---

### 4. Profile YAML Updates (15 min)

**Files**: `VEN/profiles/test.yaml`, `ven-1.yaml`, `ven-2.yaml`, `ven-3.yaml`

Add section to each (example for test):

```yaml
absorber:
  enabled: true
  dead_band_kw: 0.1
  dead_band_clearing_ticks: 1
  assets:
    - id: battery
      priority: 0
      min_state_linger_s: 0
    - id: ev
      priority: 1
      min_state_linger_s: 0
      ev_departure_guard_s: 1800
    - id: heater
      priority: 2
      min_state_linger_s: 0    # 0 in test; 30 in production
```

Production profiles: set heater linger to 30–60 seconds.

---

### 5. BDD Scenarios (1.5 hours)

**File**: `tests/features/deviation_absorber.feature` — **NEW**

6 scenarios covering:
1. Baseline deviation absorption (battery alone)
2. Sequential fallback (battery to EV)
3. Linger enforcement (heater blocked, escalation)
4. EV departure guard (near departure, protection)
5. Tier 2 escalation (residual sustained 120s)
6. Settling behavior (overlay ramp to zero)

**File**: `tests/steps/deviation_absorber_steps.py` — **NEW**

Step implementations:
- `Given` battery SoC is X, EV departure in Y minutes, etc.
- `When` PV drops by Z kW, positive/negative deviation occurs
- `Then` battery/EV setpoint changes by delta, no DeviceDeviation fires, etc.

**Test execution**:
```bash
docker compose -f tests/docker-compose.test.yml run --build --rm test-runner features/deviation_absorber.feature
```

All 6 scenarios must pass, plus existing 42+ scenarios remain green.

---

## Key Implementation Notes

### Linger Time Tracking

Per-asset, in-memory only:

```rust
pub struct AbsorberState {
    pub last_state_change_ts: HashMap<String, DateTime<Utc>>,
    // ...
}

// At state change:
absorber_state.last_state_change_ts.insert(asset_id.clone(), now);

// At next potential change:
let can_change = match absorber_state.last_state_change_ts.get(asset_id) {
    None => true,  // first time
    Some(ts) => (now - ts).num_seconds() as u64 >= min_linger_s,
};
```

No persistence across restarts (acceptable; restarts are rare).

---

### Settling State Machine

```rust
pub struct AbsorberState {
    pub settling_ticks: HashMap<String, u32>,
    pub active_overlay_kw: HashMap<String, f64>,
    // ...
}

// When deviation clears below dead-band:
if settling_ticks.get(&asset_id).copied().unwrap_or(0) == 0 {
    settling_ticks.insert(asset_id.clone(), 1);  // Begin 1-tick ramp
}

// Next tick, ramp overlay to zero:
if settling_ticks[&asset_id] >= 1 {
    active_overlay_kw.insert(asset_id.clone(), 0.0);
    settling_ticks.insert(asset_id.clone(), 0);  // Complete
}
```

---

### SSE Event Deduplication

Avoid flooding SSE channel with repeated `CorrectionActive` events:

```rust
let total_correction_kw = active_overlay_kw.values().sum::<f64>();
if (total_correction_kw - last_emitted_correction_kw).abs() > 0.2 {
    // Emit event
    last_emitted_correction_kw = total_correction_kw;
}
```

---

## Testing Strategy

### Unit Tests (cargo test)

- `absorber.rs` mod tests: 10+ scenarios covering absorber logic, linger, headroom
- `profile.rs` mod tests: 3 scenarios for config deserialization

### BDD Integration Tests

- 6 new scenarios in `deviation_absorber.feature`
- Run via Docker test-runner; validate end-to-end behavior
- All existing scenarios must remain green (backward compatibility)

### Manual Validation (on Pi4)

Once BDD passes:
1. Deploy to Pi4 via Docker
2. Monitor `/sim` endpoint — verify battery/EV/heater setpoints adjust on deviation injection
3. Monitor SSE stream — verify `CorrectionActive` / `CorrectionCleared` events
4. Check planner logs — verify DeviceDeviation fires only after 120s sustained residual
5. Verify relay switching count reduces (if heater has linger configured)

---

## Checkpoints

| Checkpoint | Validation | Go/No-Go |
|------------|-----------|----------|
| Profile structs + YAML deserialization | `cargo test profile` passes | ✓ Go |
| Absorber module unit tests | `cargo test controller::absorber` passes | ✓ Go |
| Integration in loops.rs compiles | `cargo build` succeeds (no warnings) | ✓ Go |
| BDD scenarios | `docker compose test-runner features/deviation_absorber.feature` — 6/6 pass | ✓ Go |
| Existing BDD regressions | `docker compose test-runner` — all 42+ scenarios pass | ✓ Go |
| Code review + merge | Spec, code, tests, BDD all reviewed; ready for main branch | ✓ Go |

---

## Success Criteria (from Spec)

- ✅ SC-001: Planner solves ~120s (production) instead of ~20s (CPU load 5% vs 50%)
- ✅ SC-002: Heater relay switching 80%+ reduction (with min_state_linger_s=30)
- ✅ SC-003: 95% of small deviations (<2 kW, <60s) absorbed without escalation
- ✅ SC-004: All 42 existing BDD scenarios pass
- ✅ SC-005: 6 new absorber-specific BDD scenarios pass
- ✅ SC-006: Avg residual <0.5 kW over 24h production run
- ✅ SC-007: Battery SoC drift <5% over 24h
- ✅ SC-008: YAML profiles load successfully; startup validates absorber asset IDs

---

## Resources

- **Spec**: `spec.md` (complete requirements, acceptance scenarios, success criteria)
- **Data Model**: `data-model.md` (entities, fields, relationships, validation)
- **Research**: `research.md` (design decisions + rationale)
- **Constitution**: `C:\Users\TinkerPHU\.claude\projects\D--Tinker-OpenAdr-Lab\.specify\memory\constitution.md` (dev principles)
- **Key Learnings**: `docs/reference/KEY_LEARNINGS.md` (past lessons on Rust, Docker, BDD)
- **CLAUDE.md**: `CLAUDE.md` (project runtime guidelines)

---

## Ready to Implement

All design complete. Follow implementation sequence above. Estimated effort: **12–16 hours** (accounting for unit tests, BDD, Pi4 validation).
