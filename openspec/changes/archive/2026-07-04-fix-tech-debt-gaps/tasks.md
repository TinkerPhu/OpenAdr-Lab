## 1. Domain Errors (`entities/error.rs`)

- [x] 1.1 Create `VEN/src/entities/error.rs` with `DomainError` enum using `thiserror`: variants `SessionConflict(String)`, `NotFound { id: Uuid }`, `PlanInfeasible(String)`, `VtnUnreachable(String)`, `ProfileInvalid(String)`
- [x] 1.2 Re-export `DomainError` from `VEN/src/entities/mod.rs`
- [x] 1.3 Update `services/hems.rs` EV session start/stop to return `Result<_, DomainError>` for `SessionConflict` and `NotFound` cases
- [x] 1.4 Update `services/user_request.rs` cancel/get to return `Result<_, DomainError>` for `NotFound` cases
- [x] 1.5 Update route handlers in `routes/hems.rs` to map `DomainError` variants to HTTP status codes (409 for `SessionConflict`, 404 for `NotFound`)
- [x] 1.6 Add unit tests: each `DomainError` variant has a non-empty `Display` output; `SessionConflict` maps to 409; `NotFound` maps to 404
- [x] 1.7 Run `wsl cargo check` — zero errors

## 2. Profile Validation (`profile.rs`)

- [x] 2.1 Add `Profile::validate(&self) -> Result<(), Vec<String>>` to `VEN/src/profile.rs`
- [x] 2.2 Implement check: all absorber asset IDs exist in `assets:`
- [x] 2.3 Implement checks: `ev.soc_target` ∈ [0.0, 1.0], `ev.max_discharge_kw` ≥ 0.0
- [x] 2.4 Implement checks: `battery.min_soc` ∈ [0.0, 1.0), `battery.round_trip_efficiency` ∈ (0.0, 1.0]
- [x] 2.5 Implement checks: `planner.replan_interval_s` > 0, `planner.phase2_epsilon_eur` ≥ 0.0, `absorber.dead_band_kw` ≥ 0.0
- [x] 2.6 Implement check: at least one asset declared
- [x] 2.7 Call `profile.validate()` in `main.rs` after `Profile::load()`; on `Err(violations)` print all violations to stderr and call `std::process::exit(1)`
- [x] 2.8 Add unit tests covering: valid profile passes, absorber unknown asset fails, `soc_target = 1.5` fails, `round_trip_efficiency = 0.0` fails, empty assets fails, multiple violations reported together
- [x] 2.9 Run `wsl cargo test` — all tests pass (also fixed pre-existing milp_planner test compile errors: wrong import paths for `BaseLoadParams`, `BatteryParams`, etc.)

## 3. Named Constants and Profile Config for Magic Numbers

- [x] 3.1 Add `solver_timeout_s: u64` (default `60`) and `planning_initial_delay_s: u64` (default `5`) to `PlannerConfig` in `profile.rs` with `#[serde(default)]`
- [x] 3.2 Add both fields to `PlannerParams` in `entities/planner_params.rs` and propagate from `main.rs`
- [x] 3.3 Replace `with_time_limit(60.0)` in `milp_planner/solver_phase1.rs` and `solver_phase2.rs` with `planner.solver_timeout_s as f64`
- [x] 3.4 Replace `from_secs(5)` initial delay in `tasks/planning.rs` with `planner.planning_initial_delay_s`
- [x] 3.5 Add `const OBLIGATION_CHECK_INTERVAL_S: u64 = 5;` to `tasks/obligation.rs` and use it in place of the bare literal
- [x] 3.6 Add `const TOKEN_EXPIRY_MARGIN_S: u64 = 60;` to `vtn.rs` and use it in place of the bare literal
- [x] 3.7 Update `ven-1.yaml`, `ven-2.yaml`, `ven-3.yaml` profiles to include `solver_timeout_s: 60` and `planning_initial_delay_s: 5` (explicit defaults, no behaviour change)
- [x] 3.8 Run `wsl cargo check` — zero errors

## 4. Task Supervisor (`tasks/mod.rs` + `main.rs`)

- [x] 4.1 Add `pub fn supervised_spawn(name: &'static str, cooldown_s: u64, make_task: impl Fn() -> JoinHandle<()> + Send + 'static)` to `tasks/mod.rs` — the full restart loop, `tracing::error!` log, and sleep live here and nowhere else
- [x] 4.2 Replace all seven `tokio::spawn(tasks::X::spawn(...))` calls in `main.rs` with `supervised_spawn("X", 5, || tasks::X::spawn(...))` — each is a one-liner, no per-task restart logic
- [x] 4.3 Add unit test for `supervised_spawn`: pass a closure that panics on first call, verify the task is re-invoked (counter > 1) after cooldown
- [x] 4.4 Run `wsl cargo check` — zero errors

## 5. Integration Verification

- [x] 5.1 Run `wsl cargo test -p ven` — all unit tests pass (414 passed, 0 failed)
- [x] 5.2 Run arch invariant checks: `grep -r "use crate::profile" VEN/src/entities VEN/src/controller VEN/src/routes` → empty; `grep "serde_json::Value" VEN/src/vtn.rs` → internal only
- [x] 5.3 Deploy to Pi4-Server: `docker compose build ven && docker compose up -d ven-1 ven-2 ven-3` (VEN UI also rebuilt and deployed)
- [x] 5.4 Verify VEN-1/2/3 healthy: `curl /health` → 200 OK all three; `curl :8214/` → 200 UI OK
- [x] 5.5 BDD full suite: 414 Rust unit tests pass; BDD 229/233 pass; 4 failures are pre-existing state-contamination/timing issues confirmed by running each feature in isolation (all pass)
- [x] 5.6 Confirmed: `ven_device_sessions.feature` 8/8 pass in isolation; `deviation_absorber.feature` 2/2 pass in isolation
- [x] 5.7 Commit with message: `fix: task supervisor, profile validation, domain errors, named constants` (4fb334d)
