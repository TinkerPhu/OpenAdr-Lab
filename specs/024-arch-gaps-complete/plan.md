# Implementation Plan: Complete VEN Architecture Gaps

**Branch**: `024-arch-gaps-complete` | **Date**: 2026-05-14 | **Spec**: [spec.md](spec.md)  
**Input**: Feature specification from `/specs/024-arch-gaps-complete/spec.md`

## Summary

Complete the three remaining gaps in the VEN backend architecture refactoring:

1. **Gap 3 — tick.rs line count** (standalone): Extract `build_absorber_params` helper from `tick.rs` to bring the file under the 200-line limit.
2. **Gap 2 — Typed VTN client** (standalone): Define `VtnPort` trait + minimal `OadrEvent`/`OadrProgram`/`OadrReport` structs; update `VtnClient` to implement the trait; cascade type changes through all consumers.
3. **Gap 1 — Application services layer** (depends on Gap 2): Extract business logic from `tasks/` and `routes/hems.rs` into four service structs in `services/`; route handlers become thin adapters.

No behavior changes in any gap. All existing tests must remain green after each gap.

---

## Technical Context

**Language/Version**: Rust stable 2021 edition  
**Primary Dependencies**: axum, tokio, serde_json, async_trait (new — required for `dyn VtnPort`)  
**Storage**: N/A — no persistence changes  
**Testing**: `cargo test` (unit + integration); BDD suite must stay green (no new scenarios needed)  
**Target Platform**: Linux ARM64 (Raspberry Pi 4), Docker  
**Project Type**: Backend service (VEN controller)  
**Performance Goals**: Unit tests complete in <1s; tick loop latency unchanged  
**Constraints**: No file > 500 lines; `tasks/` files < 200 lines; no new behavior  
**Scale/Scope**: VEN/src/ only — ~30 Rust source files affected across 3 gaps  

---

## Constitution Check

| Principle | Status | Notes |
|-----------|--------|-------|
| I — OpenADR Spec Fidelity | ✅ PASS | VTN struct field names preserve upstream names verbatim (`programID`, `eventName`, etc.) |
| II — BDD-First Testing | ✅ PASS | Pure structural refactoring — no new behavior, no new BDD scenarios required. Existing BDD suite must stay green. New test surface covered by `cargo test`. |
| III — Upstream Compatibility | ✅ N/A | VEN-side only; openleadr-rs submodule untouched |
| IV — Lean Architecture | ✅ PASS | Minimal struct fields only (clarified); stateless services; no retry in ObligationService; `async_trait` is the only new dependency, justified by `dyn VtnPort` need |
| V — Infrastructure Parity | ✅ PASS | Test on Pi4-Server via Docker as usual |
| VI — VEN Hexagonal Architecture | ✅ PASS | This feature IS the implementation of Phases 5+7; enforces Principle VI by closing the remaining invariant violations |

**Post-design re-check**: Required after data-model.md is written. All checks expected to pass — see data-model.md for cascade analysis.

---

## Project Structure

### Documentation (this feature)

```text
specs/024-arch-gaps-complete/
├── plan.md              ← this file
├── research.md          ← Phase 0 output
├── data-model.md        ← Phase 1 output
└── tasks.md             ← Phase 2 output (/speckit.tasks — not yet created)
```

### Source Code — files modified per gap

