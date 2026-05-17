# VEN Backend Architecture Refactoring Plan

**Status:** Draft — direction agreed, implementation not started  
**Date:** 2026-05-08  
**Scope:** `VEN/src/` — all Rust modules

---

## 1. Goal

Reshape the VEN backend toward **Hexagonal Architecture** (Ports & Adapters) with **Clean Architecture's use-case layer**. These two styles are complementary: Hexagonal defines the structural shape; Clean Architecture adds the dependency rule and the use-case ring that Hexagonal leaves open.

The target property: **the domain core (planning, dispatch, entity model) has zero imports from infrastructure** (simulator physics, VTN HTTP client, HiGHS solver, filesystem, YAML config). All traffic crosses a port (a Rust trait). All business rules live in a use-case or domain service, not in a state container or route handler.

---

## 2. Current Dependency Problems

The component diagram (`docs/architecture/ven_backend_components.md`) and review (`docs/architecture/ven_backend_review.md`) identify five structural breaches:

| ID | Breach | Symptom |
|----|--------|---------|
| AB-01 | `loops.rs` god module | ~19 outgoing deps, 7+ distinct concerns in one file; grows with every new feature |
| AB-02 | Both `milp_planner.rs` and `milp_interactions.rs` depend on concrete asset types (`A_BAT`, `A_EV`, `A_HTR`) — diagram edges: `C_MILP → A_BAT/A_EV/A_HTR` and `C_MILPI → A_BAT/A_EV/A_HTR` | Adding a new asset requires editing two planner files |
| AB-03 | `SimState` imported directly by controller and routes: `C_DISP`, `C_ABS`, `C_MILP`, `C_MON`, `C_ENV`, `R_SIM`, `R_TL` → `S_MOD` | Cannot test planning, envelope, or timeline logic without a live simulator |
| AB-04 | `PROFILE` imported by entities, assets, simulator, and controller | Domain is untestable without loading a YAML profile |
| AB-05 | Routes reach into `AppState` domain methods and internal modules directly | `R_HEMS → AppState`, `R_SIM → S_MOD`, `R_ASSET → S_MOD/A_MOD`, `R_TRACE → C_TRACE` — no service layer |
| AB-06 | `R_HEMS → PROFILE` — route handler imports raw YAML config directly | HTTP layer depends on infrastructure config; profile change requires route edits |

---

## 3. Target Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│  Driving Adapters                                                │
│  routes/ (axum handlers)    tasks/ (background loops)           │
└──────────────────────────┬──────────────────────────────────────┘
                           │ calls
┌──────────────────────────▼──────────────────────────────────────┐
│  Application Services (Use Cases)                    [NEW]       │
│  PlanningService   HvacService   HvacService                     │
│  ObligationService  UserRequestService                           │
└──────────────────────────┬──────────────────────────────────────┘
                           │ depends on (via traits)
┌──────────────────────────▼──────────────────────────────────────┐
│  Domain Core                                                      │
│  entities/   controller/  (pure Rust, no I/O, no config refs)   │
│  Port traits: SimulatorPort  SolverPort  VtnPort  StatePort      │
└──────┬──────────────────────────────────────┬───────────────────┘
       │ implemented by                        │ implemented by
