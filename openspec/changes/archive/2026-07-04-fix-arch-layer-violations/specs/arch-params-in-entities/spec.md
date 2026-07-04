## ADDED Requirements

### Requirement: Asset physics-parameter structs live in entities

`BatteryParams`, `EvParams`, `EvCharger` (EV param wrapper), `HeaterParams`, `PvParams`, `PvInverter` (PV param wrapper), `BaseLoadParams`, and `GridConfig` SHALL be defined in `entities/asset_params.rs` as pure data structs (no `impl Asset`, no physics logic). The `AssetParams` enum and `AssetRequestSlice` struct remain in the same file.

#### Scenario: entities/asset_params.rs contains param struct definitions
- **WHEN** `VEN/src/entities/asset_params.rs` is compiled
- **THEN** it defines `BatteryParams`, `EvParams`, `HeaterParams`, `PvParams`, `BaseLoadParams`, `GridConfig` as structs with `#[derive(Debug, Clone, Serialize, Deserialize)]`

#### Scenario: assets/ modules import params from entities
- **WHEN** `VEN/src/assets/battery.rs`, `ev.rs`, `heater.rs`, `pv.rs`, `base_load.rs`, `grid.rs` are compiled
- **THEN** each imports its params struct via `use crate::entities::asset_params::<T>` and does NOT define its own `*Params` struct

#### Scenario: milp_planner imports params from entities not assets
- **WHEN** `VEN/src/controller/milp_planner/` files are compiled
- **THEN** all `use crate::assets::` imports of `*Params` types are replaced with `use crate::entities::asset_params::<T>`

### Requirement: assets/ milp_planner invariant holds

`grep -r "use crate::assets::" VEN/src/controller/milp_planner/` (excluding test modules) SHALL return no matches in production code.

#### Scenario: invariant grep returns empty in production code
- **WHEN** the grep command `grep -r "use crate::assets::" VEN/src/controller/milp_planner/` is run and test-only lines are excluded
- **THEN** there are zero production-code matches

### Requirement: assets/ does not re-export MILP types

`assets/battery.rs`, `assets/ev.rs`, `assets/heater.rs` SHALL NOT contain `pub use crate::controller::milp_planner::asset_port::*` re-export lines. Consumers of `BatteryMilpContext`, `EvMilpContext`, `HeaterMilpContext` etc. SHALL import directly from `crate::controller::milp_planner::asset_port`.

#### Scenario: assets battery has no pub use milp re-export
- **WHEN** `VEN/src/assets/battery.rs` is inspected
- **THEN** there is no line matching `pub use crate::controller::milp_planner`

#### Scenario: assets ev has no pub use milp re-export
- **WHEN** `VEN/src/assets/ev.rs` is inspected
- **THEN** there is no line matching `pub use crate::controller::milp_planner`

#### Scenario: assets heater has no pub use milp re-export
- **WHEN** `VEN/src/assets/heater.rs` is inspected
- **THEN** there is no line matching `pub use crate::controller::milp_planner`

### Requirement: entities does not import from assets

`VEN/src/entities/` SHALL contain no `use crate::assets::` imports.

#### Scenario: entities has no assets imports
- **WHEN** `grep -r "use crate::assets::" VEN/src/entities/` is run
- **THEN** it returns no matches

### Requirement: VEN compiles without errors after params migration

After completing the params migration, `wsl cargo check` in the VEN directory SHALL exit with code 0 and report zero errors.

#### Scenario: cargo check passes after track A
- **WHEN** `wsl cargo check` is executed in `VEN/`
- **THEN** exit code is 0 and stderr contains no `error[` lines
