## 1. Unused imports — cargo fix + manual re-export audit

- [x] 1.1 Run `wsl bash -c "cd /mnt/c/DriveD/Tinker/OpenAdr-Lab/VEN && ~/.cargo/bin/cargo fix --bin 'ven-app' 2>&1"` and review the diff before committing
- [x] 1.2 Audit `controller/mod.rs` line 7: remove `OadrEvent`, `OadrProgram`, `OadrReport` from the `pub use vtn_port::{...}` line — keep `VtnPort`
- [x] 1.3 Audit `entities/mod.rs`: remove `AbsorberAssetParams`, `AbsorberParams`, `PlannerParams`, `SimulatorParams`, `AssetParams` from re-exports — keep `PlannerObjective`
- [x] 1.4 Audit `services/mod.rs`: remove `CancelError`, `UserRequestService`, `EvSessionService`, `HvacService`, `PlanCycleResult`, `evaluate_acceptance_gate` from re-exports — keep `ObligationService`
- [x] 1.5 Run `wsl cargo check` and verify the 11 unused-import warnings are gone; fix any callers that break

## 2. Dead profile API — delete

- [ ] 2.1 Delete `Profile::load()` from `VEN/src/profile.rs` (main.rs uses `try_load()`)
- [ ] 2.2 Delete `Profile::ev_config()`, `heater_config()`, `pv_config()`, `battery_config()`, `base_load_kw()` from `profile.rs`
- [ ] 2.3 Delete `AssetProfile::id()` from `profile.rs`
- [ ] 2.4 Delete `PvConfig::forecast_kw()` from `profile.rs`
- [ ] 2.5 Run `wsl cargo check` — verify the 8 dead-profile-method warnings are gone and no compile errors

## 3. Dead utilities — delete

- [ ] 3.1 Remove `id: String` field from `Grid` struct in `VEN/src/assets/grid.rs` and remove it from the constructor/default if present
- [ ] 3.2 Delete `TariffSnapshot::is_empty()` from `VEN/src/entities/tariff_snapshot.rs`
- [ ] 3.3 Delete `solver_ms: u64` field from `PlanCycleResult` struct in `VEN/src/services/planning.rs` and remove any assignments to it
- [ ] 3.4 Delete `VtnClient::post_json()` from `VEN/src/vtn.rs`
- [ ] 3.5 Run `wsl cargo check` — verify the 4 dead-utility warnings are gone

## 4. Test support mocks — gate with #[cfg(test)]

- [ ] 4.1 In `VEN/src/services/mod.rs`, change `pub mod test_support;` to `#[cfg(test)] pub mod test_support;`
- [ ] 4.2 Run `wsl cargo check` — verify the ~12 mock dead-code warnings (MockVtn, MockSimulatorPort, MockBatteryCtx, MockEvCtx, MockHeaterCtx, their constructors and methods) are gone
- [ ] 4.3 Run `wsl bash -c "cd /mnt/c/DriveD/Tinker/OpenAdr-Lab/VEN && ~/.cargo/bin/cargo test 2>&1"` — verify all unit tests still pass (tests that import mock_vtn / mock_simulator_port are already inside `#[cfg(test)]` blocks)

## 5. Unfinished-feature dead code — annotate with #[allow(dead_code)]

- [ ] 5.1 Add `#[allow(dead_code)]` above `pub fn apply_battery_correction_overlay` in `VEN/src/controller/dispatcher.rs` with comment: `// Not yet wired into build_setpoints(); see design.md §Decision 4`
- [ ] 5.2 Add `#[allow(dead_code)]` above each of `deviation_threshold_kw`, `deviation_trigger_ticks`, `correction_min_kw` fields in `PlannerParams` in `VEN/src/entities/planner_params.rs` with comment referencing `apply_battery_correction_overlay`
- [ ] 5.3 Add `#[allow(dead_code)]` on `PacketSeed` struct and `ComfortRateSeed` struct in `VEN/src/profile.rs`, and on the `packets` field in `Profile`, with comment: `// Packet-based scheduling — not yet implemented`
- [ ] 5.4 Add `#[allow(dead_code)]` above `create_shiftable`, `cancel`, `is_shiftable` methods and the `CancelError` enum in `VEN/src/services/user_request.rs` with comment: `// Not yet wired to a route`
- [ ] 5.5 Run `wsl cargo check` — verify the unfinished-feature warnings are gone

## 6. EV session deletion bug — wire EvSessionService into route

- [ ] 6.1 In `VEN/src/routes/hems.rs`, add `use crate::services::hems::EvSessionService;` to the imports
- [ ] 6.2 Replace the body of `delete_ev_session` from `ctx.state.set_ev_session(None).await;` to `EvSessionService::end(&ctx.state).await.unwrap_or_default();` (the `unwrap_or_default` handles the None-session early return cleanly since `()` is the Ok type)
- [ ] 6.3 Run `wsl cargo check` — verify `EvSessionService` and `HvacService` dead-code warnings are gone (both structs are now used: EvSessionService in the route, HvacService still dead — handle in 6.4)
- [ ] 6.4 Check `HvacService::set_heater_target` and `clear_heater_target` — if still dead after 6.2, add `#[allow(dead_code)]` with comment `// Not yet wired to route; heater target is set directly in post_heater_target`
- [ ] 6.5 Run `wsl bash -c "cd /mnt/c/DriveD/Tinker/OpenAdr-Lab/VEN && ~/.cargo/bin/cargo test services::hems 2>&1"` — existing unit tests for EvSessionService must pass

## 7. Final verification

- [ ] 7.1 Run `wsl cargo check` — output must show 0 warnings
- [ ] 7.2 Run `wsl bash -c "cd /mnt/c/DriveD/Tinker/OpenAdr-Lab/VEN && ~/.cargo/bin/cargo test 2>&1"` — all unit tests must pass (HiGHS-dependent tests will be skipped; that is expected on WSL)
- [ ] 7.3 Verify the six architectural invariant greps from `docs/plans/ven_backend_architecture_refactoring_v2.md §3` still return empty (no regressions from this cleanup)
- [ ] 7.4 Commit with message: `fix(030): clear all compiler warnings; fix ev-session request completion`
