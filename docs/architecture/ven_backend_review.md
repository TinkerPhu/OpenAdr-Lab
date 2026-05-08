# VEN Backend — Architecture & Code Review

**Date:** 2026-05-03  
**Reviewer:** Claude (claude-sonnet-4-6)  
**Scope:** `VEN/src/` — all Rust modules (≈ 630 KB, ≈ 9 000 lines, 40 files)

---

## 1. Intended Architecture

The VEN backend is a three-tier real-time energy management system:

```
┌────────────────────────────────────────────────────────────────────┐
│  Tier 0 — Plan (MILP, minutes horizon)                             │
│  milp_planner → dispatcher → setpoints                             │
├────────────────────────────────────────────────────────────────────┤
│  Tier 1 — Absorber (reactive, sub-second)                          │
│  absorber → per-asset overlay → residual_kw                        │
├────────────────────────────────────────────────────────────────────┤
│  Tier 2 — Escalation (sustained deviation → trigger replan)        │
│  residual_ticks counter → PlanTrigger::DeviceDeviation             │
└────────────────────────────────────────────────────────────────────┘
```

Modules are grouped into four declared concerns:

| Module group    | Declared concern                        |
|-----------------|-----------------------------------------|
| `entities/`     | Domain data models (plan, rate, packet) |
| `controller/`   | Optimization & control logic            |
| `simulator/`    | Physics simulation                      |
| `routes/`       | HTTP surface                            |

Plus top-level modules: `state.rs` (shared state), `loops.rs` (background tasks), `profile.rs` (YAML config), `vtn.rs` (VTN API client).

---

## 2. Architecture Findings

### 2.1 Correct architectural decisions

- **Lock discipline enforced and documented** (`state.rs:164`): no function acquires more than one lock simultaneously; guards are always dropped before `.await`. This is the single most important safety property and it is consistently upheld.
- **Snapshot-and-release** pattern throughout: acquire RwLock → `clone()` → drop guard → work on snapshot. Prevents holding locks over async points.
- **Planning off the async runtime** (`loops.rs:spawn_planning`): `tokio::task::spawn_blocking` for the HiGHS call — correct, keeps the Tokio event loop unblocked.
- **Tier separation is clean**: absorber produces `residual_kw`; planning reacts to `residual_ticks`; dispatcher builds setpoints from plan. These three stages are genuinely independent code paths.
- **Change detection is pure** (`loops.rs:detect_event_changes`): no state mutations inside, returns trace events only. Correct design for testability.

### 2.2 Architectural breaches

#### AB-01 · `loops.rs` is a God Module

`loops.rs` (63 KB, 1 077 lines) contains seven structurally different concerns inside a single file:

| Function | Concern |
|---|---|
| `detect_event_changes()` | OpenADR event parsing |
| `spawn_program_poll()` | Program polling loop |
| `spawn_event_poll()` | Event polling loop |
| `spawn_report_poll()` | Report polling loop |
| `spawn_obligation_check()` | Obligation lifecycle |
| `spawn_sim_tick()` | Physics + absorber + Layer 2 |
| `spawn_planning()` | MILP solve + plan adoption |
| `spawn_state_persist()` | Persistence loop |

These belong in `controller/`, `simulator/`, and a small `loops/` (or `tasks/`) wrapper module. The current structure makes it impossible to reason about any one concern without scanning the entire file.

#### AB-02 · `milp_planner.rs` monolith

`milp_planner.rs` is 142 KB — the largest file in the project. It mixes:

- MILP input assembly (reading live state, resolving configs)
- Phase 1 constraint building (economic)
- Phase 2 constraint building (friction)
- Per-asset variable builders (battery, EV, heater, shiftable)
- Solver invocation and timeout
- Solution translation (variables → `Plan` slots)

None of these are separate files or even well-named internal sections. The declared architecture groups optimization under `controller/` but within that file there is no further structure. A reader cannot find "where does EV constraint building start?" without a text search.

#### AB-03 · Physics tick contains Layer 1 + Layer 2 logic

