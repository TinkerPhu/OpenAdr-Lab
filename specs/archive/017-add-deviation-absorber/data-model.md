# Data Model: Multi-Asset Deviation Absorber

**Phase**: 1 (Design) | **Date**: 2026-05-01

---

## Entities

### AbsorberConfig (Profile-Level)

**Location**: `VEN/src/profile.rs` — global configuration in `Profile` struct

**Fields**:
- `enabled: bool` (default: false) — enable/disable absorber globally
- `dead_band_kw: f64` (default: 0.1) — magnitude threshold below which deviation is ignored (prevents chatter)
- `dead_band_clearing_ticks: usize` (default: 1) — ticks within dead-band before settling begins
- `assets: Vec<AbsorberAssetConfig>` — list of absorber-eligible assets with per-asset settings

**Relationships**:
- Contained in `Profile` struct (parallel to `planner: PlannerConfig`, `simulator: SimulatorConfig`)
- Referenced by `absorber.rs::apply_deviation_absorption()` at every tick
- Validated at startup: all asset IDs must exist in `SimState.assets` (FR-013)

**Serialization**: YAML in profile files; serde with `#[serde(default)]` for new fields

**Example**:
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
      min_state_linger_s: 30
```

---

### AbsorberAssetConfig (Per-Asset)

**Location**: `VEN/src/profile.rs` — contained in `AbsorberConfig.assets`

**Fields**:
- `id: String` — asset ID (must match an entry in `SimState.assets`)
- `priority: u8` — iteration order (0 = first, higher = later); must be unique
- `min_state_linger_s: u64` — minimum seconds between state changes (0 = no linger, typical: 30–60 for relays)
- `ev_departure_guard_s: Option<u64>` — (EV only) refuse charging reduction if departure < N seconds away (default: 1800 / 30 min); if unset, no guard

**Validation**:
- `id` must exist in `SimState.assets` (checked at startup, FR-013)
- `priority` values should form a contiguous sequence (0, 1, 2, ...) for clarity
- `min_state_linger_s: 0` for electronics (battery, EV); 30–60 for mechanical relays (heater, boiler)
- `ev_departure_guard_s` only meaningful for EV; ignored for other asset types

**Relationships**:
- Contained in `AbsorberConfig.assets` list
- Referenced during asset iteration in `apply_deviation_absorption()`
- Tied to runtime `AbsorberState.last_state_change_ts` and `AbsorberState.settling_ticks` per asset ID

---

### AbsorberState (Runtime)

**Location**: `VEN/src/loops.rs` (created once before the loop, persists across ticks)

**Fields**:
- `residual_ticks: u32` — counter of consecutive ticks where residual > dead_band_kw; resets when residual ≤ dead_band_kw
- `last_state_change_ts: HashMap<String, DateTime<Utc>>` — per-asset ID, timestamp of last state change (used for linger enforcement)
- `settling_ticks: HashMap<String, u32>` — per-asset ID, counter of ticks since deviation cleared; when ≥ 1, begin ramp to zero
- `active_overlay_kw: HashMap<String, f64>` — per-asset ID, current correction overlay (delta from MILP setpoint); 0.0 = no correction
- `correction_is_active: bool` — true if any asset has active overlay (used for SSE state bookkeeping)
- `last_emitted_correction_kw: f64` — magnitude of last emitted SSE event (used to deduplicate events)

**Initialization**:
```rust
let mut absorber_state = AbsorberState {
    residual_ticks: 0,
    last_state_change_ts: HashMap::new(),
    settling_ticks: HashMap::new(),
    active_overlay_kw: HashMap::new(),
    correction_is_active: false,
    last_emitted_correction_kw: 0.0,
};
```

**Lifecycle**:
- Created once before `loop { tick_interval.tick().await; ... }` at loop start (line ~727 in current loops.rs)
- Mutated at every tick by `apply_deviation_absorption()` and `accumulate_deviation()`
- Persists for the lifetime of the VEN process (in-memory only, not persisted to disk)
- Reset on process restart (all assets "fresh", no linger from prior run)

**Relationships**:
- Passed mutable to `apply_deviation_absorption()` at line ~788 (PHASE 3)
- Passed mutable to `accumulate_deviation()` at line ~853 (PHASE 6)
- Never serialized; purely transient state

---

## Domain Objects

### GridDeviation

**Definition**: Signed difference between actual and planned grid power.

**Formula**: 
```
deviation_kw = actual_net_kw - planned_net_kw
```

**Sign Convention**:
- Positive: importing more than planned (PV shortfall, base load spike) → absorber reduces import
- Negative: importing less than planned (PV surplus) → absorber absorbs surplus
- Zero: perfect alignment (rare)

**Measurement**:
- Captured at PHASE 2 (line ~776): `prev_actual_net_kw = sim_guard.grid.net_power_w / 1000.0`
- Compared to slot's planned net: `plan.current_slot(now).net_import_kw - plan.current_slot(now).net_export_kw` (or shorthand: `plan_net_kw`)

**Dead-band**:
- `|deviation_kw| < dead_band_kw` (0.1 kW) → absorber produces zero correction
- Prevents chatter from measurement noise and transient spikes

---

### AbsorberResidual

**Definition**: Uncovered deviation after all absorber assets have been iterated.

**Formula**:
```
residual_kw = deviation_kw - sum(corrections applied by absorber)
```

**Lifecycle in Tier 2 Escalation**:
1. Returned by `apply_deviation_absorption()` at PHASE 3 (line ~804)
2. Passed to `accumulate_deviation()` at PHASE 6 (line ~853)
3. Compared to dead-band: if `|residual_kw| > dead_band_kw`, increment `residual_ticks`
4. When `residual_ticks >= deviation_trigger_ticks` (120 in production), fire `trigger_tx.send(PlanTrigger::DeviceDeviation)`

---

### AssetHeadroom

**Definition**: Available corrective power budget for a single asset, bounded by physical limits.

**Computation** (per-asset, per-tick):

| Asset | Headroom Calculation | Bounds |
|-------|----------------------|--------|
| **Battery** | Discharge: min(SoC - min_soc, max_discharge_kw); Charge: min(1.0 - SoC, max_charge_kw) | [0, max_discharge_kw] or [0, max_charge_kw] |
| **EV** | Charge: min(soc_target - SoC, max_charge_kw); (discharge forbidden, max_discharge_kw = 0) | [0, max_charge_kw] |
| **Heater** | Power difference to 0, mid, or full tier (based on current state and temp bounds) | [0, max_kw] |

**Usage**:
- Limits the delta that can be applied to a setpoint: `delta_kw.clamp(-headroom, headroom)`
- Ensures absorber respects flexibility envelope and asset limits

---

## Validation Rules

### Profile Startup Validation (FR-013)

At VEN startup, when `absorber.enabled: true`:

1. **Asset ID Matching**: For each `AbsorberAssetConfig.id` in profile, verify an asset with that ID exists in `SimState.assets`
   - Error: Log ERROR, refuse to start VEN
   - Severity: High (absorber cannot function without valid asset IDs)

2. **Priority Uniqueness**: All `AbsorberAssetConfig.priority` values should be unique (optional, but recommended for clarity)
   - Warning: Log WARN if duplicates detected; absorber will still work (iteration order defined by iteration, not by priority)

3. **Linger Time Bounds**: Verify `min_state_linger_s` is reasonable (0–300s typical)
   - Warning: Log WARN if > 300s (likely configuration error)
   - Severity: Low (high values work but indicate user error)

4. **EV Departure Guard**: Only relevant if asset type is EV
   - Ignored if set on non-EV assets (no error)

---

## State Transitions

### AbsorberState.correction_is_active

```
┌─────────────────────────────────────────────────────────────┐
│ Idle (correction_is_active = false)                         │
│ - active_overlay_kw is {asset_id: 0.0} for all assets      │
│ - settling_ticks may be > 0 (ramping back)                 │
└───────────┬─────────────────────────────────────────────────┘
            │
            │ deviation > dead_band_kw, absorber applies correction
            ▼
