# Architecture Gap Analysis — speckit input

**Source:** `docs/plans/ven_backend_architecture_refactoring.md`  
**Date:** 2026-05-14  
**Purpose:** Input document for `speckit.specify` — describes what is missing or incomplete
across the 7-phase VEN architecture refactoring so a proper feature spec can be produced.

---

## Status overview

| Phase | Name | Status |
|-------|------|--------|
| 1 | `tasks/` split (AB-01) | COMPLETE |
| 2 | `SimulatorPort` trait (AB-03) | COMPLETE |
| 3 | `AssetMilpContext` trait (AB-02) | COMPLETE |
| 4 | `PROFILE` decoupled from domain (AB-04) | COMPLETE |
| 5 | Application services layer (AB-05) | PARTIAL |
| 6 | Remove `PROFILE` from routes (AB-06) | COMPLETE |
| 7 | Typed VTN client | NOT STARTED |

Additional issue: `tasks/sim_tick/tick.rs` is **208 lines** — exceeds the 200-line hard limit
defined in CLAUDE.md and the Phase 5 constraint note in the refactoring plan.

---

## Gap 1 — Phase 5: Application services layer not implemented (AB-05)

### What the plan requires

`VEN/src/services/` should contain four application service modules that extract business logic
from `tasks/` and `routes/`:

```
VEN/src/services/
  mod.rs            — re-exports each service
  planning.rs       — PlanningService (trigger, adopt, query plan, acceptance gate)
  user_request.rs   — UserRequestService (create, cancel, query, EV/heater linking)
  hems.rs           — HvacService, EvSessionService (setpoints, departure guard)
  obligation.rs     — ObligationService (check obligations, submit reports)
```

### What is actually present

`VEN/src/services/mod.rs` contains a single line:
```rust
pub mod test_support;
```

`services/test_support/` has:
- `mock_simulator_port.rs` — `MockSimulatorPort` struct implementing `SimulatorPort` (153 lines, 9 tests)
- `milp_mocks.rs` — MILP test fixtures

No application service files exist. The four service modules (`planning.rs`, `user_request.rs`,
`hems.rs`, `obligation.rs`) are missing entirely.

### Where the logic currently lives

| Service (target) | Current location | Key types / functions |
|-----------------|-----------------|----------------------|
| `PlanningService` | `tasks/planning.rs` (324 lines) | `spawn_planning(...)`, acceptance gate, plan adoption logic |
| `UserRequestService` | `controller/user_request.rs` | `CreateUserRequestBody`, `create_user_request(...)`, `RequestError` |
| `HvacService` / `EvSessionService` | `routes/hems.rs` (route handlers) | Session start/stop handlers, heater target handlers |
| `ObligationService` | `tasks/obligation.rs` | Obligation polling, report submission |

`AppState` already has the correct sub-struct decomposition for service boundaries:
- `HemsState` → `UserRequestService` and `HvacService`
- `EvSettings` → `EvSessionService`
- `ControllerSimState` → `PlanningService`
- `PollingState` → `ObligationService`

### What the routes currently do (breach)

`routes/hems.rs` handlers contain inline business logic that should move to services:
- `cancel_request` — validates state, calls `AppState` methods, enforces business rules
- `start_ev_session` — creates session, links user request, enforces departure guard
- EV unplug / heater target — state mutation with validation inline in the handler

After Phase 5, routes must only call services. `AppState` must become a pure state container.

### Required deliverables (Phase 5)

1. **`services/planning.rs`** — `PlanningService` struct:
   - `run_planning_cycle(...)` — encapsulates the plan trigger, solve, acceptance gate, and
     adoption logic currently in `tasks/planning.rs` lines 39–276.
   - Takes `Arc<dyn SimulatorPort>`, `Arc<dyn SolverPort>` (from Phase 2/3 ports).
   - Returns `PlanningResult { adopted: bool, plan: Plan, solver_ms: u64 }`.

2. **`services/user_request.rs`** — `UserRequestService` struct:
   - `create(body: CreateUserRequestBody, assets: &SimSnapshot) -> Result<UserRequest, RequestError>`
   - `cancel(id: Uuid, state: &AppState) -> Result<UserRequest>` — returns ABANDONED status
   - `get(id: Uuid, state: &AppState) -> Option<UserRequest>`
   - Moves business rules out of `controller/user_request.rs` and route handlers.