┌──────▼──────────┐                  ┌─────────▼──────────────────┐
│ Driven Adapters │                  │ Driven Adapters             │
│ simulator/      │                  │ vtn.rs (VtnAdapter)         │
│ (SimulatorPort) │                  │ milp_solver/ (SolverPort)   │
│                 │                  │ state.rs (StatePort)         │
└─────────────────┘                  └────────────────────────────┘
```

**Dependency rule:** arrows point inward only. Inner rings never import outer rings.

### Port traits to introduce

| Port | Implemented by | Consumed by |
|------|---------------|-------------|
| `SimulatorPort` | `simulator/` | `controller/dispatcher`, `controller/absorber`, `controller/milp_planner` |
| `SolverPort` | `controller/milp/` (HiGHS) | `PlanningService` |
| `VtnPort` | `vtn.rs` → `VtnAdapter` | `ObligationService`, event poll tasks |
| `AssetMilpContext` trait | each asset in `assets/` | `milp_planner` (replaces direct `A_BAT`, `A_EV`, `A_HTR` imports) |

### Profile values — injected, not imported

`PROFILE` (raw YAML config) must not be imported by domain code. Instead, each domain object receives the values it needs as plain primitives at construction time:

```rust
// Before: milp_planner.rs imports Profile, reads AssetProfile variants inline
// After: BatteryParams constructed from Profile in the application service layer, injected into the planner
struct BatteryParams { capacity_kwh: f64, max_charge_kw: f64, ... }
```

The `Profile` struct remains in the infrastructure ring. The application service reads it at startup and constructs domain-level parameter structs.

---

## 4. Refactoring Phases

Each phase is independently shippable. All tests must remain green after each phase.

### Phase 1 — Split `loops.rs` into `tasks/` (AB-01)

Extract each `spawn_*` function to its own file. No logic changes.

```
VEN/src/tasks/
  mod.rs            — re-exports spawn_* fns
  event_poll.rs     — spawn_program_poll, spawn_event_poll, detect_event_changes
  sim_tick.rs       — spawn_sim_tick (physics phases only)
  planning.rs       — spawn_planning
  obligation.rs     — spawn_obligation_check
  reporter.rs       — spawn_report_poll (extracted from loops.rs → C_RPT2)
  persist.rs        — spawn_state_persist (extracted from loops.rs → S_PERSIST)
  envelope.rs       — spawn_envelope_update (extracted from loops.rs → C_ENV)
