# Spec Delta

## Purpose
Fix the unit in which the lab stores, passes and serves power and energy, so that a reader never
needs a conversion factor to interpret a number it receives from another layer.

## ADDED Requirements

### Requirement: Power is kW and energy is kWh across layers
Every power value the VEN, the BFF and the UIs store, pass between layers or serve over the lab's
own HTTP/telemetry APIs SHALL be expressed in kW, and every energy value in kWh. The field name
SHALL carry the unit suffix (`_kw`, `_kwh`; `Kw` in the BFF's camelCase API). A conversion SHALL
exist only at a boundary to an external protocol that requires a different unit.

#### Scenario: Site meter in kW
- **WHEN** a client reads the simulator state (`GET /sim`)
- **THEN** the grid object carries `net_power_kw` in kW
- **AND** it carries no field ending in `_w`

#### Scenario: The meter is the sum of the assets
- **WHEN** a client reads the simulator state (`GET /sim`)
- **THEN** `grid.net_power_kw` equals the sum of the assets' `power_kw`

#### Scenario: Fleet view in kW
- **WHEN** the VTN BFF serves the fleet view
- **THEN** each VEN's power is a kW value under a `*Kw` name and the VTN UI displays it without
  scaling

#### Scenario: Sensor power in kW
- **WHEN** a client posts a sensor reading with `power_kw`
- **THEN** the stored sensor snapshot reports the same value under `power_kw`

### Requirement: Persisted state survives the rename
State and recorder data written before the rename SHALL remain usable after it.

#### Scenario: Old sim_state.json
- **WHEN** the VEN starts from a `sim_state.json` whose grid meter holds `net_power_w`
- **THEN** the file loads, every asset's state and the meter's `import_kwh` / `export_kwh` are
  restored
- **AND** the meter's power fields hold kW after the first tick

#### Scenario: Old recorder rows
- **WHEN** the BFF starts on a fleet recorder that holds raw and one-minute rows written in watts
- **THEN** the same rows read back in kW
- **AND** starting the BFF again does not rescale them a second time

### Requirement: A mixed fleet shows no wrong value
While VENs on the old and the new image run side by side, the fleet view SHALL show a VEN whose
message it cannot read as not reporting, never as a value in the wrong unit.

#### Scenario: Old VEN, new BFF
- **WHEN** a VEN publishes telemetry that carries `grid.net_power_w` and no `grid.net_power_kw`
- **THEN** the fleet view lists that VEN without a power value
- **AND** the fleet sum excludes it
