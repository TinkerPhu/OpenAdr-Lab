## Context

The VEN backend follows Hexagonal + Clean Architecture with four rings (outer→inner): Adapters (`routes/`, `tasks/`) → Application (`services/`) → Domain (`entities/`, `controller/`) → Infra (`simulator/`, `vtn.rs`, `controller/milp_planner/`). An architectural review identified five confirmed violations:

| # | Violation | Root cause |
|---|---|---|
| ❶ | `entities/asset_params.rs` → `assets/` | `AssetParams` enum wraps concrete `*Params` structs defined in `assets/` |
| ❷ | `assets/battery·ev·heater` → `controller/milp_planner` | `pub use` re-exports of MILP types smuggle in an Infra→Domain import |
| ❸ | `milp_planner/envelopes·inputs·results·mod` → `assets/` | Direct imports of `*Params` from `assets/` — stated invariant broken |
| ❹ | `assets/heater.rs` → `controller/timeline` | `HeaterPlanTrajectory` lives in controller but heater physics needs it |
| ❺ | `simulator/mod.rs` → `controller/timeline` | `TimelinePoint/Snapshot` live in controller but simulator produces them |

Violations ❶–❸ share one root: `*Params` structs are physically in `assets/` even though they are pure data (no physics logic, no `impl Asset`). Violations ❹–❺ share a second root: timeline data-carrier structs live in `controller/timeline.rs` even though they are plain data.

## Goals / Non-Goals

**Goals:**
- Move asset physics-parameter structs to `entities/` so both `assets/` and `milp_planner/` import from Domain — breaking the `assets/ ↔ milp_planner/` cycle.
- Move timeline data-carrier types to `entities/` so `assets/heater.rs` and `simulator/` import from Domain without going through `controller/`.
- Remove `pub use` re-exports of MILP types from `assets/`.
- Fix the false comment in `asset_port.rs` and update the `CLAUDE.md` ring map.
- `wsl cargo check` passes with zero errors and zero new warnings after each task.

**Non-Goals:**
- Refactoring `state.rs`, `profile.rs`, `common/`, `models.rs` ring placement.
- Fixing `serde_json::Value` in `vtn.rs` private helpers.
- Any behaviour change to MILP solver, physics, or HTTP routes.

## Decisions

### D1 — Where to place `*Params` structs

**Options considered:**
1. Create a new `entities/asset_physics_params.rs` file for all params — clean separation but adds a new file.
2. Inline into the existing `entities/asset_params.rs` — keeps all asset-param types together in one file; `AssetParams` enum and the concrete param structs already conceptually belong together.
3. Keep in `assets/` and fix consumers — does not solve the dependency direction.

**Decision: Option 2.** `entities/asset_params.rs` already re-exports types from `assets/`; replacing those with the actual struct definitions removes the import entirely. The file will hold `BatteryParams`, `EvParams`/`EvCharger`, `HeaterParams`, `PvParams`/`PvInverter`, `BaseLoadParams`, `GridConfig`, and the `AssetParams`/`AssetRequestSlice` enums — all pure data, no `impl Asset`.

After the move `assets/battery.rs` etc. import their own params from `entities::asset_params`, not from `assets::<self>`. This is a small intra-infra re-direction that is fine (Infra → Domain is allowed).

### D2 — Where to place timeline data-carrier types

**Options considered:**
1. Move to `entities/timeline.rs` (new file).
2. Move to `entities/plan.rs` (plan is already related to timeline/trajectory).
3. Keep a thin `controller/timeline.rs` for controller-specific orchestration; move plain structs to `entities/`.

**Decision: Option 3.** `controller/timeline.rs` contains both plain data structs (`TimelinePoint`, `TimelineAssetData`, `TimelineSnapshot`, `HeaterPlanTrajectory`, `TimeWindow`) and controller-level logic (building timeline snapshots from `SimState`). We move only the plain data types to a new `entities/timeline.rs`. `controller/timeline.rs` becomes a thin orchestration file that imports from `entities/timeline`.

This keeps `controller/timeline.rs` small and removes the upward dependency from `simulator/` and `assets/heater.rs` into `controller/`.

### D3 — Migration order to maintain compile-ability at each step

The two fix tracks are independent of each other and can be executed in either order, but within each track the order matters to keep the project compiling:

**Track A (params):**
1. Define `*Params` structs in `entities/asset_params.rs`.
2. Update `assets/<asset>.rs` to remove own struct definitions and import from `entities`.
3. Remove `pub use` re-exports of MILP types from `assets/<asset>.rs`.
4. Update all `milp_planner/` files to import `*Params` from `entities` instead of `assets`.
5. Run `wsl cargo check`; fix any remaining import cascades.

**Track B (timeline):**
1. Define data-carrier types in `entities/timeline.rs`.
2. Update `controller/timeline.rs` to import + re-export from `entities/timeline` (to keep existing callers compiling without mass-change).
3. Update `assets/heater.rs` and `simulator/mod.rs` to import from `entities/timeline`.
4. Remove the re-export shim from `controller/timeline.rs` once all callers are updated.
5. Run `wsl cargo check`.

### D4 — Re-export shim in `controller/timeline.rs`

A temporary `pub use entities::timeline::*` re-export in `controller/timeline.rs` allows existing callers in `tasks/`, `routes/`, and `services/` to continue compiling without change during the migration. The shim is removed once `assets/heater.rs` and `simulator/mod.rs` are updated to import directly from `entities/timeline`.

## Risks / Trade-offs

- **File size**: `entities/asset_params.rs` will grow to hold all `*Params` structs (currently ~50–80 lines each × 5 assets = ~300 extra lines). At ~400 total lines it stays under the 500-line limit.
- **`entities/timeline.rs`** is a new file under 200 lines — well within limits.
- **Circular risk**: if any `entities/` type tries to import from `assets/` during the move, the cycle is re-introduced. Mitigation: verify `entities/` has no `use crate::assets::` at each step.
- **Test suite**: unit tests inside `assets/<asset>.rs` that import from `controller::milp_planner` remain valid (test code is allowed to cross ring boundaries); they need no change unless the types move.
- **No runtime risk**: this is purely a compile-time structural change — no serialisation formats, no HTTP payloads, no state file schemas are touched.

## Migration Plan

1. Execute Track A tasks (`*Params` migration) — `wsl cargo check` must pass before Track B starts.
2. Execute Track B tasks (timeline migration) — `wsl cargo check` must pass before docs step.
3. Update `CLAUDE.md` ring map (add `assets/` to Infra; update invariant command).
4. Fix false comment in `controller/milp_planner/asset_port.rs:14`.
5. Run `wsl cargo check` final pass; commit.
6. Deploy on Pi4-Server and run BDD suite.

**Rollback**: entire change is in VEN Rust source only; `git revert` or branch switch restores previous state instantly. No DB or file-format changes to undo.

## Open Questions

*(none — scope is fully bounded by the five confirmed violations)*
