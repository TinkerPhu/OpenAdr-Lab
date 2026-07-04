## Why

The VEN backend has five confirmed dependency-rule violations where Infra modules import Domain modules (and vice versa), including a bidirectional `assets/ ↔ controller/milp_planner/` cycle that directly contradicts a stated invariant (`"none in production code"`) in `asset_port.rs` — while four production files break it. The violations were discovered during a structured architectural review against the Hexagonal + Clean Architecture rules in `CLAUDE.md`.

## What Changes

- **Move asset physics-parameter structs to `entities/`**: `BatteryParams`, `EvParams`/`EvCharger`, `HeaterParams`, `PvParams`, `BaseLoadParams` migrate from `assets/<asset>.rs` into `entities/asset_params.rs` (they are already partially referenced there). This breaks the `milp_planner/ → assets/` cycle (violation ❸) and the `entities/ → assets/` import (violation ❶).
- **Remove `pub use` re-exports of MILP types from `assets/`**: `assets/battery.rs`, `ev.rs`, `heater.rs` re-export `*MilpContext` types from `controller/milp_planner/asset_port.rs`. These re-exports will be removed; consumers import from `milp_planner::asset_port` directly (violation ❷).
- **Move timeline data-carrier types to `entities/`**: `HeaterPlanTrajectory`, `TimelinePoint`, `TimelineAssetData`, `TimelineSnapshot`, `TimeWindow` currently live in `controller/timeline.rs` but are plain data structs with no controller logic. Moving them to `entities/` removes the `assets/ → controller/` (violation ❹) and `simulator/ → controller/` (violation ❺) imports.
- **Update ring-map documentation**: Add `assets/` to the Infra ring in `CLAUDE.md`. Fix the false comment in `asset_port.rs:14`.
- **No behaviour changes**: all changes are compile-time structural; the runtime behaviour of the VEN is identical.

## Capabilities

### New Capabilities

- `arch-params-in-entities`: Asset physics-parameter structs (`*Params`) live in `entities/` as pure data types; `assets/` and `milp_planner/` both import them from there. The `milp_planner/ → assets/` and `entities/ → assets/` imports are eliminated.
- `arch-timeline-in-entities`: Timeline data-carrier structs (`HeaterPlanTrajectory`, `TimelinePoint`, `TimelineAssetData`, `TimelineSnapshot`, `TimeWindow`) live in `entities/`; `controller/timeline.rs`, `simulator/`, and `assets/heater.rs` import them from there.

### Modified Capabilities

*(none — no requirement-level behaviour changes)*

## Impact

- **VEN service only** — no changes to VTN, BFF, VEN UI, VTN UI, or openleadr-rs.
- **No public HTTP API changes** — route signatures and JSON payloads are unchanged.
- **No persistence changes** — `state.json` format is unchanged.
- **Compiler-verified**: every import path change is caught at compile time; `wsl cargo check` is the acceptance gate.
- **No openleadr-rs change required.**

### Non-goals

- Refactoring `state.rs`, `profile.rs`, `common/`, or `models.rs` ring placement (unlabelled shared modules — tracked separately if desired).
- Changing the `serde_json::Value` private helper methods in `vtn.rs` (low-priority, separate concern).
- Any behaviour change to the MILP solver, simulator physics, or route handlers.
