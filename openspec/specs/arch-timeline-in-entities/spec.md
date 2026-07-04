# arch-timeline-in-entities Specification

## Purpose
TBD - created by archiving change fix-arch-layer-violations. Update Purpose after archive.
## Requirements
### Requirement: Timeline data-carrier types live in entities

`HeaterPlanTrajectory`, `TimelinePoint`, `TimelineAssetData`, `TimelineSnapshot`, and `TimeWindow` SHALL be defined in `entities/timeline.rs` as plain data structs with `#[derive(Debug, Clone, Serialize, Deserialize)]` (or the subset of derives each currently has). They SHALL contain no controller orchestration logic.

#### Scenario: entities/timeline.rs is created with data carrier types
- **WHEN** `VEN/src/entities/timeline.rs` is compiled
- **THEN** it defines `HeaterPlanTrajectory`, `TimelinePoint`, `TimelineAssetData`, `TimelineSnapshot`, and `TimeWindow` as structs

#### Scenario: entities/mod.rs exports the timeline module
- **WHEN** `VEN/src/entities/mod.rs` is read
- **THEN** it contains `pub mod timeline;`

### Requirement: assets/heater.rs imports HeaterPlanTrajectory from entities

`assets/heater.rs` SHALL import `HeaterPlanTrajectory` from `crate::entities::timeline`, not from `crate::controller::timeline`.

#### Scenario: heater uses entities timeline import
- **WHEN** `VEN/src/assets/heater.rs` is inspected
- **THEN** the import for `HeaterPlanTrajectory` reads `use crate::entities::timeline::HeaterPlanTrajectory`
- **THEN** there is no `use crate::controller::timeline` import at the module level

### Requirement: simulator imports timeline types from entities

`simulator/mod.rs` SHALL import `HeaterPlanTrajectory`, `TimelineAssetData`, `TimelinePoint`, `TimelineSnapshot` from `crate::entities::timeline`, not from `crate::controller::timeline`.

#### Scenario: simulator uses entities timeline imports
- **WHEN** `VEN/src/simulator/mod.rs` is inspected
- **THEN** all timeline type imports reference `crate::entities::timeline`
- **THEN** there is no `use crate::controller::timeline` import for data-carrier types

### Requirement: controller/timeline.rs imports data carriers from entities

`controller/timeline.rs` SHALL import `HeaterPlanTrajectory`, `TimelinePoint`, `TimelineAssetData`, `TimelineSnapshot`, `TimeWindow` from `crate::entities::timeline` and MAY re-export them for backward compatibility during migration. The orchestration functions (building snapshots from `SimState`) remain in `controller/timeline.rs`.

#### Scenario: controller timeline imports from entities
- **WHEN** `VEN/src/controller/timeline.rs` is inspected
- **THEN** it imports data-carrier types via `use crate::entities::timeline::{...}`
- **THEN** it does NOT re-define those structs locally

### Requirement: assets does not import from controller/timeline at module level

After the timeline migration, `assets/heater.rs` SHALL have no `use crate::controller::timeline` at the top-level module scope. (Test-module imports are excluded from this requirement.)

#### Scenario: assets heater has no controller timeline import
- **WHEN** `grep "use crate::controller::timeline" VEN/src/assets/heater.rs` is run excluding `#[cfg(test)]` blocks
- **THEN** it returns no matches

### Requirement: VEN compiles without errors after timeline migration

After completing the timeline migration, `wsl cargo check` in the VEN directory SHALL exit with code 0 and report zero errors.

#### Scenario: cargo check passes after track B
- **WHEN** `wsl cargo check` is executed in `VEN/`
- **THEN** exit code is 0 and stderr contains no `error[` lines

