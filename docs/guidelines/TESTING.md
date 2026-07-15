# Testing Guide

## Quick Start

Run everything with one command:

```bash
bash run_all_tests.sh
```

Or pick individual suites:

```bash
bash run_all_tests.sh --local        # UI unit tests only
bash run_all_tests.sh --e2e          # E2E behave integration tests
bash run_all_tests.sh --resilience   # failure recovery tests
bash run_all_tests.sh --rust         # openleadr-rs cargo tests
bash run_all_tests.sh --local --e2e  # combine flags
```

---

## Test Suites

### 1. UI Unit Tests (local)

React component tests using Vitest + React Testing Library. Run locally, no Docker needed.

| Suite | Directory | Files | Command |
|---|---|---|---|
| VEN UI | `VEN/ui/` | see `VEN/ui/src/__tests__/` | `cd VEN/ui && npm test` |
| VTN UI | `VTN/ui/` | see `VTN/ui/src/__tests__/` | `cd VTN/ui && npm test` |

**Prerequisites:** Node.js, `npm install` in each UI directory.

**Windows note:** If using a subst drive (D: -> C:\DriveD), vitest may resolve paths through the real filesystem. Run from the real path if you get module resolution errors.

### 2. openleadr-rs Cargo Tests (Pi4-Server)

Rust unit and integration tests for the VTN library, client library, and wire protocol.

| Package | What it tests |
|---|---|
| `openleadr-vtn` | JWT, database queries, API handlers |
| `openleadr-client` | Client operations, event/program parsing |
| `openleadr-wire` | OpenADR 3 serialization/deserialization |

**Run directly (requires Rust toolchain + PostgreSQL):**

```bash
cd openleadr-rs
docker compose up db --wait -d
cargo sqlx migrate run --source openleadr-vtn/migrations
psql -U openadr -h localhost openadr < fixtures/test_user_credentials.sql
cargo test --workspace
docker compose down
```

**Run offline (no DB, compile-check only):**

```bash
cd openleadr-rs
SQLX_OFFLINE=true cargo test --workspace
```

### 3. E2E Integration Tests (Pi4-Server)

Behave (Python) scenarios testing the full stack: VTN, VENs, BFF, and UI through real HTTP calls. Runs inside Docker Compose with an ephemeral database.

Feature files live in `tests/features/` (`ls tests/features/*.feature` for the
current count — hard numbers rot). Coverage:
- VTN auth, programs, events CRUD
- VEN health, integration, sensors, reports
- BFF proxy endpoints
- Enrollment targeting
- VEN isolation (multi-tenant)
- 8 end-to-end use cases (UC1-UC8)
- Browser-based UI tests via Playwright

**Run on Pi4-Server:**

```bash
ssh Pi4-Server
cd /srv/docker/openadr_lab
docker compose -f tests/docker-compose.test.yml run --build --rm test-runner
docker compose -f tests/docker-compose.test.yml down -v
```

**Run specific tags:**

```bash
# Only UI tests
docker compose -f tests/docker-compose.test.yml run --rm test-runner --tags=@ui

# Only a specific feature
docker compose -f tests/docker-compose.test.yml run --rm test-runner features/enrollment.feature

# Upstream-pending tests (skipped by default, need PR #365 merged)
docker compose -f tests/docker-compose.test.yml run --rm test-runner --tags=@upstream_pending
```

**Excluded by default** (via `tests/behave.ini`):
- `@upstream_pending` — VEN report isolation tests awaiting upstream fix
- `@resilience` — requires Docker socket access, run separately

### 4. Resilience / Failure Recovery Tests (Pi4-Server)

Tests that services recover from restarts and outages. Uses Docker socket to stop/start compose services mid-test.

**4 behave scenarios:**
- VEN retains cached events when VTN goes down
- VEN re-syncs after VTN restart
- Both VENs converge after VTN restart
- VEN recovers after its own restart

**Run:**

```bash
ssh Pi4-Server
cd /srv/docker/openadr_lab
docker compose -f tests/docker-compose.test.yml run --build --rm test-runner --tags=@resilience
docker compose -f tests/docker-compose.test.yml down -v
```

**Standalone script** (alternative, runs outside Docker):

```bash
ssh Pi4-Server
cd /srv/docker/openadr_lab
# Start the test stack first
docker compose -f tests/docker-compose.test.yml up -d --build
# Run the script
bash tests/failure_recovery_test.sh
# Tear down
docker compose -f tests/docker-compose.test.yml down -v
```

