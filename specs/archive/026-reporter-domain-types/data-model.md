# Data Model: Reporter — Domain-Side Snapshot Types

**Feature**: 026-reporter-domain-types
**Date**: 2026-05-15

---

## New Entity: `AssetReportSample`

**Location**: `VEN/src/controller/reporter.rs` (domain ring)
**Ring**: Controller (domain) — no infra imports

```
AssetReportSample
├── ts: DateTime<Utc>       required — timestamp of the measurement
├── power_kw: f64           required — instantaneous power (positive = import, negative = export)
└── soc: Option<f64>        optional — state of charge as fraction 0.0–1.0; None for non-storage assets
```

**Validation rules**:
- `soc` must be `None` for PV, BaseLoad, Heater, Grid assets
- `soc` must be in range `[0.0, 1.0]` when present (enforced by caller at infra boundary)
- `power_kw` carries the same sign convention as `HistoryPoint.power_kw`: positive = import, negative = export
- No `Default` or `Serialize/Deserialize` derives needed — domain-internal type, never serialized to wire

**State transitions**: None — immutable value type (no methods, no state machine).

---

## Entity Usage Map

| Consumer | Access Pattern | Notes |
|----------|---------------|-------|
| `build_measurement_report` | latest sample per asset (`samples.last()`) | Reads `power_kw` for net import/export; reads `soc` for EV SoC payload |
| `build_measurement_reports_for_active_events` | passes map through to inner call | No direct field access |
| `build_measurement_report_for_obligation` | full vec per asset | Passes to `build_net_site_power_ts` and `build_soc_intervals` |
| `build_net_site_power_ts` | iterates all vecs | Converts to `TimeSeries` via `samples_to_power_ts` (reads `ts`, `power_kw`) |
| `build_soc_intervals` | "ev" / "battery" vecs only | Reads `ts` and `soc` to build SoC `TimeSeries` |
| `samples_to_power_ts` (private) | slice | Reads `ts`, `power_kw` — maps to `Vec<(DateTime<Utc>, f64)>` |

---

## Existing Entities (unchanged)

### `SimSnapshot` (existing, `controller/simulator_port.rs`)

Used by `build_status_report` after the change.

```
SimSnapshot
├── ts: DateTime<Utc>
├── grid: GridSnapshot
└── assets: HashMap<String, AssetSnapshot>
    └── AssetSnapshot
        ├── power_kw: f64           ← used for net_import_kw / net_export_kw
        ├── asset_type: String
        ├── cap_max_import_kw: f64
        ├── cap_max_export_kw: f64
        ├── available_discharge_kwh: Option<f64>
        ├── available_charge_kwh: Option<f64>
        ├── default_setpoint_kw: f64
        ├── setpoint_kw: f64
        └── values: HashMap<String, f64>   ← "soc" key for battery/EV
```

### `HistoryPoint` (existing, `assets/mod.rs`) — infra ring

After the change, `HistoryPoint` is used ONLY in `publish.rs` and `obligation.rs` (adapter ring) during boundary extraction. It is never referenced in `reporter.rs`.

```
HistoryPoint
├── ts: DateTime<Utc>
├── power_kw: f64
└── state: AssetState          ← infra enum; .soc() method extracts Option<f64>
```

**Mapping to `AssetReportSample`** (performed at infra boundary):
```rust
AssetReportSample {
    ts: point.ts,
    power_kw: point.power_kw,
    soc: point.state.soc(),
}
```

---

## Infra-Ring Reference Map (post-change)

| Type | Ring | Where Used After Change |
|------|------|------------------------|
| `AssetReportSample` | Domain (`controller/`) | `reporter.rs` — defined and consumed |
| `SimSnapshot` | Domain (`controller/`) | `reporter.rs` (`build_status_report`), `planning.rs` (caller already has it) |
| `SimState` | Infra (`simulator/`) | `publish.rs`, `obligation.rs` (lock source only — never passed to domain) |
| `HistoryPoint` | Infra (`assets/`) | `publish.rs`, `obligation.rs` (mapping only — never passed to domain) |

This table satisfies SC-001: no infra type appears in `controller/reporter.rs` after the change.