```

`sim_tick.rs` calls `absorber::apply(...)` and `controller::escalate_if_needed(...)` as explicit function calls rather than inline phases. This is the AB-03 prerequisite.

Note: the diagram shows `loops.rs` wires 19 outgoing dependencies — `C_RPT2`, `C_ENV`, `S_PERSIST`, `E_CAP`, `E_PLAN`, `E_TARIFF`, and `A_MOD` in addition to the original set. Each of these that does not belong in the orchestrator becomes an explicit call into a named function in the relevant task file.

Estimated effort: **1–2 days**

### Phase 2 — Introduce `SimulatorPort` trait (AB-03)

Define a trait in `controller/`:

```rust
pub trait SimulatorPort: Send + Sync {
    fn snapshot(&self) -> SimSnapshot;
    fn inject(&self, state: SimInjectState);
}
```

`SimState` in `simulator/mod.rs` implements `SimulatorPort`. All modules that currently import `S_MOD` directly must switch to `&dyn SimulatorPort`:

| Module | Current import | After Phase 2 |
|--------|---------------|---------------|
| `controller/dispatcher.rs` | `S_MOD` | `SimulatorPort` |
| `controller/absorber.rs` | `S_MOD` | `SimulatorPort` |
| `controller/milp_planner.rs` | `S_MOD` | `SimulatorPort` |
| `controller/monitor.rs` | `S_MOD` | `SimulatorPort` |
| `controller/envelope.rs` | `S_MOD` | `SimulatorPort` |
| `routes/sim.rs` | `S_MOD` | `SimulatorPort` |
| `routes/timeline.rs` | `S_MOD` | `SimulatorPort` |

Note: `AssetHistoryBuffer` has moved from `simulator/mod.rs` to `assets/mod.rs` — the `SimSnapshot` returned through this port does not carry history; history is a read-only query concern handled separately by the route layer via `assets/mod.rs`. The port trait stays clean.

New testable functions once this port exists: `dispatcher::build_setpoints()`, `dispatcher::apply_surplus_ev_overlay()`, `dispatcher::apply_battery_correction_overlay()`, `absorber::apply_deviation_absorption()`, `monitor::record_tick()`, `envelope::compute_envelope()`.

This makes the entire planning/dispatch path unit-testable with a mock simulator.

Estimated effort: **2–3 days**

### Phase 3 — Introduce `AssetMilpContext` trait (AB-02) ✅ COMPLETE

Replace the direct imports `C_MILP → A_BAT, A_EV, A_HTR` with a trait:

```rust
pub trait AssetMilpContext {
    fn asset_id(&self) -> &str;
    fn add_variables(&self, pool: &mut MilpVarPool);
    fn add_constraints(&self, pool: &MilpVarPool, problem: &mut Problem);
    fn extract_setpoint(&self, solution: &Solution) -> f64;
}
```

Each asset implements it. Both `milp_planner.rs` and `milp_interactions.rs` receive `Vec<Box<dyn AssetMilpContext>>` — both files currently import `A_BAT`, `A_EV`, `A_HTR` directly (diagram: `C_MILP → A_BAT/A_EV/A_HTR` and `C_MILPI → A_BAT/A_EV/A_HTR`). Adding a new asset type requires zero changes to either planner file after this phase.

`A_GRID` (grid.rs) does not get an `AssetMilpContext` implementation — the grid node is the import/export reference bus, not an optimisable asset. It has no decision variables. This is by design.

Simultaneously split `milp_planner.rs` into `controller/milp/` sub-modules (see AB-02 fix in the review doc).

**Completed as feature `020-milp-asset-port` (worktree `refactoring_phase_3`).** Invariant verification:
- `grep -r "use crate::assets::" VEN/src/controller/milp_planner` → 0 matches in production code ✓
- `impl.*AssetMilpContext` exists only in `assets/battery.rs`, `ev.rs`, `heater.rs` ✓
- `asset_port.rs` ≤ 500 lines ✓
- 350 cargo tests pass (including 5 new n=48 regression tests) ✓

### Phase 4 — Decouple `PROFILE` from domain (AB-04)

Introduce per-domain parameter structs assembled by the application layer:

```rust
// entities/battery_params.rs  (domain ring — no config imports)
pub struct BatteryParams { pub capacity_kwh: f64, pub max_charge_kw: f64, ... }

// In main.rs / application service (infrastructure ring)
let battery_params = BatteryParams::from_profile(&profile);
```

The diagram shows `PROFILE` flowing into all of these groups — each must be cleaned:

| Group | Files with PROFILE import |
|-------|--------------------------|
| `entities/` | `plan.rs` (`E_PLAN → PROFILE`) |
| `assets/` | `battery.rs`, `ev.rs`, `heater.rs`, `pv.rs`, `base_load.rs` (all → PROFILE) |
| `controller/` | `milp_planner.rs`, `absorber.rs` |
| `simulator/` | `mod.rs` (`S_MOD → PROFILE`), `persist.rs` (`S_PERSIST → PROFILE`) |

`routes/hems.rs` (`R_HEMS → PROFILE`) is addressed separately in Phase 6.

Remove all `use crate::profile::Profile` imports from all four groups above. Pass params by value at construction.

Estimated effort: **2–3 days**

### Phase 5 — Introduce application service layer (AB-05)

Add `VEN/src/services/` with one service per bounded subdomain:

```
VEN/src/services/
  mod.rs
  planning.rs        — PlanningService (trigger, adopt, query plan)
  user_request.rs    — UserRequestService (create, cancel, query)
  hems.rs            — HvacService, EvSessionService
  obligation.rs      — ObligationService (check obligations, report)
