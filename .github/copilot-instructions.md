# OpenADR Lab — Copilot Instructions

## Base instructions
Look at .claude\CLAUDE.md for further instructions.
Refer to docs\reference\KEY_LEARNINGS.md to learn from past issues.
Commit messages: DO NOT ADD and 'Co-authored-by: Copilot ...' line 

## Architecture Overview

Two independent stacks run in Docker Compose on a Linux host (designed for Raspberry Pi 4 ARM64):
The Linux host is easy to reach: ssh Pi4-Server (no password needed, key pairs installed).

**VTN Stack** (`VTN/`)
- `openleadr-rs/` — OpenADR 3.0 VTN server (Rust, git submodule, TinkerPhu fork). Built from source; first build ~25 min on Pi4.
- PostgreSQL 16 — VTN persistence with SQLx auto-migrations (15 tables)
- `VTN/bff/` — Rust (axum) Backend-for-Frontend. Holds **two** OAuth credentials: `any-business` (CRUD) and `ven-manager` (VEN enrollment). The UI never touches the VTN directly.
- `VTN/ui/` — React + MUI operator dashboard (programs, events, VENs, reports)

**VEN Stack** (`VEN/`)
- `VEN/src/` — Rust (axum + tokio) polling-based VEN with physics simulation and HEMS controller. Three instances run via Docker Compose environment variables.
- `VEN/ui/` — React + MUI device dashboard (events, sensors, simulation, planner)

The stacks communicate over `openadr-net` (Docker bridge). VENs poll the VTN; the VTN never pushes to VENs.

## VEN Application Internals

The VEN is a single Rust binary with background tokio tasks and an HTTP API:

**`AppCtx`** — cloneable context injected into all route handlers:
```
AppCtx { state: AppState, vtn: VtnClient, metrics_handle, trigger_tx, profile, sim }
```

**`AppState`** — `Arc<RwLock<InnerState>>`, accessed only through async methods. All state mutations go through named accessors (never lock directly from outside `state.rs`).

Fields marked `#[serde(skip)]` in `InnerState` are **not persisted** to `state.json` — this includes all HEMS runtime state (`active_packets`, `active_plan`, `report_obligations`, etc.). Only `programs`, `events`, `reports`, and `sensor` survive restarts.

**Background loops** (spawned in `loops.rs`):
- `spawn_program_poll` / `spawn_event_poll` / `spawn_report_poll` — polling loops to the VTN
- `spawn_sim_tick` — physics simulation tick; posts sensor data and sends `PlanTrigger` on change
- `spawn_planning` — MILP planner; receives `PlanTrigger` via `watch::channel` for reactive re-planning
- `spawn_obligation_check` — monitors and fulfills report obligations
- `spawn_state_persist` — periodically serializes `InnerState` to disk

**Controller** (`VEN/src/controller/`):
- `milp_planner.rs` — LP-based HEMS planner using `good_lp`/HiGHS
- `dispatcher.rs` — translates plan into asset setpoints
- `reporter.rs` — builds OpenADR reports from sensor/asset data
- `openadr_interface.rs` — interprets OpenADR events into controller inputs

**Simulation injection** has three behaviours:
- **A (one-shot)**: applied once to physics, then auto-cleared (`battery_soc`, `ev_soc`, `heater_temp_c`)
- **B (frozen + EMA return)**: held while active, EMA-blended back on release (`pv_irradiance`, `base_load_kw`)
- **C (frozen + snap)**: held while active, snaps to profile default on release (all others)

## Build & Test Commands

### UI Unit Tests (local, no Docker)
```bash
cd VEN/ui && npm install && npm test     # VEN UI — Vitest
cd VTN/ui && npm install && npm test     # VTN UI — Vitest
```

Run a single test file:
```bash
cd VEN/ui && npx vitest run src/__tests__/MyComponent.test.tsx
```

**Windows note:** If using a subst drive (D: → C:\DriveD), run vitest from the real path to avoid module resolution errors.

### E2E Integration Tests (requires Docker)
```bash
# Run full behave suite
docker compose -f tests/docker-compose.test.yml run --build --rm test-runner
docker compose -f tests/docker-compose.test.yml down -v

# Run a single feature file
docker compose -f tests/docker-compose.test.yml run --rm test-runner features/ven_sensors.feature

# Run by tag
docker compose -f tests/docker-compose.test.yml run --rm test-runner --tags=@ui
docker compose -f tests/docker-compose.test.yml run --rm test-runner --tags=@resilience
```

Tags excluded by default (see `tests/behave.ini`): `@upstream_pending`, `@resilience`.

### All tests via script
```bash
bash run_all_tests.sh              # everything
bash run_all_tests.sh --local      # UI unit tests only
bash run_all_tests.sh --e2e        # E2E behave only
bash run_all_tests.sh --resilience # resilience tests only
bash run_all_tests.sh --rust       # openleadr-rs cargo tests only
```

Configure `DOCKER_HOST` and `DOCKER_DIR` at the top of `run_all_tests.sh` for remote execution.

### openleadr-rs Cargo Tests (requires PostgreSQL)
```bash
cd openleadr-rs
docker compose up db --wait -d
cargo sqlx migrate run --source openleadr-vtn/migrations
psql -U openadr -h localhost openadr < fixtures/test_user_credentials.sql
cargo test --workspace
docker compose down

# Offline compile-check only:
SQLX_OFFLINE=true cargo test --workspace
```

## Key Conventions

### React Components
- Use named `function` declarations, not `const` + `FC`:
  ```tsx
  export function MyComponent(props: MyComponentProps) {
    const { value, onChange } = props;
  ```
- Define a `Props` type (e.g. `MyComponentProps`) and pass as generic to `props`.
- Add `data-testid` to every interactive and data-displaying element.
- Only add explicit `role` when the rendered element lacks a semantic role (MUI components provide most roles automatically).

### React Data Fetching
- All API calls use TanStack React Query (`useQuery` / `useMutation`).
- The BFF base URL is the only API endpoint; VEN UIs talk to the VEN's own HTTP API.
- Invalidate relevant query keys in `onSuccess` of mutations.

### Rust State
- All shared state lives behind `Arc<RwLock<...>>`; never hold the lock across `await` points.
- HEMS in-memory state uses `#[serde(skip)]` — assume it resets on restart and plan accordingly.
- Use `anyhow::Result` for application errors; `thiserror` for library/domain errors.
- Structured JSON logging via `tracing`; log level controlled by `RUST_LOG` env var.

### Tests (Behave)
- Feature files in `tests/features/`; step definitions in `tests/features/steps/`.
- Helpers: `api_client.py` (HTTP), `wait.py` (retry/polling), `ui.py` (Playwright page objects), `docker_ctl.py` (compose stop/start).
- New resilience scenarios must be tagged `@resilience` to run separately.
- Tests that depend on upstream VTN fixes are tagged `@upstream_pending`.

### Docker
- Each stack has its own `docker-compose.yml`; the test stack is `tests/docker-compose.test.yml`.
- The test stack uses an ephemeral PostgreSQL (no volume) — it resets on every `down -v`.
- `openleadr-rs` is a git submodule at the repo root; always clone with `--recursive`.
