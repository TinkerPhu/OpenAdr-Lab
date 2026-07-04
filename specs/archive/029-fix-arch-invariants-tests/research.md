# Research: Fix Architecture Invariant Gaps and Missing Tests (029)

## Item 2 — ObligationService SimState dependency

**Decision**: Lift the sim lock + asset-sample extraction from `services/obligation.rs` into `tasks/obligation.rs`, passing `HashMap<String, Vec<AssetReportSample>>` to the service.

**Rationale**: The service currently locks `Arc<Mutex<SimState>>` internally (lines 28–49) to extract `(ts, power_kw, soc)` tuples. This is the same extraction pattern already applied to the reporter service in earlier phases. Moving the lock-acquire to the task layer decouples the service from the simulator infra type, matching the hexagonal rule that `services/` must not import `simulator` or `assets`.

**Concrete findings**:
- `services/obligation.rs` lines 8 and 89 both import `use crate::simulator::SimState`
- `AssetReportSample { ts, power_kw, soc }` is already defined and exported from `controller/reporter.rs`
- `tasks/obligation.rs` already holds `Arc<Mutex<SimState>>` as a parameter — adding the extraction there is a minimal change
- The service test module at line 89 also uses `SimState` to build `make_sim()` — this helper moves to the task-layer test and the service test no longer needs a simulator at all (it can pass `HashMap::new()` or similar)

**Alternatives considered**: Introducing a `SimSnapshot` port crossing — rejected as over-engineering; the pattern of passing `HashMap<id, Vec<Sample>>` is already established by the reporter path.

---

## Item 3 — tick_once unit test prerequisites

**Decision**: Add `#[derive(Default)]` to `AbsorberState`; construct `SimState` via `serde_json::from_value` in tests; create `tick_tests.rs` as a `#[cfg(test)] mod` declared inside `tick.rs`.

**Rationale**:
- `AbsorberState` (`controller/absorber.rs` line 70) has only `#[derive(Debug, Clone)]`. Its fields are `u32`, `bool`, `f64`, and `HashMap<String, _>` — all have natural zero defaults. Adding `#[derive(Default)]` is safe.
- `SimState` has no `Default` or test constructor, but it implements `Serialize + Deserialize`. The pattern used in `services/obligation.rs` tests (`serde_json::from_value(json!({...}))`) works for an empty sim with no assets.
- `PlannerEventTx = Arc<broadcast::Sender<PlannerEvent>>` — constructed with `Arc::new(broadcast::channel::<PlannerEvent>(1).0)`.
- `trigger_tx: Arc<watch::Sender<PlanTrigger>>` — constructed with `Arc::new(watch::channel(PlanTrigger::Periodic).0)`.
- Counters `persist_counter=0, persist_every_ticks=100, report_counter=0, report_every_ticks=100` prevent filesystem writes and VTN calls during the single-tick test.

**Concrete findings**:
- `tick.rs` is currently 193 lines; adding `#[cfg(test)] mod tick_tests;` brings it to 194 — within the 200-line limit.
- `AppState::new()` provides a zero-state AppState sufficient for one tick (all `.await` calls return defaults).
- The test must import `VtnPort` and pass `Arc<dyn VtnPort>` wrapping `MockVtn::new()`.

**Alternatives considered**: Mocking `super::helpers::*` and `super::publish::*` — rejected; these are free functions (no trait), and the serde construction approach avoids touching production code structure.

---

## Item 4 — spawn_planning smoke test prerequisites

**Decision**: Add a `#[cfg(test)] mod tests` block at the bottom of `tasks/planning.rs`; construct all parameters from existing defaults and `MockVtn`; abort the handle immediately to avoid the 5-second startup delay.

**Rationale**: `spawn_planning` starts with `tokio::time::sleep(5s)` before any MILP logic runs. Aborting the `JoinHandle` immediately after calling `spawn_planning` exercises the task construction and wire-up without waiting for any real work. This validates that `Arc<dyn VtnPort>` is accepted and that channel types are correctly wired.

**Concrete findings**:
- `PlannerParams` has `Default` ✓
- `PlannerObjective` has `#[derive(Default)]` → `PlannerObjective::MinCost` ✓
- `AssetParams` is an enum; `Vec::<AssetParams>::new()` is valid for an empty asset list ✓
- `active_objective: Arc<RwLock<PlannerObjective>>` — constructed with `Arc::new(RwLock::new(PlannerObjective::default()))` ✓
- `SimState` constructed via serde_json (same pattern as Items 3 and obligation test) ✓
- `MockVtn::new()` implements `VtnPort` fully ✓

**Alternatives considered**: Waiting for the first full plan cycle (requires real HiGHS) — rejected; the smoke test goal is wiring correctness, not solver validation.

---

## Item 5 — stale invariant path

**Decision**: In `docs/plans/ven_backend_architecture_refactoring.md` §8, change `VEN/src/controller/milp` → `VEN/src/controller/milp_planner` in the invariant grep command. Also update the surrounding descriptive text to clarify that `*Params` imports are permitted in `milp_planner` (physics structs); the invariant guards only against concrete asset types (`A_BAT`, `A_EV`, `A_HTR`).

**Rationale**: The directory `controller/milp` does not exist; it was renamed to `controller/milp_planner`. The grep currently silently returns empty, providing false assurance. The constitution (`CLAUDE.md` `ven-architecture` section) also references `controller/milp` — that path should be checked as a secondary fix during implementation.

**Concrete findings**:
- Actual directory: `VEN/src/controller/milp_planner/`
- Constitution `ven-architecture` in `.claude/CLAUDE.md` contains the same stale path at the `AssetMilpContext` invariant line — fix both.
- `docs/plans/ven_backend_architecture_refactoring.md` §8 contains the canonical grep — fix here first.

**No alternatives**: This is a one-line doc fix.

---

## BDD coverage decision

**Decision**: No new BDD scenarios required for items 2–5.

**Rationale**: Items 2–5 are internal architecture fixes (dependency boundary enforcement, unit tests, doc correction). They produce no new user-facing HTTP endpoints, UI behavior, or observable system state changes. The existing 44-feature BDD suite covers all VEN behavior. The success gate is that the suite continues to pass at 238 scenarios — not that new scenarios are added.