```

Route handlers call services. `AppState` becomes a pure state store — no business rules. Domain rules that currently live in `AppState::cancel_request()`, `AppState::start_ev_session()`, etc. move to the corresponding service.

The updated diagram makes the service boundaries explicit: `AppState` now contains named sub-structs `HemsState`, `ControllerSimState`, `PollingState`, and `EvSettings`. These map directly onto services:

| `AppState` sub-struct | Service |
|-----------------------|---------|
| `HemsState` | `UserRequestService` |
| `EvSettings` | `EvSessionService` |
| `ControllerSimState` | `PlanningService` |
| `PollingState` | `ObligationService` |

`R_HEMS` now covers 12 routes (`/plan`, `/tariffs`, `/capacity`, `/obligations`, `/ledger`, `/flexibility`, `/user-requests`, `/ev-session`, `/ev-settings`, `/heater-target`, `/shiftable-loads`, `/baseline-override`). Each route maps to exactly one service call — this is the design constraint that keeps the service layer thin.

`E_DS` has grown to include `ShiftableLoadRuntime` and `BaselineOverride`; `E_SITE` includes `SiteMeter`, `DispatchState`, `DeviceSession`. The `UserRequestService` and `HvacService` must account for these when moving business rules out of `AppState`.

Estimated effort: **3–5 days** (largest phase; schedule alongside a new HEMS feature, not as standalone cleanup)

> **Constraint — `tasks/sim_tick/tick.rs` line limit**: As of 2026-05, `tick.rs` is 193/200 lines (7 lines of headroom). Do not add logic to `tick.rs` during Phase 5. New tick-path behaviour must go into `helpers.rs` or a new `tasks/sim_tick/` sub-file. If `tick.rs` must grow, split it first.

### Phase 6 — Remove `PROFILE` from routes (AB-06)

`R_HEMS` currently imports `profile.rs` directly. Profile values needed by route handlers must be extracted into the application service layer (Phase 5) so that `routes/` has zero config imports.

After Phase 5, replace any `profile.xxx` reference in route handlers with a value read from the relevant service or injected as a typed parameter at router construction time.

Estimated effort: **0.5–1 day** (natural follow-on to Phase 5)

### Phase 7 — Type the VTN client

Replace `serde_json::Value` returns in `vtn.rs` with typed OpenADR 3 structs. Define `VtnPort` trait so the domain can call the VTN without knowing `reqwest` exists.

Estimated effort: **1–2 days**

---

## 5. What This Is Not

- **Not a rewrite.** No logic changes in any phase. Each phase is a structural extraction.
- **Not a single PR.** Each phase ships independently behind the same test suite.
- **Not a prerequisite for new features.** The refactoring proceeds opportunistically — Phase 3 (MILP decoupling) is natural when the next asset type is added; Phase 5 (services) is natural when the next HEMS command is added.
- **The component diagram is partially incomplete.** `R_EVT`, `R_RPT`, `R_ASSET`, `R_TRACE`, and `R_SYS` appear as nodes in the diagram but have no outgoing edges shown. Their actual dependencies (`R_ASSET → S_MOD/A_MOD`, `R_TRACE → C_TRACE`, etc.) were identified by reading the source. Before Phase 5, the diagram should be completed with these edges so the full route surface is visible.
- **Dead code is a separate concern.** The diagram flags `ASSET_BOILER` in `ids.rs` as suspected dead code. Remove it in a standalone commit before or after any phase — do not bundle into structural refactoring PRs where the diff is already large.

---

## 6. Testing Strategy

### Test pyramid

```
BDD / system tests (232 scenarios, ~50 min)   ← unchanged, full stack
──────────────────────────────────────────────
Integration tests  (routes/ HTTP roundtrips)  ← seconds, axum in-process
──────────────────────────────────────────────
Use case tests     (services/ + mock ports)   ← milliseconds, no network
──────────────────────────────────────────────
Domain tests       (entities/ + controller/)  ← milliseconds, pure Rust
```

The BDD suite is the safety net for all structural refactoring — it must stay green after every phase. The three lower layers are what refactoring *creates surface for*: each phase extracts named functions and introduces port traits, both of which make previously untestable code directly testable.

### Layer 1 — Domain tests

**Location:** `#[cfg(test)]` blocks inside `entities/` and `controller/` modules.  
**What they test:** pure transformations with no I/O. No profile YAML, no simulator, no network.  
**Current gap (from review):** `build_milp_inputs()`, `dispatcher::build_setpoints()`, `absorber::apply_deviation_absorption()`, `timeline` resampling, `translate_solution()` — all zero coverage today.

