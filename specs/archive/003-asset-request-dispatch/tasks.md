# Tasks: Asset Request Dispatch Refactor

**Input**: Design documents from `/specs/003-asset-request-dispatch/`
**Branch**: `003-asset-request-dispatch`

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (US1, US2, US3)

---

## Phase 1: Setup

No setup required — this is a pure refactor of an existing Rust service. No new dependencies, no new files, no project initialization needed.

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Add `resolve_request_target` to the inner asset structs and wire the dispatch method on `AssetState`. Must be complete before user_request.rs can be refactored.

**⚠️ CRITICAL**: T003 depends on T001 and T002. No user story work can begin until T003 is complete.

- [x] T001 [P] Add `resolve_request_target` to `EvCharger` in `VEN/src/simulator/assets/ev.rs`
- [x] T002 [P] Add `resolve_request_target` to `Battery` in `VEN/src/simulator/assets/battery.rs`
- [x] T003 Add `resolve_request_target` dispatch method to `AssetState` in `VEN/src/simulator/assets/mod.rs` (depends on T001, T002)

**Checkpoint**: `AssetState::resolve_request_target` compiles and handles all 5 variants. `cargo check` passes.

---

## Phase 3: User Story 1 — Submit Charging Request for Any Storage Asset (Priority: P1) 🎯 MVP

**Goal**: `user_request.rs` resolves charging targets by delegating to `AssetEntry.state.resolve_request_target` — no hardcoded switch, no profile import.

**Independent Test**: `POST /user-requests` for `asset_id: "ev"` with `target_soc: 0.9` returns 201 with correct `target_energy_kwh`. Same for `"battery"`.

- [x] T004 [US1] Refactor `resolve_target` in `VEN/src/controller/user_request.rs`: replace `match body.asset_id.as_str()` switch with lookup into `&[AssetEntry]` and call to `entry.state.resolve_request_target(...)` (depends on T003)
- [x] T005 [US1] Update `create_from_body` signature in `VEN/src/controller/user_request.rs`: replace `profile: &Profile, sim: Option<&SimSnapshot>` with `assets: &[AssetEntry]`; remove `use crate::profile::Profile` and `use crate::simulator::SimSnapshot` imports (depends on T004)
- [x] T006 [US1] Update `post_requests` handler in `VEN/src/main.rs`: replace `ctx.state.sim().await` + `&ctx.profile` arguments with `ctx.sim.lock().await.assets.clone()` passed as `&assets` (depends on T005)

**Checkpoint**: `cargo build` passes. `POST /user-requests` for `"ev"` and `"battery"` return 201. All existing `ven_user_request.feature` scenarios pass.

---

## Phase 4: User Story 2 — Reject Request for Non-Storage Asset (Priority: P2)

**Goal**: A request targeting `"pv"`, `"heater"`, or `"base_load"` returns a clear 422 error response.

**Independent Test**: `POST /user-requests` with `asset_id: "pv"` returns 422 with `"error"` field.

- [x] T007 [US2] Check `tests/features/ven_user_request.feature` for a scenario covering non-storage asset rejection; if absent, add scenario: `POST /user-requests` for asset `"pv"` with `target_soc: 0.9` expects status 422 and `"error"` field in response (depends on T005)

**Checkpoint**: New or existing non-storage rejection scenario passes in the BDD suite.

---

## Phase 5: User Story 3 — No Per-Asset Switch in Request Controller (Priority: P3)

**Goal**: Structural verification that `user_request.rs` contains no asset-type-specific branches or named profile accessors.

**Independent Test**: `grep -n "match body.asset_id\|ev_config\|battery_config\|use crate::profile\|SimSnapshot" VEN/src/controller/user_request.rs` → zero results.

- [x] T008 [US3] Run full BDD test suite on Pi4-Server and confirm zero regressions: `docker compose -f tests/docker-compose.test.yml run --build --rm test-runner` (depends on T007)
- [x] T009 [US3] Verify structural acceptance criteria: confirm `user_request.rs` contains no `match body.asset_id`, no `ev_config()`, no `battery_config()`, no `Profile` import, no `SimSnapshot` import (depends on T006)

**Checkpoint**: All 5 acceptance criteria from spec met. Full BDD suite green.

---

## Phase 6: Polish

- [x] T010 Update `docs/history/project_journal.md` with what was done, why, and any key learnings from this refactor

---

## Dependencies & Execution Order

### Phase Dependencies

- **Foundational (Phase 2)**: No dependencies — start immediately
  - T001 and T002 run in **parallel** (different files)
  - T003 depends on T001 + T002
- **User Story 1 (Phase 3)**: Depends on T003
  - T004 → T005 → T006 (sequential — each step depends on previous)
- **User Story 2 (Phase 4)**: Depends on T005
- **User Story 3 (Phase 5)**: T008 depends on T007; T009 depends on T006
- **Polish (Phase 6)**: Depends on all above

### Parallel Opportunities

```
T001 [ev.rs]       ─┐
                    ├─→ T003 [AssetState] → T004 → T005 → T006 (US1 done)
T002 [battery.rs]  ─┘                              ↓
                                                  T007 (US2)
                                                   ↓
                                                  T008 (full BDD)
                                                  T009 (grep verify)
```

---

## Implementation Strategy

### MVP (User Story 1 only — 6 tasks)

1. Complete T001 + T002 in parallel
2. Complete T003
3. Complete T004 → T005 → T006
4. Run `cargo build` and the existing `ven_user_request.feature` BDD scenarios
5. **Stop and validate**: EV and battery requests behave identically to before

### Full Delivery

After MVP: add T007 (non-storage rejection scenario), then T008 + T009 for full suite verification.

---

## Notes

- No new files are created in the source tree — all 5 changes are to existing files
- `resolve_request_target` on non-storage variants returns `None`; `user_request.rs` maps `None` to `RequestError::UnknownAsset` (not `ZeroEnergy` — per spec)
- `ZeroEnergy` is reserved for the case where a storage asset is already at target SoC (computed delta < 1e-6 kWh)
- Always pass `--build` to the test-runner compose command when any Rust source or `.feature` file changes