`spawn_sim_tick()` in `loops.rs` runs 8 phases. Phases 3 and 6 are `controller/` concerns (absorber correction, residual escalation) embedded inside what is documented as the simulator tick. The simulator module (`simulator/mod.rs`) is supposed to own the physics; the controller is supposed to own corrections. Currently the wiring crosses inside `spawn_sim_tick`, not at an explicit interface.

**Consequence:** Changing absorber logic requires reasoning about the 8-phase sim tick, not just `absorber.rs`.

#### AB-04 · `state.rs` has 90+ accessor methods making it a catch-all façade

`AppState` in `state.rs` owns all shared state across polling, control, and HEMS domains. It exposes 90+ methods: getters, setters, injection management, shiftable load lifecycle, request management, due obligations, persistence. This is not a state container; it is a service object. The HEMS business logic (cancelling a request clears linked device sessions) is embedded inside `AppState::cancel_request()` rather than in a domain service.

#### AB-05 · Routes reach into `state.rs` directly for domain logic

Route handlers in `routes/hems.rs` call `AppState` methods that encode domain rules (e.g., `cancel_request()` which also clears EV/heater sessions). The routes therefore implicitly depend on the ordering and side-effects of accessor methods rather than calling an explicit domain layer. There is no service/use-case layer between HTTP and state.

#### AB-06 · `vtn.rs` uses untyped JSON throughout

`VtnClient` returns `serde_json::Value` from all endpoints. There are no typed OpenADR structs. All downstream code uses `.get("field").and_then(...)` chains. This is a parsing architecture, not a type architecture — any field rename in the VTN breaks silently at runtime.

---

## 3. Code Complexity Findings

### 3.1 File-level complexity

| File | Size | Concern |
|---|---|---|
| `controller/milp_planner.rs` | 142 KB | Needs splitting — see AB-02 |
| `loops.rs` | 63 KB | Needs splitting — see AB-01 |
| `assets/heater.rs` | 40 KB | Thermal model is inherently complex; acceptable if isolated |
| `controller/timeline.rs` | 49 KB | Resampling + forecast; worth reviewing for hidden logic |
| `controller/reporter.rs` | 36 KB | OpenADR report building; may have hidden duplication |

### 3.2 Function-level complexity

**`spawn_sim_tick()` — 8 nested phases, ≈ 200 lines**  
Each phase has its own lock acquisition pattern and conditional logic. The phases are commented, but the function is not decomposable without extracting phase functions. Extracting `tick_phase_absorber()`, `tick_phase_physics()`, `tick_phase_escalation()` would isolate each concern.

**`spawn_planning()` — ≈ 240 lines**  
Contains: trigger selection, latch management, plan adoption gate, blocking solve, acceptance threshold logic, decay computation, status reporting. This is five independent decisions packed into one function.

**`solve_milp_two_phase()` in `milp_planner.rs`**  
Two nested good_lp constraint builders, solver invocations, and solution extraction — all in sequence with no sub-function decomposition. Debugging a constraint requires reading the full function.

**`build_milp_inputs()` in `milp_planner.rs`**  
Reads from `AppState`, `Profile`, and live `SimState` to produce `MilpInputs`. This is a ≈ 40-field struct assembly with conditionals for optional assets. There is no test for this function despite it being a pure transformation.

### 3.3 Magic numbers

Hardcoded values with no named constant and no configuration key:

| Location | Value | Meaning |
|---|---|---|
| `loops.rs:spawn_planning` | `5_000 ms` | Initial planning delay |
| `loops.rs:spawn_obligation_check` | `5 s` | Obligation check interval |
| `loops.rs:spawn_state_persist` | `15 s` | State persistence interval |
| `milp_planner.rs` | `60 s` | HiGHS solver timeout |
| `vtn.rs:ensure_token` | `60 s` | Token expiry safety margin |
| `absorber.rs` | `1 tick` | Settling ramp duration |

The configuration system (`PlannerConfig`, `SimulatorConfig`) exists and is well-designed. These values should live there.