New test surface unlocked per phase:

| Phase | Functions that become testable |
|-------|-------------------------------|
| 1 — `tasks/` split | `detect_event_changes()` as a pure function; each tick phase (`tick_phase_absorber()`, `tick_phase_physics()`, `tick_phase_escalation()`) in isolation |
| 2 — `SimulatorPort` | `dispatcher::build_setpoints()`, `dispatcher::apply_surplus_ev_overlay()`, `dispatcher::apply_battery_correction_overlay()`, `absorber::apply_deviation_absorption()`, `monitor::record_tick()` — all accept `&dyn SimulatorPort` so a mock is enough |
| 3 — `AssetMilpContext` + milp/ split | `inputs::build_milp_inputs()`, `phase1::build_constraints()`, `phase2::build_constraints()`, `translate::translate_solution()` — each in its own file, each independently testable |
| 4 — Profile decoupled | All domain tests stop loading YAML; `BatteryParams::default()` replaces profile fixture wiring |

Convention: each file in `controller/milp/` and `controller/` gets a `#[cfg(test)]` section. Test functions are named `test_<function>_<scenario>`, e.g. `test_build_setpoints_ev_plugged`, `test_absorb_deviation_below_threshold`.

### Layer 2 — Use case tests

**Location:** `#[cfg(test)]` inside each `services/` module.  
**What they test:** orchestration logic — trigger conditions, error paths, state transitions — with all port traits replaced by manual mock structs.  
**Prerequisite:** Phase 2 (`SimulatorPort`) and Phase 5 (services).

Mock adapters live in a shared `VEN/src/services/test_support/` module (not `#[cfg(test)]` so they can be shared across service tests without duplication):

```rust
pub struct MockSimulator { pub snapshot: SimSnapshot }
impl SimulatorPort for MockSimulator {
    fn snapshot(&self) -> SimSnapshot { self.snapshot.clone() }
    fn inject(&self, _: SimInjectState) {}
}

pub struct MockSolver { pub result: Plan }
impl SolverPort for MockSolver {
    fn solve(&self, _: MilpInputs) -> Result<Plan> { Ok(self.result.clone()) }
}
```

New tests per service:

| Service | Key scenarios to test |
|---------|----------------------|
| `PlanningService` | trigger on deviation, skip when latch active, reject plan below adoption threshold, adopt plan on epsilon improvement |
| `UserRequestService` | create request, cancel clears linked EV session, reject duplicate, return ABANDONED on cancelled request query |
| `ObligationService` | due obligation triggers report, skip when no obligation, retry on VTN error |
| `HvacService` / `EvSessionService` | departure guard blocks absorber reduction, session cleared on unplug |

### Layer 3 — Adapter contract tests

**Location:** `#[cfg(test)]` inside each driven adapter module (`simulator/`, `vtn.rs`, `controller/milp/`).  
**What they test:** that the real implementation satisfies the port contract — not the business logic inside, just the interface behaviour.

| Adapter | Contract assertions |
|---------|-------------------|
| `SimState` → `SimulatorPort` | `snapshot()` after `inject()` reflects the injected value; `inject(reset)` restores defaults |
| `VtnAdapter` → `VtnPort` | typed structs deserialise from fixture JSON strings without panic; 401 triggers token refresh and retry |
| `MilpSolver` → `SolverPort` | a trivially feasible input returns a plan; an infeasible input returns `Err`, not a panic |

Contract tests run without network — `VtnAdapter` tests use `serde_json::from_str` on fixture strings, not a live HTTP call.

### Layer 4 — Integration tests

