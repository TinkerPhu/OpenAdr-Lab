# Data Model: Remove Profile from Routes Layer (AB-06)

**Feature**: `023-remove-profile-routes`

## Changed Entities

### `AppCtx` — `VEN/src/main.rs`

The shared application context struct gains one field and its `profile` field gains a doc-comment.

```rust
#[derive(Clone)]
pub struct AppCtx {
    pub state: AppState,
    pub vtn: VtnClient,
    pub metrics_handle: Arc<metrics_exporter_prometheus::PrometheusHandle>,
    pub trigger_tx: Arc<tokio::sync::watch::Sender<PlanTrigger>>,

    /// Retained for use by sim_tick task. Removal deferred to Phase 4/5.
    pub profile: Arc<Profile>,

    /// Pre-computed at startup from profile. Route handlers read this
    /// directly — no profile access required at request time.
    pub sim_schema: Arc<HashMap<String, Vec<ControlDescriptor>>>,

    pub sim: Arc<Mutex<SimState>>,
    pub active_objective: Arc<RwLock<PlannerObjective>>,
    pub planner_event_tx: PlannerEventTx,
}
```

**Lifecycle**: `sim_schema` is built once in `main` after `profile` is loaded, before the HTTP listener starts. It is read-only for the lifetime of the process.

**Concurrency**: `Arc` clone — safe for all concurrent request handlers.

---

## Changed Function Signatures

### `schema_from_profile` — `VEN/src/simulator/mod.rs`

| Property | Before | After |
|----------|--------|-------|
| Visibility | `pub(crate)` | `pub` |
| Signature | unchanged | unchanged |
| Return type | `HashMap<String, Vec<ControlDescriptor>>` | unchanged |

---

## No New Entities or Tables

This change does not introduce new domain entities, database tables, or persistent state. It is a pure refactoring of call-site and visibility.

---

## Test Fixtures

### `VEN/tests/fixtures/schema_snapshot.json`

A committed JSON snapshot of `schema_from_profile` output for the `ven-1.yaml` profile. Generated once (before the change) and checked in. The snapshot test asserts this exact output is produced after the change.

**Format** (illustrative):
```json
{
  "battery-1": [ { "id": "...", "name": "...", "type": "...", ... } ],
  "ev-1":      [ { ... } ],
  "pv-1":      [ { ... } ]
}
```
