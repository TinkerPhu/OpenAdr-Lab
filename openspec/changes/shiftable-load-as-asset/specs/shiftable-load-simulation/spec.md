## ADDED Requirements

### Requirement: Shiftable load is a simulated asset
A shiftable load SHALL be represented as a `Box<dyn Asset>` entry in
`SimState.asset_configs`/`SimSnapshot.assets` from the moment its request is
accepted, participating in the simulator tick loop, `iter_assets()`,
diagnostics, and capacity/envelope forecasting the same way every other asset
kind does — not as a side-channel record outside `SimState`.

#### Scenario: Accepted request is visible as an asset before it starts
- **WHEN** a shiftable-load request is accepted via the HEMS request API
- **THEN** `GET /sim` (or the equivalent internal `iter_assets()` view) SHALL
  include an entry for that asset with `started = false`, before the MILP has
  chosen a start slot

#### Scenario: Running load is visible the same way as any other asset
- **WHEN** a shiftable load has started (see the non-interruptible start
  requirement below)
- **THEN** its entry in `SimSnapshot.assets` SHALL report its actual current
  power draw (`power_kw`) exactly as Battery/EvCharger/Heater report theirs —
  no separate runtime-tracking API is required to see it running

### Requirement: Fixed power, non-interruptible once started
A shiftable load SHALL draw a fixed power level while running (no modulation:
`p_min_kw == p_max_kw`), and once started SHALL continue running at that fixed
power for its full configured duration regardless of any setpoint subsequently
requested of it.

#### Scenario: A later zero setpoint does not stop a running load
- **GIVEN** a shiftable load that has started and has remaining duration
- **WHEN** the simulator ticks with a plan setpoint of `0` for that asset
- **THEN** the asset SHALL still draw its full rated `power_kw`, unaffected by
  the requested setpoint

#### Scenario: Load finishes after its configured duration
- **GIVEN** a shiftable load that started at time `t0` with duration `d`
- **WHEN** the simulator reaches `t0 + d`
- **THEN** the asset SHALL stop drawing power and SHALL be removed from
  `SimState.asset_configs`/`SimSnapshot.assets`

### Requirement: Hard scheduling window
A shiftable load's `[earliest_start, latest_end]` window SHALL be treated as a
hard constraint by the planner: a schedule that cannot start and finish the
load within its window SHALL be treated as infeasible for that load, not as a
suboptimal-but-acceptable placement.

#### Scenario: Planner never schedules a start outside the valid window
- **GIVEN** a shiftable load with `earliest_start`, `latest_end`, and
  `duration_min`
- **WHEN** the MILP planner produces a schedule for this asset
- **THEN** the chosen start time SHALL satisfy
  `earliest_start <= start <= latest_end - duration_min`

### Requirement: Dynamic asset roster supports add and remove
Unlike the fixed-at-boot asset roster (Battery/EvCharger/Heater/PvInverter/
BaseLoad), the simulator SHALL support adding a new shiftable-load asset
instance when a request is accepted and removing one when a not-yet-started
request is cancelled or a running load finishes, without disturbing any other
asset's persisted mutable state.

#### Scenario: Cancelling a pending request removes its asset entry
- **GIVEN** a shiftable-load asset with `started = false`
- **WHEN** the user cancels the corresponding request
- **THEN** the asset entry SHALL be removed from `SimState.asset_configs`/
  `SimSnapshot.assets`

#### Scenario: Duplicate asset_id is rejected
- **GIVEN** an existing shiftable-load asset with `asset_id = "wm"`
- **WHEN** a new request for `asset_id = "wm"` is accepted
- **THEN** the system SHALL reject it, matching today's duplicate-`asset_id`
  behavior

#### Scenario: Unrelated asset state survives a shiftable-load add/remove across a restart
- **GIVEN** a persisted `SimState` containing a Battery with a mutated SoC and
  no shiftable-load entries
- **WHEN** a shiftable-load asset is added, the state is persisted, the load
  finishes and is removed, and the VEN restarts
- **THEN** the Battery's persisted SoC SHALL still be restored correctly —
  shiftable-load roster churn SHALL NOT cause the persisted-state reload to
  fall back to a fresh state for other assets

### Requirement: MILP planning uses `AssetMilpContext`
Shiftable-load scheduling in the MILP planner SHALL go through the same
`AssetMilpContext` trait used by Battery/EvCharger/Heater, not a bespoke
struct wired directly into the solver phases.

#### Scenario: Solver schedule parity with the previous bespoke implementation
- **GIVEN** an existing shiftable-load MILP test fixture that previously
  exercised the bespoke `ShiftableLoadMilp` path
- **WHEN** the same fixture is solved through `ShiftableLoadMilpContext`
- **THEN** the resulting schedule SHALL be identical to the pre-migration
  result

### Requirement: Forecasting reads shiftable loads from the asset snapshot
`capacity_forecast.rs` and `envelope_forecast.rs` SHALL derive shiftable-load
contributions from `SimSnapshot.assets`, not from bolt-on
`&[ShiftableLoad]`/`&[ShiftableLoadRuntime]` parameters.

#### Scenario: Capacity forecast reflects a pending load's future window
- **GIVEN** a shiftable-load asset with `started = false` and a future
  `[earliest_start, latest_end]` window
- **WHEN** the capacity forecast is computed
- **THEN** it SHALL account for that load's possible future power draw using
  only data available from `SimSnapshot.assets`