---

## 4. Code Duplication Findings

### 4.1 Per-asset config accessor pattern repeated in `profile.rs`

```rust
pub fn ev_config(&self) -> Option<&EvConfig> { ... }
pub fn heater_config(&self) -> Option<&HeaterConfig> { ... }
pub fn pv_config(&self) -> Option<&PvConfig> { ... }
pub fn battery_config(&self) -> Option<&BatteryConfig> { ... }
```

These four methods are structurally identical: iterate `assets`, match on `AssetProfile::Ev(c)` / `::Heater(c)` / etc., return first match. Each is ≈ 8 lines of identical shape. A generic helper `find_asset::<T>()` or a macro would eliminate the repetition. The duplication also means a new asset type requires adding the same accessor boilerplate in four places.

### 4.2 401-retry pattern duplicated in `vtn.rs`

`get_json()`, `post_json()`, and `put_json()` each contain an identical inline 401-retry block:

```rust
if resp.status() == StatusCode::UNAUTHORIZED {
    self.token.write().await.take();
    // re-fetch token, retry once
}
```

This is ≈ 15 lines duplicated three times. A private `with_retry(|client| async { ... })` method would centralise the retry.

### 4.3 Energy counter accumulation pattern repeated across assets

`battery.rs`, `ev.rs`, `heater.rs`, `pv.rs`, `base_load.rs` each compute an energy counter update with the same arithmetic (`last_power_kw * dt_h → energy_kwh`). The `EnergyCounter` struct exists in `simulator/energy.rs` but each asset still reimplements the accumulation rather than calling a shared `counter.accumulate(power_kw, dt_h)` method. The duplication is ≈ 5–10 lines per asset.

### 4.4 Setpoint clamping logic repeated in dispatcher and absorber

Both `dispatcher.rs` and `absorber.rs` clamp setpoints to asset physical limits (min/max charge, grid limit). The clamping logic is inline in both files rather than delegated to the asset itself (`asset.clamp_setpoint(kw)` or similar). A change to an asset's physical bounds requires updating two files.

### 4.5 Snapshot pattern boilerplate in route handlers

Every route handler in `routes/hems.rs` follows:

```rust
let state = state.read_field().await;   // or multiple
// validate
// mutate via AppState method
// return Json(...)
```

The pattern itself is fine, but the error response construction (`Json(serde_json::json!({"error": "..."}))` with status code) is inlined in every handler rather than using a shared error response helper or a typed `AppError` enum with `IntoResponse`.

---

## 5. Other Quality Findings

### 5.1 No panic recovery in background loops

All seven background tasks are spawned as fire-and-forget (`tokio::spawn(...)`). A panic in any loop silently kills that task — the process continues running but the control functionality stops. There is no supervisor, no restart logic, no alerting. Given the long-running nature of the planning loop and the real-world consequences of a silent failure, this is the highest-risk operational gap.

### 5.2 Profile validation absent

`Profile::load()` deserialises YAML but does not validate:
- Absorber asset IDs reference assets that exist in `assets:`
- EV `soc_target` is in [0.0, 1.0]
- `max_discharge_kw` ≤ `capacity_kwh / step_h` (physics feasibility)
- Heater `thermal_mass` and `k_loss` are consistent with the configured tick interval

An invalid profile produces silent wrong behaviour or a runtime panic, not a startup error.

### 5.3 `AppState::to_json()` is not atomic

State is persisted by serialising `AppState` to a file. If the process is killed mid-write, the file is corrupted. There is no atomic rename (write to `.tmp`, rename). This is a low-frequency risk but will cause silent data loss on power loss.

### 5.4 `anyhow::Result` used everywhere, domain errors are invisible

`thiserror` is a declared dependency but unused. All errors are `anyhow::Error`. Callers cannot distinguish "VTN unreachable" from "plan infeasible" from "bad profile YAML" — they all surface as `anyhow::Error` with a message string. The planning loop catches errors generically and logs them; domain-specific recovery (e.g., keep last valid plan on infeasibility) is not possible without string matching.