3. **`services/hems.rs`** — `HvacService` + `EvSessionService`:
   - `EvSessionService::start(session: EvSession, linked_request: Option<UserRequest>) -> Result<()>`
   - `EvSessionService::end(state: &AppState) -> Result<()>` — clears session, updates linked request
   - `HvacService::set_heater_target(target: HeaterTarget, state: &AppState) -> Result<()>`
   - `HvacService::clear_heater_target(state: &AppState)`

4. **`services/obligation.rs`** — `ObligationService` struct:
   - `check_and_report(obligations: &[OadrReportObligation], vtn: &dyn VtnPort, ...) -> Result<()>`
   - Extract from `tasks/obligation.rs`.

5. **`services/mod.rs`** — re-export all four services.

6. **Route handlers** updated to call services instead of inlining business logic.

7. **`tasks/planning.rs`** becomes a thin orchestrator: set up trigger channel, call
   `PlanningService::run_planning_cycle()` in a loop, handle the timer/channel select.
   Target: reduce from 324 lines to ≤ 80 lines.

### Required tests (Phase 5)

Per the architecture plan's test obligations, each service needs a use-case test suite
using the existing mock support in `services/test_support/`:

| Service | Minimum test scenarios |
|---------|----------------------|
| `PlanningService` | trigger on deviation, skip when latch active, reject plan below adoption threshold, adopt plan on epsilon improvement |
| `UserRequestService` | create request, cancel clears linked EV session, reject duplicate, return ABANDONED on cancelled request query |
| `ObligationService` | due obligation triggers report, skip when no obligation, retry on VTN error |
| `HvacService` / `EvSessionService` | departure guard blocks absorber reduction, session cleared on unplug |

---

## Gap 2 — Phase 7: Typed VTN client (not started)

### What the plan requires

Replace `serde_json::Value` in all public methods of `VEN/src/vtn.rs` with typed OpenADR 3
structs. Introduce a `VtnPort` trait so the domain can call the VTN without knowing `reqwest`
exists.

### What is actually present

`VEN/src/vtn.rs` (321 lines). Every public method returns `serde_json::Value`:

```rust
// Lines 256–319 — all public methods use serde_json::Value
pub async fn fetch_programs(&self) -> Result<Vec<serde_json::Value>>
pub async fn fetch_events(&self)   -> Result<Vec<serde_json::Value>>
pub async fn fetch_reports(&self)  -> Result<Vec<serde_json::Value>>
pub async fn submit_report(&self, body: serde_json::Value) -> Result<serde_json::Value>
pub async fn upsert_report(&self, body: serde_json::Value) -> Result<serde_json::Value>
pub async fn update_report(&self, id: &str, body: serde_json::Value) -> Result<serde_json::Value>
```

No `VtnPort` trait exists. Consumers (`tasks/poll_events.rs`, `tasks/obligation.rs`,
`tasks/planning.rs`, `controller/reporter.rs`) all receive and manipulate raw JSON, making
test isolation and type safety impossible at this layer.

CLAUDE.md invariant that **must pass** but currently fails:
```
grep "serde_json::Value" VEN/src/vtn.rs  → must be empty or internal only
```
Currently returns ~9 matches in public method signatures.

`PollingState` in `state.rs` mirrors this problem — it stores programs/events/reports as
`Vec<serde_json::Value>` (lines 108–112). This is the downstream effect of the untyped VTN.

### Required deliverables (Phase 7)

1. **OpenADR 3 typed structs** — new file `VEN/src/entities/openadr.rs` (or `entities/vtn_types.rs`):
   ```rust
   #[derive(Debug, Clone, Deserialize, Serialize)]
   pub struct OadrProgram { pub id: String, pub programName: String, /* ... */ }

   #[derive(Debug, Clone, Deserialize, Serialize)]
   pub struct OadrEvent { pub id: String, pub programID: String, pub eventName: Option<String>, /* ... */ }

   #[derive(Debug, Clone, Deserialize, Serialize)]
   pub struct OadrReport { pub id: String, pub reportName: String, /* ... */ }
   ```
   Field names must follow upstream OpenADR 3 spec names per the `dto` rule in CLAUDE.md
   (pass through upstream field names, no normalization).