┌─────────────────────────────────────────────────────────────┐
│ Correcting (correction_is_active = true)                    │
│ - active_overlay_kw has non-zero entries                    │
│ - SSE CorrectionActive emitted                              │
└───────────┬─────────────────────────────────────────────────┘
            │
            │ deviation ≤ dead_band_kw for 1 tick
            ▼
┌─────────────────────────────────────────────────────────────┐
│ Settling (correction_is_active transitions to false)         │
│ - active_overlay_kw ramps to zero over 1 tick              │
│ - settling_ticks increments                                 │
│ - After 1 tick, return to Idle                              │
│ - SSE CorrectionCleared emitted                             │
└─────────────────────────────────────────────────────────────┘
```

---

## Integration Points

### With loops.rs

- **PHASE 3** (line ~788): Call `apply_deviation_absorption()` after setpoints built, before sim.tick()
- **PHASE 6** (line ~853): Call `accumulate_deviation()` with residual from Phase 3, after sim.tick()
- Pass `absorber_state: &mut AbsorberState`, computed deviation, plan snapshot, profile config

### With profile.rs

- Add `absorber: AbsorberConfig` field to `Profile` struct (parallel to `planner`, `simulator`)
- Implement serde defaults for new fields (backward compatibility: profiles without absorber section use `enabled: false`)

### With dispatcher.rs

- No changes to `build_setpoints()` signature or behavior
- `apply_battery_correction_overlay()` may be refactored into absorber module, or left as helper function
- Absorber's headroom computation independently validates battery SoC/power limits (no shared state)

### With existing SSE event system

- Reuse `PlannerEvent::CorrectionActive` / `CorrectionCleared` variants
- Emit from within `apply_deviation_absorption()` via event_tx parameter

---

## Backward Compatibility

- **Profile YAML**: New `absorber:` section is optional; defaults to `enabled: false` (absorber inactive)
- **Existing deployments**: No absorber config → absorber disabled → existing Layer 1/2 behavior unchanged
- **Migration path**: Add `absorber:` section to production profiles incrementally; test profiles enable from start

---

## Ready for Implementation

All entities, relationships, and validation rules defined. Phase 1 design complete.