### 5.5 Test coverage gaps

| Area | Coverage |
|---|---|
| `detect_event_changes()` | Partial (event_poll_tests) |
| `build_milp_inputs()` | None |
| `solve_milp_two_phase()` | None |
| `absorber.apply_deviation_absorption()` | None |
| `dispatcher.build_setpoints()` | None |
| `timeline` resampling | None |
| Route handlers | None |

The best-tested areas are `state.rs` (shiftable loads, request lifecycle) and `profile.rs` (config parsing). The core control path — the one where bugs have grid-level consequences — has no unit tests.

### 5.6 EV departure guard not implemented

`absorber.rs` (≈ lines 145–151) has a comment acknowledging that the EV departure guard (`ev_departure_guard_s` in `AbsorberAssetConfig`) is not yet enforced. The config field is parsed, stored, and validated in tests, but the actual guard (`if departure_in < guard_s { skip }`) is missing from `apply_deviation_absorption()`. The absorber can therefore reduce EV charge immediately before a departure deadline.

---

## 6. Options to Fix Findings

### Fix AB-01 — Split `loops.rs`

Create `tasks/` module (or `loops/`) with one file per concern:

```
VEN/src/tasks/
  mod.rs               — re-exports spawn_* functions
  event_poll.rs        — spawn_program_poll, spawn_event_poll, detect_event_changes
  sim_tick.rs          — spawn_sim_tick (phases 0–5 physics only)
  planning.rs          — spawn_planning
  obligation.rs        — spawn_obligation_check
  persist.rs           — spawn_state_persist
```

The physics loop should call `absorber::apply_deviation_absorption()` and `controller::escalate_if_needed()` as explicit calls, not inline phases. Estimated effort: **1–2 days**, no logic changes, pure extraction.

### Fix AB-02 — Split `milp_planner.rs`

```
VEN/src/controller/milp/
  mod.rs               — run_planner(), public entry point
  inputs.rs            — MilpInputs, build_milp_inputs()
  phase1.rs            — Phase1Weights, build_phase1_constraints()
  phase2.rs            — Phase2Weights, build_phase2_constraints()
  assets/
    battery.rs         — battery constraint builders
    ev.rs              — EV constraint builders
    heater.rs          — heater constraint builders
    shiftable.rs       — shiftable load constraint builders
  translate.rs         — translate_solution() → Plan
```

Estimated effort: **2–3 days**, no logic changes, structural split only.

### Fix AB-03 — Extract tick phases as functions

Inside `sim_tick.rs` (after AB-01 fix), replace the 8-phase block with named function calls:

```rust
async fn run_sim_tick(ctx: &AppCtx) {
    let setpoints = dispatcher::build_setpoints(&ctx.state, &profile).await;
    let (residual_kw, overlay) = absorber::apply(setpoints, &sim_snapshot, &profile);
    let snap = simulator.tick(setpoints_with_overlay, dt_s);
    controller::escalate_if_needed(residual_kw, &ctx.state, &trigger_tx).await;
}
```

Estimated effort: **0.5 days** alongside AB-01.

### Fix AB-04 / AB-05 — Introduce a thin domain service layer

Rather than routing directly into `AppState`, add a `VenService` (or per-domain services: `HvacService`, `EvService`) that owns the business rules:

```rust
impl VenService {
    pub async fn cancel_request(&self, id: Uuid) -> Result<(), DomainError> { ... }
    pub async fn start_ev_session(&self, target_soc: f64) -> Result<(), DomainError> { ... }
}
```

`AppState` becomes a pure state container; business rules move to the service. Routes call the service. This is a **larger refactor** (3–5 days) and should be scheduled when adding new domain features, not as a standalone cleanup.

### Fix AB-06 — Type the VTN client

Add typed structs for OpenADR 3 payloads in `vtn.rs` (or a new `openleadr_types.rs`):

```rust
#[derive(Deserialize)]
struct OpenAdrEvent { id: String, program_id: String, event_name: String, ... }
```

