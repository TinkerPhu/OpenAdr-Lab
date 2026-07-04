## 1. Track A — Move *Params structs to entities/

- [x] 1.1 Read `VEN/src/entities/asset_params.rs` and the five asset files (`assets/battery.rs`, `ev.rs`, `heater.rs`, `pv.rs`, `base_load.rs`) to record each `*Params` struct definition (fields + derives) and any non-`Asset` impl blocks on those structs.
- [x] 1.2 Move `BatteryParams` struct definition (fields + derives) from `assets/battery.rs` into `entities/asset_params.rs`, directly above the `AssetParams` enum. Remove the struct from `assets/battery.rs`; add `use crate::entities::asset_params::BatteryParams;` at the top of `assets/battery.rs`. Keep any `impl BatteryParams` methods in `assets/battery.rs` (move the `use` import in to the impl block if needed).
- [x] 1.3 Same as 1.2 for `EvParams` (and its nested type aliases/wrappers) from `assets/ev.rs` → `entities/asset_params.rs`.
- [x] 1.4 Same as 1.2 for `HeaterParams` from `assets/heater.rs` → `entities/asset_params.rs`.
- [x] 1.5 Same as 1.2 for `PvParams` from `assets/pv.rs` → `entities/asset_params.rs`.
- [x] 1.6 Same as 1.2 for `BaseLoadParams` from `assets/base_load.rs` → `entities/asset_params.rs`.
- [x] 1.7 Remove the `use crate::assets::{BatteryParams, EvParams, HeaterParams, PvParams, BaseLoadParams, ...}` import from `entities/asset_params.rs` (it now defines these structs directly). Verify the `AssetParams` enum variants still compile.
- [x] 1.8 Run `wsl cargo check` in `VEN/`; fix all errors before proceeding.

## 2. Track A — Remove milp_planner → assets/ imports

- [x] 2.1 In `controller/milp_planner/envelopes.rs`: replace `use crate::assets::{ev::EvParams, heater::HeaterParams}` with `use crate::entities::asset_params::{EvParams, HeaterParams}`.
- [x] 2.2 In `controller/milp_planner/inputs.rs`: replace `use crate::assets::{base_load::BaseLoadParams, pv::PvParams}` with `use crate::entities::asset_params::{BaseLoadParams, PvParams}`.
- [x] 2.3 In `controller/milp_planner/results.rs`: replace `use crate::assets::{battery::BatteryParams, ev::EvParams, heater::HeaterParams}` with `use crate::entities::asset_params::{BatteryParams, EvParams, HeaterParams}`.
- [x] 2.4 In `controller/milp_planner/mod.rs`: inspect the `use crate::assets::{...}` block and replace each `*Params` import with the equivalent `entities::asset_params` import. If any non-params types (e.g. `AssetConfig`) are imported from `assets/` in this file, leave those in place.
- [x] 2.5 Run `wsl cargo check`; confirm zero `error[` lines.

## 3. Track A — Remove pub-use re-exports of MILP types from assets/

- [x] 3.1 In `assets/battery.rs`: remove the line `pub use crate::controller::milp_planner::asset_port::{BatteryMilpContext, BatteryMilpVars, BatterySolOutput};`. Search for callers that used `crate::assets::battery::BatteryMilpContext` and update them to import directly from `crate::controller::milp_planner::asset_port`.
- [x] 3.2 In `assets/ev.rs`: remove the line `pub use crate::controller::milp_planner::asset_port::{EvMilpMode, EvMilpContext, EvMilpVars, EvSolOutput};`. Update callers accordingly.
- [x] 3.3 In `assets/heater.rs`: remove the line `pub use crate::controller::milp_planner::asset_port::{HeaterMilpMode, HeaterMilpContext, HeaterMilpVars, HeaterSolOutput};`. Update callers accordingly.
- [x] 3.4 Run `wsl cargo check`; confirm zero `error[` lines and zero new warnings related to unused imports.
- [x] 3.5 Verify invariant: `grep -r "use crate::assets::" VEN/src/controller/milp_planner/` (excluding `#[cfg(test)]` lines) returns no matches.
- [x] 3.6 Verify invariant: `grep -r "use crate::assets::" VEN/src/entities/` returns no matches.

