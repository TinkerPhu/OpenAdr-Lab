# data-model.md

## Entities

### SimulatorPort (trait)

The port interface. Lives in `VEN/src/controller/simulator_port.rs`.

| Method | Signature | Notes |
|--------|-----------|-------|
| `snapshot` | `(&self) -> Result<SimSnapshot, SnapshotError>` | Point-in-time read; must not hold lock after returning |
| `inject` | `(&self, state: SimInjectState)` | Fire-and-forget override; silently dropped if uninitialized |

---

### SimSnapshot

Returned by `snapshot()`. Represents the simulator state at one tick.

| Field | Type | Notes |
|-------|------|-------|
| `ts` | `DateTime<Utc>` | Timestamp of the tick that produced this snapshot |
| `grid` | `GridSnapshot` | Aggregated grid metrics |
| `assets` | `HashMap<String, AssetSnapshot>` | Per-asset snapshots; key is asset id |

**Validation rules**:
- Must NOT contain `AssetHistoryBuffer` — history is a read-only query concern in `assets/mod.rs`.
- `assets` map keys must be stable asset ids consistent with those used by history routes.
- Snapshot is immutable after construction; all fields are owned values, not references.

---

### GridSnapshot

| Field | Type | Notes |
|-------|------|-------|
| `net_power_w` | `f64` | Net power in watts; positive = import from grid |
| `voltage_v` | `f64` | Grid voltage |
| `import_kwh` | `f64` | Cumulative energy imported (monotonically increasing) |
| `export_kwh` | `f64` | Cumulative energy exported (monotonically increasing) |

---

### AssetSnapshot

| Field | Type | Notes |
|-------|------|-------|
| `power_kw` | `f64` | Actual power in kW; positive = consuming, negative = producing |
| `values` | `HashMap<String, f64>` | Flattened asset-specific numeric state (e.g., `soc`, `temp_c`) |

**Validation rules**:
- `values` keys must be defined consistently per asset type; callers that depend on a specific key must document that assumption.
- Controllers that build setpoints must treat missing keys as "not applicable" rather than panicking.

---

### SnapshotError

| Variant | Semantics | Caller action |
|---------|-----------|---------------|
| `Uninitialized` | Simulator has not produced a tick yet | Wait and retry |
| `Transient` | Temporary condition (e.g., sim locked during long MILP solve) | Retry on next poll interval |
| `Fatal` | Unrecoverable simulator state | Abort operation, alert operator |

---

### SimInjectState

Override parameters applied by the simulator on the next tick.

| Field | Type | Notes |
|-------|------|-------|
| `ambient_temp_c_override` | `Option<f64>` | Override ambient temperature |
| `pv_irradiance_override` | `Option<f64>` | Override PV irradiance |
| `base_load_kw_override` | `Option<f64>` | Override base load |
| `ev_plugged_override` | `Option<bool>` | Override EV plugged-in state |
| `ev_soc_target_override` | `Option<f64>` | Override EV SoC target |
| `pv_alpha` | `f64` | PV smoothing factor (0.0–1.0) |
| `base_load_alpha` | `f64` | Base load smoothing factor (0.0–1.0) |

**Validation rules**:
- `pv_alpha` and `base_load_alpha` are smoothing factors; implementation may clamp to [0.0, 1.0].
- All `Option` fields are `None` by default (no override); `Some(v)` overrides the physics simulation value.

---

## Relationships

```
SimulatorPort (trait)
    │ returns
    ▼
SimSnapshot ──── contains ──▶ GridSnapshot
    │
    └─── contains (per asset id) ──▶ AssetSnapshot

SimulatorPort (trait)
    │ accepts
    ▼
SimInjectState

SnapshotError
    │ returned by
    ▼
SimulatorPort::snapshot()
```

`SimState` (in `simulator/mod.rs`) is the production implementor of `SimulatorPort`.  
`MockSimulatorPort` (in `services/test_support/`) is the test implementor.