Replace `serde_json::Value` returns with typed results. This catches VTN field changes at deserialization, not deep in parsing chains. Estimated effort: **1–2 days**, medium risk (touches all parsing code).

### Fix duplication CD-01 — Generic asset config accessor

```rust
pub fn asset_config<T: 'static>(&self) -> Option<&T> {
    self.assets.iter().find_map(|a| a.as_any().downcast_ref::<T>())
}
```

Or simpler: a macro over the existing pattern to eliminate boilerplate while keeping explicit methods. Estimated effort: **2–4 hours**.

### Fix duplication CD-02 — Centralise 401-retry in `vtn.rs`

```rust
async fn with_auth_retry<F, Fut, T>(&self, f: F) -> Result<T>
where F: Fn(HeaderValue) -> Fut, Fut: Future<Output = Result<reqwest::Response>>;
```

All three HTTP methods delegate to this. Estimated effort: **2–4 hours**.

### Fix duplication CD-03 — `EnergyCounter::accumulate()`

Add a method to the existing `EnergyCounter` struct:

```rust
impl EnergyCounter {
    pub fn accumulate(&mut self, power_kw: f64, dt_h: f64) { ... }
}
```

Remove inline arithmetic from each asset. Estimated effort: **1–2 hours**.

### Fix quality — Panic recovery for background loops

Wrap each spawned task:

```rust
tokio::spawn(async move {
    loop {
        let result = spawn_sim_tick(ctx.clone()).await;
        if let Err(e) = result {
            tracing::error!("sim_tick loop died: {e:#}, restarting in 5s");
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }
});
```

Estimated effort: **2–4 hours**, high operational value.

### Fix quality — Profile validation on startup

Add a `Profile::validate()` method called in `main.rs` after `Profile::load()`. Return `Err` with a descriptive message on any invariant violation. Fail fast at startup rather than silently. Estimated effort: **2–4 hours**.

### Fix quality — Atomic state persistence

```rust
let tmp = format!("{path}.tmp");
tokio::fs::write(&tmp, json).await?;
tokio::fs::rename(&tmp, path).await?;
```

Two lines, eliminates corrupt-on-crash. Estimated effort: **30 minutes**.

### Fix quality — Implement EV departure guard in absorber

In `apply_deviation_absorption()`, before reducing EV setpoint:

```rust
if let Some(guard_s) = absorber_cfg.ev_departure_guard_s {
    if time_to_departure_s < guard_s as f64 {
        continue; // skip EV for this correction
    }
}
```

Requires surfacing `departure_time` from `EvState` to the absorber call site. Estimated effort: **2–4 hours**.

---

## 7. Priority Summary

| # | Finding | Severity | Effort | Action |
|---|---|---|---|---|
| 1 | No panic recovery in background loops | **High** | 2–4 h | Fix now |
| 2 | EV departure guard not implemented | **High** | 2–4 h | Fix now |
| 3 | Atomic state persistence | **High** | 30 min | Fix now |
| 4 | `loops.rs` God Module (AB-01) | **Medium** | 1–2 d | Next refactor sprint |
| 5 | `milp_planner.rs` monolith (AB-02) | **Medium** | 2–3 d | Next refactor sprint |
| 6 | 401-retry duplication in `vtn.rs` | **Medium** | 2–4 h | Next refactor sprint |
| 7 | Profile startup validation | **Medium** | 2–4 h | Next refactor sprint |
| 8 | No unit tests on control path | **Medium** | ongoing | Add alongside each change |
| 9 | Untyped VTN JSON (AB-06) | **Medium** | 1–2 d | Planned feature work |
| 10 | Magic numbers without config keys | **Low** | 2–4 h | Next refactor sprint |
| 11 | Asset config accessor duplication | **Low** | 2–4 h | Next cleanup |
| 12 | `anyhow` everywhere, no domain errors | **Low** | 3–5 d | Long-term |
| 13 | Route → service layer (AB-04/05) | **Low** | 3–5 d | Long-term |
