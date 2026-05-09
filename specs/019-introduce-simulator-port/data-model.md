# data-model.md

## Entities

### SimSnapshot
- ts: DateTime<Utc> — timestamp of snapshot
- grid: GridSnapshot — aggregated grid metrics
- assets: HashMap<String, AssetSnapshot> — per-asset snapshots keyed by asset id

### GridSnapshot
- net_power_w: f64 — net power in watts (positive import)
- voltage_v: f64 — voltage
- import_kwh: f64 — cumulative import energy
- export_kwh: f64 — cumulative export energy

### AssetSnapshot
- power_kw: f64 — actual power in kW (positive import, negative export)
- values: HashMap<String, f64> — flattened asset-specific state numeric fields (e.g., soc, temp)

### SimInjectState
- ambient_temp_c_override: Option<f64>
- pv_irradiance_override: Option<f64>
- base_load_kw_override: Option<f64>
- ev_plugged_override: Option<bool>
- ev_soc_target_override: Option<f64>
- pv_alpha: f64
- base_load_alpha: f64

## Notes & Validation Rules
- SimSnapshot must be compact and NOT include AssetHistoryBuffer (history moved to assets module)
- AssetSnapshot.values numeric fields must be defined consistently across assets for endpoint consumers

## Usage
- The `SimulatorPort::snapshot()` returns a full `SimSnapshot` representing the simulator's most recent tick. The spec clarifies that snapshot returns `Result<SimSnapshot, SnapshotError>` to model transient failures.