---

## CI/CD

**No CI pipeline is configured yet.** `.github/workflows/` is empty.

Run linting and all test suites manually before merging any branch:

```bash
# Rust lint (WSL)
wsl cargo fmt --check -p ven
wsl cargo clippy -p ven -- -D warnings

# All suites
bash run_all_tests.sh
```

The openleadr-rs submodule has its own CI (`.github/workflows/checks.yml`) that runs on
upstream PRs — build, test, clippy, format, audit.

---

## Test Infrastructure

```
tests/
  docker-compose.test.yml   # 7-service ephemeral test stack
  Dockerfile                # Python test runner (behave + playwright + docker CLI)
  entrypoint.sh             # Loads SQL fixtures, provisions VEN-2, runs behave
  behave.ini                # Default tag exclusions
  requirements.txt          # behave, requests, playwright
  provision_ven2.py         # Idempotent VEN-2 setup via API
  failure_recovery_test.sh  # Standalone resilience script
  nginx-test.conf           # Nginx config for test UI
  features/
    environment.py          # Behave hooks (service waits, Playwright lifecycle)
    helpers/
      api_client.py         # HTTP helpers for VTN, VEN, BFF
      wait.py               # Polling/retry helpers
      ui.py                 # Playwright page object for VTN UI
      docker_ctl.py         # Docker compose stop/start/restart
    steps/                  # 13 step definition files
    *.feature               # 16 feature files
```

### Test Stack Services

| Service | Image | Role |
|---|---|---|
| test-db | postgres:16-alpine | Ephemeral database (no volume) |
| test-vtn | openleadr-rs | VTN server |
| test-ven-1 | VEN app | VEN with 5s polling |
| test-ven-2 | VEN app | Second VEN for enrollment/isolation tests |
| test-bff | BFF app | Dual-credential API proxy |
| test-ui | VTN UI (nginx) | React frontend |
| test-runner | Python test image | Runs behave + playwright |

---

## 5. VEN Rust — 4-Layer Test Pyramid

Required by Constitution Principle VI. All four layers must stay green after every VEN refactoring
phase. They complement the BDD suite — BDD is the safety net; these layers are the test surface that
each refactoring phase *creates*.

| Layer | Location | Framework | What it tests | Speed |
|-------|----------|-----------|---------------|-------|
| 1 — Domain | `#[cfg(test)]` in `entities/`, `controller/` | cargo test | Pure transformations — no I/O, no profile, no simulator | ms |
| 2 — Use case | `#[cfg(test)]` in `services/` + `MockSimulator`/`MockSolver` | cargo test | Service orchestration, trigger conditions, error paths, state transitions | ms |
| 3 — Adapter contract | `#[cfg(test)]` in `simulator/`, `vtn.rs`, `controller/milp/` | cargo test | Real adapter satisfies its port contract (no network — uses fixture JSON) | ms |
| 4 — Integration | `VEN/tests/*.rs` | cargo test / axum `oneshot` | Full HTTP roundtrip with real router, driven adapters real or doubled | s |

**Mock placement rule**: Shared mock adapters live in `VEN/src/services/test_support/`
(not `#[cfg(test)]` — Rust cannot import `cfg(test)` types across module boundaries).

**Test naming convention**: `test_<function>_<scenario>`, e.g. `test_build_setpoints_ev_plugged`.

**Run all four layers**: `cargo test -p ven`

New test surface unlocked per refactoring phase:

| Phase | Functions that become testable |
|-------|-------------------------------|
| 1 — `tasks/` split | `detect_event_changes()` as a pure function; each tick phase in isolation |
| 2 — `SimulatorPort` | `dispatcher::build_setpoints()`, `apply_surplus_ev_overlay()`, `absorber::apply_deviation_absorption()`, `monitor::record_tick()`, `envelope::compute_envelope()` |
| 3 — `AssetMilpContext` + `milp/` split | `build_milp_inputs()`, `translate_solution()`, per-phase constraint builders |
| 4 — Profile decoupled | All domain tests stop loading YAML; `BatteryParams::default()` replaces profile fixture wiring |
| 5 — Services | Full use case suite per service (`PlanningService`, `UserRequestService`, `ObligationService`, `HvacService`) |