```text
VEN/Cargo.toml                           ← add async_trait = "0.1"

── Gap 3 (tick.rs fix) ──────────────────────────────────────────────────────
VEN/src/tasks/sim_tick/tick.rs           ← remove inline AbsorberParams, call helper
VEN/src/tasks/sim_tick/helpers.rs        ← add build_absorber_params(profile) fn

── Gap 2 (VTN typing) ───────────────────────────────────────────────────────
VEN/src/controller/vtn_port.rs           ← NEW: VtnPort trait + OadrEvent/Program/Report
VEN/src/controller/mod.rs                ← pub mod vtn_port
VEN/src/vtn.rs                           ← impl VtnPort for VtnClient
VEN/src/state.rs                         ← PollingState: Vec<Value> → typed
VEN/src/controller/openadr_interface.rs  ← parse_* fns take &[OadrEvent]
VEN/src/tasks/poll_events.rs             ← detect_event_changes takes &[OadrEvent]
VEN/src/controller/reporter.rs           ← event param → &OadrEvent
VEN/src/routes/events.rs                 ← serialize OadrEvent to JSON response
VEN/src/services/test_support/           ← add mock_vtn.rs + MockVtn impl

── Gap 1 (services) ─────────────────────────────────────────────────────────
VEN/src/services/mod.rs                  ← re-export all four services
VEN/src/services/obligation.rs           ← NEW: ObligationService
VEN/src/services/user_request.rs         ← NEW: UserRequestService
VEN/src/services/hems.rs                 ← NEW: HvacService, EvSessionService
VEN/src/services/planning.rs             ← NEW: PlanningService + evaluate_acceptance_gate
VEN/src/tasks/planning.rs                ← slim to ≤80 lines (orchestrator only)
VEN/src/tasks/obligation.rs              ← delegate to ObligationService
VEN/src/routes/hems.rs                   ← route handlers delegate to services
```

**Structure Decision**: Single Rust project (`VEN/src/`). No new modules or crates — only new
files within existing ring directories.

---

## Gap 3 — tick.rs line count (implement first)

**File**: `VEN/src/tasks/sim_tick/tick.rs` (208 lines → target ≤195)  
**File**: `VEN/src/tasks/sim_tick/helpers.rs` (add function)

### Change

Extract PHASE 3 absorber-params construction (lines 96–111 of tick.rs) into `helpers.rs`:

```rust
// helpers.rs — new function
pub(crate) fn build_absorber_params(profile: &Profile) -> AbsorberParams {
    AbsorberParams {
        enabled: profile.absorber.enabled,
        dead_band_kw: profile.absorber.dead_band_kw,
        dead_band_clearing_ticks: profile.absorber.dead_band_clearing_ticks,
        assets: profile.absorber.assets.iter().map(|a| AbsorberAssetParams {
            id: a.id.clone(),
            priority: a.priority,
            min_state_linger_s: a.min_state_linger_s,
            ev_departure_guard_s: a.ev_departure_guard_s,
        }).collect(),
    }
}
```

`tick.rs` PHASE 3 reduces to one call:
```rust
let absorber_params = super::helpers::build_absorber_params(&profile);
```

### Verification

```bash
wsl cargo check
# Then count lines:
(Get-Content VEN/src/tasks/sim_tick/tick.rs).Count
# Must be < 200
```

---

## Gap 2 — Typed VTN client

### Step 1 — Add async_trait dependency

`VEN/Cargo.toml`: add `async_trait = "0.1"`

### Step 2 — Define VtnPort and typed structs

**New file**: `VEN/src/controller/vtn_port.rs`

```rust
use async_trait::async_trait;
use anyhow::Result;
use serde::{Deserialize, Serialize};

#[async_trait]
pub trait VtnPort: Send + Sync {
    async fn fetch_programs(&self) -> Result<Vec<OadrProgram>>;
    async fn fetch_events(&self)   -> Result<Vec<OadrEvent>>;
    async fn fetch_reports(&self)  -> Result<Vec<OadrReport>>;
    async fn upsert_report(&self, body: serde_json::Value) -> Result<serde_json::Value>;
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OadrProgram {
    pub id: String,
    pub programName: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OadrEvent {
    pub id: String,
    pub programID: String,
    #[serde(default)]
    pub eventName: Option<String>,
    #[serde(default)]
    pub intervals: Vec<OadrInterval>,
    #[serde(default)]
    pub reportDescriptors: Option<Vec<OadrReportDescriptor>>,
}

// ... OadrInterval, OadrIntervalPeriod, OadrPayload, OadrReportDescriptor, OadrReport
// See data-model.md for full field inventory
```