**Location:** `VEN/tests/*.rs` (Rust integration test directory, outside `src/`).  
**What they test:** a full HTTP roundtrip — request in, response out — with the real axum router wired up but driven adapters either real (simulator) or replaced by test doubles.  
**Tool:** `axum::Server` bound to a random port, or `tower::ServiceExt::oneshot()` for zero-port overhead.  
**Current gap:** route handlers have no tests at any level today.

Priority routes for first integration tests (drawn from the 12-route `R_HEMS` surface now visible in the diagram):

| Route | Scenario |
|-------|----------|
| `POST /user-requests` | returns 201 with created request; returns 400 on missing field |
| `DELETE /user-requests/:id` | returns 200 and status ABANDONED; returns 404 for unknown id |
| `GET /plan` | returns 200 with valid plan shape; returns 503 before first plan available |
| `GET /flexibility` | returns envelope with correct slot count |
| `GET /ev-settings` | returns current EV settings from `EvSettings` state |
| `PUT /baseline-override` | stores override; GET returns updated value |
| `GET /capability` | returns asset capability list from `A_MOD` |
| `GET /timeline/all` | returns uniform grid with correct slot count |
| `POST /sim/inject` | partial merge — absent field unchanged, null releases override |

### Mock placement rule

```
VEN/src/
  services/
    test_support/     ← shared mock impls, compiled in all builds (no cfg(test))
      mod.rs
      mock_simulator.rs
      mock_solver.rs
      mock_vtn.rs
```

Using `test_support/` (not `#[cfg(test)]`) avoids the Rust limitation where `#[cfg(test)]` types cannot be imported across module boundaries. The module is compiled in production builds but contains only zero-cost structs used exclusively in tests — acceptable tradeoff.

### Per-phase test obligations

Each refactoring phase must ship with tests, not just structural changes:

| Phase | Minimum test deliverable |
|-------|--------------------------|
| 1 — `tasks/` split | `detect_event_changes()` unit tests (pure function, no mock needed) |
| 2 — `SimulatorPort` | `dispatcher::build_setpoints()`, `apply_surplus_ev_overlay()`, `apply_battery_correction_overlay()`, `envelope::compute_envelope()` tests with `MockSimulator` |
| 3 — milp/ split | `build_milp_inputs()` tests; `translate_solution()` tests with synthetic HiGHS output |
| 4 — Profile decoupled | All existing domain tests remove profile fixture loading; at least one test per asset using direct `BatteryParams` / `EvParams` construction |
| 5 — Services | Full use case test suite per service (see Layer 2 table above); service boundary aligns with `HemsState` / `EvSettings` / `ControllerSimState` / `PollingState` sub-structs |
| 6 — Remove PROFILE from routes | Route-level test confirming no `profile::` import compiles in `routes/hems.rs` |
| 7 — Typed VTN | `VtnAdapter` contract tests with fixture JSON |

---

## 8. Success Criteria

| Criterion | How to verify |
|-----------|--------------|
| No `use crate::profile` in `entities/`, `controller/`, or `routes/` | `grep -r "use crate::profile" VEN/src/entities VEN/src/controller VEN/src/routes` returns empty |
| No concrete asset imports in `milp_planner` | `grep -r "use crate::assets::" VEN/src/controller/milp_planner` returns empty (note: `*Params` struct imports are permitted — the invariant guards only against concrete asset types `A_BAT`, `A_EV`, `A_HTR`) |
| No `serde_json::Value` in `vtn.rs` public returns | `grep "Value" VEN/src/vtn.rs` returns empty or only internal use |
| `SimulatorPort` mock exists and used in at least one unit test | `cargo test -p ven controller::` passes with mock impl |
| `loops.rs` replaced by `tasks/` with no file exceeding 200 lines | `wc -l VEN/src/tasks/*.rs` all under 200 |
| All BDD scenarios green after each phase | BDD test run passes |
| No new files exceed 500 lines | enforced in PR review |