2. **`VtnPort` trait** — new file `VEN/src/controller/vtn_port.rs` (or alongside `simulator_port.rs`):
   ```rust
   #[async_trait::async_trait]
   pub trait VtnPort: Send + Sync {
       async fn fetch_programs(&self) -> Result<Vec<OadrProgram>>;
       async fn fetch_events(&self)   -> Result<Vec<OadrEvent>>;
       async fn fetch_reports(&self)  -> Result<Vec<OadrReport>>;
       async fn upsert_report(&self, report: OadrReport) -> Result<OadrReport>;
   }
   ```

3. **`VtnClient` updated** to implement `VtnPort`. Internal methods (`get_json`, `post_json`, etc.)
   may remain as `serde_json::Value` since they are private. Only the public API surface changes.

4. **`PollingState`** in `state.rs` updated to use typed fields:
   ```rust
   pub programs: Vec<OadrProgram>,
   pub events:   Vec<OadrEvent>,
   pub reports:  Vec<OadrReport>,
   ```

5. **All consumers updated** — `tasks/poll_events.rs`, `tasks/obligation.rs`, `tasks/planning.rs`,
   `controller/reporter.rs`, `controller/openadr_interface.rs` — switch from `serde_json::Value`
   field access (`.get("id")`, `.as_str()`, etc.) to typed struct field access.

6. **`services/test_support/mock_vtn.rs`** — `MockVtn` struct implementing `VtnPort`
   (referenced in the architecture plan's mock placement rule, currently missing from
   `services/test_support/`).

### Required tests (Phase 7)

Adapter contract tests per architecture plan §6, Layer 3:

| Test | Assertion |
|------|-----------|
| `fetch_programs` with fixture JSON | typed structs deserialise without panic |
| `fetch_events` with fixture JSON | typed structs deserialise without panic |
| 401 handling | token refresh triggered, second request succeeds |
| `upsert_report` on 409 | `find_report_by_name` called, PUT issued |

Tests use `serde_json::from_str` on fixture strings — no live HTTP.

---

## Gap 3 — `tasks/sim_tick/tick.rs` line count violation

### What the rule requires

CLAUDE.md: *"tasks/ files must stay < 200 lines"*  
Architecture plan Phase 5 note: *"tick.rs is 193/200 lines (7 lines of headroom). Do not add
logic to tick.rs during Phase 5."*

### Current state

`VEN/src/tasks/sim_tick/tick.rs` is **208 lines** — 8 lines over the hard limit.

The file owns `tick_once(...)`, the full tick body with 8 numbered phases (PHASE 0 through
PHASE 8). The phases themselves are already partially extracted into helpers
(`super::helpers::apply_sim_injections`, `super::helpers::build_tick_setpoints`,
`super::helpers::finalize_tick_outputs`, `super::publish::publish_sim_tick_result`).

### What needs to happen

Extract the `profile.rs` dependency and any remaining inline phase blocks to bring `tick.rs`
back under 200 lines. Two options:

**Option A (preferred):** Move PHASE 3 absorber-params construction (lines 96–123 in tick.rs)
into a helper in `helpers.rs`:
```rust
// helpers.rs
pub(crate) fn build_absorber_params(profile: &Profile) -> AbsorberParams { ... }
```
Then `tick.rs` calls `super::helpers::build_absorber_params(&profile)` — removes ~14 lines from
tick.rs (lines 96–111 are the params struct literal, the rest is already a single function call).

**Option B:** Split PHASE 6 (`accumulate_deviation`, lines 181–189) and PHASE 7–8
(report/persist, lines 192–208) into a `post_tick.rs` called from `tick_once`.

Target after fix: `tick.rs` ≤ 195 lines.

---

## Dependency order for implementation

```
Gap 3 (tick.rs line count)     — standalone, can ship now
Gap 2 (Phase 7, VTN typing)    — standalone, no dependency on Gap 1
Gap 1 (Phase 5, services)      — depends on Phase 2 (SimulatorPort ✓) and Phase 3 (milp ✓)
                                  Gap 2 (VtnPort) should precede ObligationService
```

Recommended order: **Gap 3 → Gap 2 → Gap 1**.

---

## Invariant checklist (verify after each gap is closed)

```bash
# After Gap 1 (Phase 5)
grep -r "use crate::profile" VEN/src/entities VEN/src/controller VEN/src/routes
# → must be empty

# After Gap 2 (Phase 7)
grep "serde_json::Value" VEN/src/vtn.rs
# → must be empty (or internal private methods only)

# After Gap 3 (tick.rs split)
wc -l VEN/src/tasks/sim_tick/tick.rs
# → must be < 200
```