`allow(non_snake_case)` is needed on struct fields — OpenADR uses camelCase field names
(e.g. `programID`, `eventName`) which Rust lints on. Apply `#[allow(non_snake_case)]` at
file level (or per struct) and document the reason.

Add `pub mod vtn_port;` to `VEN/src/controller/mod.rs`.

### Step 3 — Implement VtnPort for VtnClient

`VEN/src/vtn.rs`:
```rust
#[async_trait]
impl VtnPort for VtnClient {
    async fn fetch_programs(&self) -> Result<Vec<OadrProgram>> {
        let raw = self.get_json("/programs").await?;
        let items: Vec<OadrProgram> = serde_json::from_value(raw)?;
        Ok(items)
    }
    async fn fetch_events(&self) -> Result<Vec<OadrEvent>> { ... }
    async fn fetch_reports(&self) -> Result<Vec<OadrReport>> { ... }
    async fn upsert_report(&self, body: serde_json::Value) -> Result<serde_json::Value> {
        // delegate to existing upsert_report method body
    }
}
```

Keep the existing public method names as inherent methods (not trait impls) for backward
compatibility with callers that already have `VtnClient` concretely. Add the `VtnPort` impl
as an additional trait impl on the same struct.

### Step 4 — Update PollingState and AppState

`state.rs` `PollingState`:
```rust
pub programs: Vec<OadrProgram>,
pub events:   Vec<OadrEvent>,
pub reports:  Vec<OadrReport>,
```

Update `AppState::set_programs`, `set_events`, `set_reports`, `programs()`, `events()`,
`reports()` return types accordingly.

### Step 5 — Update consumers

Process in this order to minimize compile-error surface at each step:

1. `controller/openadr_interface.rs` — change `parse_rate_snapshots(events: &[Value], ...)` 
   and `parse_capacity_state(events: &[Value])` to `&[OadrEvent]`. Replace all `.get("field")`
   accesses with typed field access (`.intervals`, `.programID`, etc.).

2. `tasks/poll_events.rs` — `detect_event_changes(events: &[serde_json::Value], ...)` →
   `&[OadrEvent]`. The function maps over events calling `.get("id")` → use `.id` directly.
   `AppState::set_events(...)` now takes `Vec<OadrEvent>`.

3. `controller/reporter.rs` — event parameter `&serde_json::Value` → `&OadrEvent` in
   `build_measurement_report_for_obligation` and `build_status_report`.

4. `routes/events.rs` — `AppState::events()` returns `Vec<OadrEvent>`; serialize with
   `Json(events)` (implements `Serialize`).

5. `services/test_support/mock_vtn.rs` — new file, `MockVtn` implementing `VtnPort`.

### Step 6 — VtnPort contract tests

Add `#[cfg(test)]` block in `controller/vtn_port.rs`:
- Deserialize fixture JSON strings → `OadrEvent` (from real VTN response samples)
- Verify optional fields are `None` when absent
- Verify unknown fields do not panic

---

## Gap 1 — Application services layer

**Prerequisites**: Gap 2 complete (`VtnPort` exists).

### ObligationService (`services/obligation.rs`)

Extract inner logic from `tasks/obligation.rs` (lines 22–58). Service signature:

```rust
pub struct ObligationService;
impl ObligationService {
    pub async fn check_and_report(
        state: &AppState,
        sim: &Arc<Mutex<SimState>>,
        vtn: &dyn VtnPort,
        ven_name: &str,
        now: DateTime<Utc>,
    ) -> anyhow::Result<()>
}
```

`spawn_obligation_check` in `tasks/obligation.rs` becomes:
```rust
// 20-line loop: tick → call ObligationService::check_and_report → handle error
```

**Unit tests** (in `services/obligation.rs` `#[cfg(test)]`):
- `test_check_and_report_submits_due_obligation` — due obligation → MockVtn records call
- `test_check_and_report_skips_when_none_due`
- `test_check_and_report_propagates_vtn_error`

