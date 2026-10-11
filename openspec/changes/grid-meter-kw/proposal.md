# Proposal

> Written 2026-10-06, corrected against the source on `main` (`670750a7`) on 2026-10-11. Not
> started. Branch when it is: `refactor/grid-meter-kw`.

## Why

The lab states every power in kW and every energy in kWh: about 6,200 `_kw` identifiers in the
Rust code against 107 `_w`, and half of those are one field, `net_power_w`. The watt remnants
are the simulated site meter (`GridMeter.net_power_w` / `import_w` / `export_w`), the snapshot
and telemetry built from it, the sensor snapshot's `power_w`, and everything that reads them.

The meter is computed in kW and multiplied by 1000 at its one write site
(`simulator/grid_meter.rs`). Every reader then converts back, each on its own:

- VEN: `GridSnapshot::net_power_kw()`, `simulator/recording.rs`, and `EnergyCounter::integrate`,
  which is handed `w_from_kw(actual_kw)` and applies `kw_from_w` to it on the next line.
- VEN UI: `Controller.tsx` (2), `dataBuilders.ts` (2), `SimProfileChart.tsx`; `Dashboard.tsx`
  prints the raw watts.
- VTN UI: `formatKw(watts)` and `toKw` in `FleetPowerChart.tsx`.
- BDD: `alerts_steps.py`, `limit_enforcement_steps.py`.

So "which unit is this number in?" is answered in every consumer instead of in the name.
OpenADR 3.1 defines `KW` and `KWH` and no watt unit, and the VEN's reports already declare
`KW` / `KWH` (`controller/report_payload.rs`), so nothing on the OpenADR wire changes: this is
lab-internal vocabulary only.

## What Changes

- `GridMeter.net_power_w` / `import_w` / `export_w` become `net_power_kw` / `import_kw` /
  `export_kw`, stored in kW. The `w_from_kw` at the write site and every conversion at a reader
  are deleted. `GridSnapshot.net_power_w` becomes `net_power_kw` and its accessor method goes.
- `EnergyCounter::integrate` takes kW.
- `kw_from_w` and `w_from_kw` (`entities/units.rs`) lose their last callers and are deleted, with
  their pattern in `scripts/audit_ven_architecture.py`. The VEN then has no watt at all.
- `SensorSnapshot.power_w` / `SensorInput.power_w` (`entities/sensor.rs`, `GET`/`POST /sensors`)
  become `power_kw`; `SensorRaw.import_w` / `export_w` become `import_kw` / `export_kw`.
- The telemetry body (`grid.net_power_w`, it is the snapshot itself) and the BFF fleet API
  (`netPowerW`, `powerBeforeW`, `powerAfterW`, `deltaW`) become kW-named; the two recorder
  columns `fleet_telemetry.net_power_w` and `fleet_telemetry_1m.net_power_w_mean` are renamed
  and rescaled in place.
- UI: the VEN Dashboard prints kW instead of W; the VTN UI's `formatKw` wrapper and `toKw` are
  deleted in favour of the shared `formatKwOrDash`.
- **BREAKING (lab-internal):** the JSON field names above change on the VEN HTTP API, the
  telemetry message and the BFF fleet API. An old `sim_state.json` still loads (design D3);
  recorder rows are migrated (design D4).
- Documented convention: power is kW, energy is kWh, in every layer; a conversion exists only at
  a boundary to an external protocol that demands another unit.
- Not changed: OpenADR wire payloads (already `KW` / `KWH`); `experiments/` (it reads none of
  these fields); the g/kg CO₂ conversions, which are a different quantity.

## Capabilities

### New Capabilities
- `power-unit-convention`: the unit in which power and energy are stored, passed between layers
  and served over the lab's own HTTP and telemetry APIs.

### Modified Capabilities
<!-- None: openspec/specs/ holds no capabilities; current-state behaviour lives in docs/. -->

## Impact

- About 200 occurrences in 59 files: `VEN/src` (21 files), `VEN/ui/src` (19, three of them
  generated), `VTN/bff/src` (4), `VTN/ui/src` (8), `tests/features` (7).
- Deploy: BFF and both UIs, then all 20 VENs on both hosts. A mixed fleet shows gaps in the fleet
  view, never values off by 1000 (design D5).
- One irreversible step: the recorder column rescale. Dump the `lab_recorder` schema first.
- Estimated one day including the full E2E run and the fleet redeploy.
