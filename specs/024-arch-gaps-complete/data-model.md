# Data Model: 024-arch-gaps-complete

## New types

### VtnPort + typed VTN structs (`controller/vtn_port.rs`)

These are the port trait and its data types. Field names preserve the OpenADR 3 convention
verbatim per Constitution Principle I.

```
VtnPort (trait)
  fetch_programs() -> Result<Vec<OadrProgram>>
  fetch_events()   -> Result<Vec<OadrEvent>>
  fetch_reports()  -> Result<Vec<OadrReport>>
  upsert_report(OadrReport) -> Result<OadrReport>

OadrProgram
  id: String
  programName: String

OadrEvent
  id: String
  programID: String
  eventName: Option<String>
  intervals: Vec<OadrInterval>
  reportDescriptors: Option<Vec<OadrReportDescriptor>>

OadrInterval
  intervalPeriod: Option<OadrIntervalPeriod>
  payloads: Vec<OadrPayload>

OadrIntervalPeriod
  start: Option<String>      ← ISO 8601 datetime string
  duration: Option<String>   ← ISO 8601 duration string (e.g. "PT1H")

OadrPayload
  type: String               ← e.g. "PRICE", "EXPORT_PRICE", "GHG", "MAX_POWER"
  values: Vec<serde_json::Value>  ← mixed numeric/string per payload type; internal use only

OadrReportDescriptor
  payloadType: String
  readingType: Option<String>
  frequency: Option<String>

OadrReport
  id: String
  reportName: String
  ── all other report fields remain serde_json::Value in the submit/upsert body ──
  ── only id + reportName need typed access; full report body stays as Value ──
```

**Note on OadrReport**: `VtnClient::upsert_report` and `submit_report` take a full report
body built by `controller/reporter.rs` which constructs `serde_json::json!{...}` literals.
Typing the full report body would require typing all reporter output structs — out of scope.
The `VtnPort::upsert_report` accepts `serde_json::Value` for the body, and returns a typed
`OadrReport` only for the `find_report_by_name` pattern (id + reportName lookup).

**Revised VtnPort signature for upsert_report**:
```
upsert_report(body: serde_json::Value) -> Result<serde_json::Value>
fetch_reports() -> Result<Vec<OadrReport>>   ← typed for id/reportName lookup
```
This keeps the minimally-typed approach consistent — only type what's actually accessed by field.

---

### Service types (`services/`)

```
ObligationService (zero-field unit struct)
  check_and_report(
    state: &AppState,
    sim: &Arc<Mutex<SimState>>,
    vtn: &dyn VtnPort,
    ven_name: &str,
    now: DateTime<Utc>,
  ) -> Result<()>
  ── no internal state; stateless function in struct form ──

UserRequestService (zero-field unit struct)
  create_ev(body, assets, asset_configs, now) -> Result<(UserRequest, EvSession), RequestError>
  create_heater(body, assets, asset_configs, now) -> Result<(UserRequest, HeaterTarget), RequestError>
  create_shiftable(body, now) -> Result<(UserRequest, ShiftableLoad), RequestError>
  cancel(id, state) -> Result<UserRequest>  ← sets ABANDONED, clears linked session

EvSessionService (zero-field unit struct)
  start(session, linked_request_id, state) -> Result<()>
  end(state) -> Result<()>  ← clears session, transitions linked request if any

HvacService (zero-field unit struct)
  set_heater_target(target, state) -> Result<()>
  clear_heater_target(state)

PlanningService (zero-field unit struct)
  run_cycle(
    inputs: PlanCycleInputs,
    solver_fn: F,   ← closure wrapping controller::milp_planner::run_planner
    state: &AppState,
    trigger: PlanTrigger,
    event_tx: &PlannerEventTx,
    now: DateTime<Utc>,
  ) -> PlanCycleResult

PlanCycleInputs
  tariff_ts: TariffTimeSeries
  capacity: OadrCapacityState
  sim_snap: SimSnapshot
  asset_contexts: Vec<Box<dyn AssetMilpContext>>
  planner: PlannerParams
  grid_max_import_kw: f64
  grid_max_export_kw: f64
  asset_params: Vec<AssetParams>
  ev_sess: Option<EvSession>
  heat_tgt: Option<HeaterTarget>
  shift_loads: Vec<ShiftableLoad>
  bl_override: Option<BaselineOverride>
  objective: PlannerObjective
  pv_forecast_override: Option<f64>

PlanCycleResult
  adopted: bool
  plan: Plan
  solver_ms: u64
```

**Note on PlanningService**: The acceptance gate logic (threshold, decay, elapsed time) is
extracted as a pure function `evaluate_acceptance_gate(current: Option<&Plan>, new_plan: &Plan, trigger, planner_params, now) -> bool` in `services/planning.rs`. This is the primary testable unit.

---

### Mock adapters (`services/test_support/`)

```
MockVtn   (new)
  Implements VtnPort
  programs: Vec<OadrProgram>
  events: Vec<OadrEvent>
  reports: Vec<OadrReport>
  submitted_reports: Arc<Mutex<Vec<serde_json::Value>>>  ← captures upsert calls for assertions

MockSimulatorPort  (existing, unchanged)
  Implements SimulatorPort
```

---

## State changes

### `state.rs` — PollingState

```
Before:
  programs: Vec<serde_json::Value>
  events:   Vec<serde_json::Value>
  reports:  Vec<serde_json::Value>

After:
  programs: Vec<OadrProgram>
  events:   Vec<OadrEvent>
  reports:  Vec<OadrReport>
```

All `AppState` methods that read/write `PollingState` fields update their return types accordingly.

---

## Cascade: consumers of PollingState that change type

| File | Change |
|------|--------|
| `tasks/poll_events.rs` | `detect_event_changes` takes `&[OadrEvent]`; field accesses → typed |
| `controller/openadr_interface.rs` | `parse_rate_snapshots`, `parse_capacity_state` take `&[OadrEvent]` |
| `controller/reporter.rs` | Event param changes to `&OadrEvent` where typed |
| `routes/events.rs` | `AppState::events()` returns `Vec<OadrEvent>` — serialize to JSON for response |
| `tasks/obligation.rs` | Uses `VtnClient` directly → after Gap 1 delegates to `ObligationService` |

---

## No changes to

- `entities/` — no new domain types
- `controller/simulator_port.rs` — unchanged
- `controller/milp_planner/` — unchanged
- `assets/` — unchanged
- `simulator/` — unchanged
- Database / persistence — unchanged