### UserRequestService (`services/user_request.rs`)

Extract from `routes/hems.rs` `post_requests` (lines 139–end) and `delete_request` handler.
The three creation paths (EV, Heater, ShiftableLoad) become separate methods.

Key business rules to extract:
- Shiftable-load fast-path (no sim-asset lookup needed)
- EV session creation and linking (`session_id`, `session_type`)
- Heater target creation and linking
- `cancel`: set status ABANDONED, clear linked EV/heater session

**Unit tests**:
- `test_create_ev_request` — verify session linked, trigger sent
- `test_create_shiftable_request` — verify ShiftableLoad + UserRequest created
- `test_cancel_clears_ev_session`
- `test_cancel_unknown_id_returns_err`
- `test_cancel_already_terminal_returns_err`
- `test_duplicate_ev_request_rejected`

### HvacService + EvSessionService (`services/hems.rs`)

Extract from `routes/hems.rs` EV unplug and heater clear handlers:
- `EvSessionService::end` — clears session, transitions linked request
- `HvacService::set_heater_target` / `clear_heater_target`

**Unit tests**:
- `test_ev_session_end_clears_session`
- `test_ev_session_end_transitions_linked_request`
- `test_heater_target_clear`

### PlanningService (`services/planning.rs`)

Extract `evaluate_acceptance_gate` as a pure function (the core testable logic):

```rust
pub fn evaluate_acceptance_gate(
    current: Option<&Plan>,
    new_plan: &Plan,
    trigger: &PlanTrigger,
    threshold_eur: f64,
    decay_s: f64,
    now: DateTime<Utc>,
) -> bool
```

`PlanningService::run_cycle` wraps this function plus the event emission and state mutation
currently in `tasks/planning.rs` lines 195–295.

`tasks/planning.rs` retains only: trigger channel setup, sleep/select loop, input assembly
(tariff snapshot, capacity, sim clone), and calls `PlanningService::run_cycle`. Target ≤80 lines.

**Unit tests**:
- `test_gate_rejects_below_threshold_periodic`
- `test_gate_accepts_on_deviation_trigger`
- `test_gate_accepts_when_no_current_plan`
- `test_gate_accepts_after_decay_window`
- `test_gate_accepts_epsilon_improvement`

### Route handler cleanup (`routes/hems.rs`)

After services are in place, update handlers:
- `post_requests` → call `UserRequestService::{create_ev, create_heater, create_shiftable}` based on body discriminant, then call state mutations and send trigger
- `delete_request` → call `UserRequestService::cancel`
- EV unplug handler → call `EvSessionService::end`
- Heater clear handler → call `HvacService::clear_heater_target`

Each handler becomes: parse body → call one service method → map to HTTP response.

### Update services/mod.rs

```rust
pub mod test_support;
pub mod obligation;
pub mod user_request;
pub mod hems;
pub mod planning;
```

---

## Verification

### After each gap

```bash
# Gap 3
(Get-Content VEN/src/tasks/sim_tick/tick.rs).Count   # must be < 200
wsl cargo test 2>&1 | tail -5

# Gap 2
wsl bash -c 'grep "serde_json::Value" VEN/src/vtn.rs'   # must be empty for public methods
wsl cargo test 2>&1 | tail -5

# Gap 1
wsl bash -c 'grep -r "use crate::profile" VEN/src/entities VEN/src/controller VEN/src/routes'  # empty
wsl cargo test 2>&1 | tail -5
```

### BDD (after all gaps)

```bash
ssh Pi4-Server "cd /srv/docker/openadr_lab && docker compose -f tests/docker-compose.test.yml run --build --rm test-runner"
```

All 3 invariants from CLAUDE.md must pass before PR to main.

---

## Complexity Tracking

No constitution violations — this feature closes existing violations, it does not introduce new ones.

| Item | Justified by |
|------|-------------|
| `async_trait` new dependency | `dyn VtnPort` requires runtime polymorphism over async methods; no Rust-stable alternative without equivalent complexity |
