# Design

## Context

See proposal.md. W exists only in the simulated site meter and what is derived from it. The meter
is written once per tick in `simulator/grid_meter.rs::derive_grid_meter` from kW values
(`w_from_kw(meter_kw)`) and converted back by each reader.

## Goals / Non-Goals

**Goals:** one unit (kW) from meter to screen; no consumer-side conversion; names that say the
unit; no watt helper left in the VEN.

**Non-Goals:** any OpenADR wire change; other quantities (V, °C, g/kg CO₂); new fields. `GET /sim`
exposes only the net meter reading today and keeps doing so: `import_kw` / `export_kw` stay in
the persisted meter and in `SensorRaw`, where they already are.

## Decisions

### D1. Rename and re-scale in one pass, compiler-led
Rename the field, delete the conversion at the single write site, then follow compile errors
(Rust) and `tsc` errors (UIs) to each reader. A rename that kept the value in W would make every
`_kw` name a lie, so name and value change together.

The compiler does not see the **untyped readers**. These are changed by hand and are what the
grep gate (task 6.1) and the BDD scenario (task 5.2) exist for:

| Untyped reader | Where |
|---|---|
| BFF reads the telemetry body by key | `VTN/bff/src/fleet.rs::net_power_w` (`telemetry.get("grid")?.get("net_power_w")`) |
| BFF builds its responses with `json!` | `VTN/bff/src/routes/fleet.rs` (`"netPowerW"`, `"deltaW"`) |
| SQL text | `VTN/bff/src/fleet_store.rs`, `fleet_reactions.rs` (`avg(net_power_w)`) |
| BDD steps | `alerts_steps.py`, `limit_enforcement_steps.py`, `fleet_steps.py`, `ven_sensors_steps.py`, `ven_integration_steps.py`; `ven_simulator.feature`, `ven_integration.feature` |
| `json!` test fixtures | `simulator/plan_context.rs`, `tasks/planning/mod.rs`, `state/mod.rs` |

### D2. Reuse inventory (one-concept-one-function)
| Concept | Where it lives today | Action |
|---|---|---|
| site meter reading | `GridMeter` (`simulator/mod.rs`), written in `grid_meter.rs` | single writer stays; stores kW |
| meter in kW for the VEN | `GridSnapshot::net_power_kw()` (`controller/simulator_port.rs`), `simulator/recording.rs` | both go; the field is read directly |
| W to kW in the VEN UI | `Controller.tsx`, `dataBuilders.ts`, `SimProfileChart.tsx`; `Dashboard.tsx` prints W | delete; Dashboard uses the shared `formatKwOrDash` (`ui-charts/unitFormat`) |
| kW formatting in the VTN UI | `formatKw(watts)` (`VTN/ui/src/utils/power.ts`), `toKw` (`FleetPowerChart.tsx`) | both deleted; once the value is kW, `formatKw` is `formatKwOrDash` under a second name, so its call sites call that |
| energy counter input | `EnergyCounter::integrate(net_w, ..)` (`simulator/energy.rs`), one caller | takes kW |
| W/kW helpers | `kw_from_w`, `w_from_kw` (`entities/units.rs`) | no caller left: deleted with their tests and the `net_power_w / 1000\.0` pattern in `scripts/audit_ven_architecture.py` |
| fleet net power | `fleet::net_power_w` (`VTN/bff/src/fleet.rs`), `fleet_store.rs`, `fleet_reactions.rs` | renamed `net_power_kw`, still the one accessor |
| VEN UI API types | `VEN/ui/src/api/generated/{GridSnapshot,SensorSnapshot,SensorRaw}.ts` (ts-rs) | regenerated (`UPDATE_UI_TYPES=1 cargo test ui_types`), never edited |

### D3. `sim_state.json` and the persisted sensor: default, do not convert
The meter's three instantaneous fields are overwritten on every tick, so a value loaded from disk
lives for at most one tick. They need no divide-by-1000 migration. They do need
`#[serde(default)]` (on `GridMeter`, which already derives `Default`): without it an old file
lacks `net_power_kw`, fails to parse, and `persist::load` starts the VEN fresh, losing every
asset's state and the energy counters. `import_kwh` / `export_kwh` keep their names and values.

The persisted `sensor` section (`state/persistence.rs`) is restored field by field and replaced on
the next tick: an old `power_w` is ignored, and old `import_w` / `export_w` keys land in
`SensorRaw.extra` until then. No code for it.

### D4. Recorder: rename is the guard
The fleet store has no schema version; `fleet_store::init_schema` runs `CREATE TABLE IF NOT
EXISTS` at start-up. Add one step before those statements, in one transaction per table: if the
old column exists (`information_schema.columns`), `ALTER TABLE … RENAME COLUMN` and
`UPDATE … SET <new> = <new> / 1000`. After the rename the old column no longer exists, so the step
cannot run twice, and the transaction keeps rename and rescale together.

| Table | Column | Retention |
|---|---|---|
| `lab_recorder.fleet_telemetry` | `net_power_w` → `net_power_kw` | 7 days |
| `lab_recorder.fleet_telemetry_1m` | `net_power_w_mean` → `net_power_kw_mean` | 90 days |

`fleet_telemetry.payload_json` keeps the bodies as they were received (old ones say
`grid.net_power_w` in watts). Nothing reads them back, they are history, and they age out in 7
days: not migrated.

The BFF has no database tests. The migration is one function; it is exercised on the test stack
by starting the new BFF on a database the old one created (task 4.3).

### D5. Deploy order: gaps, not wrong values
The BFF reads the meter by key and returns "absent" when the key is missing, which the fleet view
already draws as "not reporting". So a mixed fleet never shows a value off by 1000. Order: dump
`lab_recorder`, deploy BFF and VTN UI, then the VENs host by host, then the VEN UI. During the
roll-out, VENs still on the old image are missing from the fleet view. No compatibility reader
for the old key: it would be a second copy of the rule, kept for an hour.

### D6. Sensors
`SensorInput.power_w` is a lab API, not an external protocol, so it becomes `power_kw` for the
same reason. The BDD sensor steps send kW.

### D7. Keeping it true
After the change no power identifier in `VEN/src` ends in `_w`. Add that to
`scripts/audit_ven_architecture.py` rule 10 (an identifier ending `_w` in production code), after
checking the pattern against the tree for false positives (weights are spelled `w_…`, voltage is
`_v`).

## Risks / Trade-offs

- A missed untyped reader shows a value off by 1000x or nothing. Mitigation: D1's table, the grep
  gate, and a BDD scenario asserting that `grid.net_power_kw` equals the sum of the assets'
  `power_kw`, which a watt value fails by three orders of magnitude.
- The recorder rescale is irreversible in place: dump the schema first (D5).
- 20 VENs on two hosts are redeployed; run no experiment during the roll-out.