## 4. Track B — Create entities/timeline.rs

- [x] 4.1 Read `VEN/src/controller/timeline.rs` in full. Identify the plain data-carrier structs (`HeaterPlanTrajectory`, `TimelinePoint`, `TimelineAssetData`, `TimelineSnapshot`, `TimeWindow`) vs. the orchestration functions (those that take `SimState` or `AppState` arguments).
- [x] 4.2 Create `VEN/src/entities/timeline.rs`. Copy the five data-carrier struct definitions (with all fields, derives, and any pure-data impl blocks) into it. Add the necessary `use` imports for types those structs reference from `entities/` or `std`/`chrono`/`serde`.
- [x] 4.3 Add `pub mod timeline;` to `VEN/src/entities/mod.rs` and ensure `pub use timeline::*` or individual re-exports are available under `crate::entities`.
- [x] 4.4 In `controller/timeline.rs`: add `use crate::entities::timeline::{HeaterPlanTrajectory, TimelinePoint, TimelineAssetData, TimelineSnapshot, TimeWindow};` and add `pub use crate::entities::timeline::{HeaterPlanTrajectory, TimelinePoint, TimelineAssetData, TimelineSnapshot, TimeWindow};` as a shim to keep existing callers in `tasks/` and `routes/` compiling. Remove the struct definitions that were moved.
- [x] 4.5 Run `wsl cargo check`; fix all errors.

## 5. Track B — Update assets/ and simulator/ to import from entities/timeline

- [x] 5.1 In `assets/heater.rs`: replace `use crate::controller::timeline::HeaterPlanTrajectory;` with `use crate::entities::timeline::HeaterPlanTrajectory;`.
- [x] 5.2 In `simulator/mod.rs`: replace `use crate::controller::timeline::{HeaterPlanTrajectory, TimelineAssetData, TimelinePoint, TimelineSnapshot};` with `use crate::entities::timeline::{HeaterPlanTrajectory, TimelineAssetData, TimelinePoint, TimelineSnapshot};`.
- [x] 5.3 Run `wsl cargo check`; confirm zero `error[` lines.
- [x] 5.4 Remove the re-export shim (`pub use crate::entities::timeline::*`) from `controller/timeline.rs` once all consumers outside of `controller/` have been updated.
- [x] 5.5 Run `wsl cargo check` final time for Track B; confirm zero errors.
- [x] 5.6 Verify: `grep "use crate::controller::timeline" VEN/src/assets/heater.rs` returns no matches at module level.
- [x] 5.7 Verify: `grep "use crate::controller::timeline" VEN/src/simulator/mod.rs` returns no matches.

## 6. Documentation and Cleanup

- [x] 6.1 In `VEN/src/controller/milp_planner/asset_port.rs`: update the comment at lines 14–15 to accurately reflect the current state (the invariant now holds; remove the false claim).
- [x] 6.2 In `CLAUDE.md` (root): add `assets/` to the Infra ring entry in the `ven-architecture` block. Update the verifiable invariant command for `use crate::assets::` to clarify it excludes `#[cfg(test)]` lines.
- [x] 6.3 Run the three architectural invariant checks from `CLAUDE.md` and confirm all pass: (a) `grep -r "use crate::profile" VEN/src/entities VEN/src/controller VEN/src/routes` → empty; (b) `grep -r "use crate::assets::" VEN/src/controller/milp_planner` (prod code) → empty; (c) `grep "serde_json::Value" VEN/src/vtn.rs` → internal only.
- [x] 6.4 Run `wsl cargo check` in `VEN/` for a final clean pass; confirm zero errors and zero new warnings.
- [x] 6.5 Update `docs/history/project_journal.md` with a summary of the arch-layer violations fixed, the approach taken, and any key learnings.

## 7. Deployment and Verification

- [ ] 7.1 SSH to Pi4-Server: `git pull` in `/srv/docker/openadr_lab`, then `docker compose up --build -d ven-ven-1-1 ven-ven-2-1 ven-ven-3-1` to rebuild and restart VEN containers.
- [ ] 7.2 Run the BDD cucumber suite on Pi4-Server and confirm all scenarios pass (zero failures).
- [x] 7.3 Commit with message `refactor(030): fix arch layer violations — move *Params and timeline types to entities`.
