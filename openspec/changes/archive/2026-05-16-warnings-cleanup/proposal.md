## Why

`cargo check` on VEN/src/ produces 42 warnings that accumulate noise in CI output, mask real signals, and — on close inspection — include one genuine behavioral bug: the `DELETE /ev-session` route silently skips transitioning linked UserRequests to Completed. All 42 warnings are addressable without changing any externally visible API behavior (except the bug fix).

## What Changes

- **Unused import cleanup**: Remove 11 unused `use`/`pub use` entries across `controller/mod.rs`, `entities/mod.rs`, `services/mod.rs`, and three other files. `cargo fix --bin "ven-app"` handles simple imports; re-exports are adjusted manually to preserve used symbols (`VtnPort`, `PlannerObjective`, `ObligationService`).
- **Dead profile API removal**: Delete 8 orphaned methods on `Profile`, `AssetProfile`, and `PvConfig` that were superseded when `build_domain_params()` was introduced. `Profile::load()` is replaced by `try_load()`; the typed-config accessors (`ev_config()`, `heater_config()`, etc.) are no longer called anywhere.
- **Dead utility removal**: Delete 4 items with no callers: `Grid::id` field, `TariffSnapshot::is_empty()`, `PlanCycleResult::solver_ms`, `VtnClient::post_json`.
- **Test support gating**: Add `#[cfg(test)]` to the `pub mod test_support` declaration in `services/mod.rs` so mock types (`MockVtn`, `MockSimulatorPort`, mock MILP contexts) only compile in test builds.
- **Unfinished-feature annotation**: Add `#[allow(dead_code)]` with explanatory comments to 5 items representing deliberately deferred features: `apply_battery_correction_overlay` (battery correction overlay — implemented and unit-tested but not yet wired into the dispatch loop), three `PlannerParams` deviation fields, `PacketSeed`/`ComfortRateSeed` (packet-scheduling stubs), and `UserRequestService::create_shiftable` / `cancel` / `is_shiftable` / `CancelError`.
- **Bug fix — EV session deletion**: Update `routes/hems.rs::delete_ev_session` to call `EvSessionService::end(&ctx.state)` instead of `ctx.state.set_ev_session(None)` directly. This ensures linked UserRequests are transitioned to `Completed` when a session ends — behavior that `EvSessionService::end()` implements and its unit tests already verify.

## Capabilities

### New Capabilities

- `ev-session-request-completion`: When an EV session is deleted, any UserRequest linked to that session (status: Active) is automatically transitioned to Completed.

### Modified Capabilities

*(None — no existing spec-level requirement changes.)*

## Impact

- **VEN binary only** (`VEN/src/`). No BFF, VTN, UI, or database changes.
- **No API surface changes** except the behavioral fix: `DELETE /ev-session` now returns the same HTTP response but also completes linked requests in state.
- **No openleadr-rs changes** required.
- **No persistence schema changes** — state.json format is unchanged.
- **Non-goals**: This change does not wire `apply_battery_correction_overlay` into the dispatch loop (that is a separate feature). It does not implement packet-based scheduling. It does not address VG-01 through VG-07 architecture violations (those are tracked in `docs/plans/ven_backend_architecture_refactoring_v2.md`).
