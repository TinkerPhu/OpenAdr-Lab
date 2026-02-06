# OpenADR Lab — Project Journal

---

## Project Overview

This project builds a **Raspberry Pi 4–hosted OpenADR 3 lab environment** for demand response experimentation. The Pi runs Docker and hosts a VTN (Virtual Top Node) stack, multiple VEN (Virtual End Node) containers, and web UIs — all communicating over a shared Docker bridge network (`openadr-net`).

The system design is defined in `open_adr_3_raspberry_pi_lab_complete_system_design.md`.

---

## What Has Been Done

### 1. VTN Stack — Deployed and Running

**Status: COMPLETE**

The VTN stack is live on Pi4-Server with two healthy containers:

| Container | Image | Status | Port |
|-----------|-------|--------|------|
| `vtn-vtn-1` | openleadr-rs (built from source) | healthy | 3000 |
| `vtn-db-1` | postgres:16-alpine | healthy | 5432 |

**What was done:**
- Created `VTN/docker-compose.yml` with services `db` (PostgreSQL) and `vtn` (openleadr-rs)
- Cloned `openleadr-rs` at project root (not inside VTN/); docker-compose references `../openleadr-rs`
- Built VTN from source inside Docker (~25 min on Pi4 ARM64, cached afterwards)
- Confirmed VTN auto-runs SQLx migrations at startup (15 tables created)
- Loaded test credential fixtures (5 users: any-business, ven-manager, user-manager, business-1, ven-1)

**Confirmed VTN behavior:**
- Health endpoint: `GET /health` returns `OK`
- Token endpoint: `POST /auth/token` (not `/oauth/token`)
- Token expiry: 2,592,000 seconds (30 days)
- Role-based access: `any-business` can access /programs, /events but NOT /vens (403)
- `ven-manager` credentials required for VEN management

### 2. Step-by-Step Setup Guide — Written and Verified

**Status: COMPLETE**

`VTN/vtn_setup_from_blog_step_by_step.md` was updated with all confirmed findings from the actual deployment. Every section was verified against the running system — no assumptions remain.

### 3. Infrastructure — Git + Deployment Pipeline

**Status: COMPLETE**

- Repository on GitHub, Pi4-Server pulls via HTTPS with PAT
- `ssh Pi4-Server "cd /srv/docker/openadr_lab && git pull"` works
- `.gitignore` excludes `openleadr-rs/` (cloned third-party repo)

### 4. Design Documents — All Written

**Status: COMPLETE (design phase)**

| Document | Location | Purpose |
|----------|----------|---------|
| System Design | `open_adr_3_raspberry_pi_lab_complete_system_design.md` | Master architecture |
| VTN Setup Guide | `VTN/vtn_setup_from_blog_step_by_step.md` | Deployment instructions |
| VTN BFF Blueprint | `VTN/vtn_rust_bff_blueprint.md` | Rust backend-for-frontend |
| VTN Web UI Blueprint | `VTN/vtn_web_ui_blueprint.md` | React + MUI operator console |
| VEN Container Blueprint | `VEN/ven_container_blueprint.md` | Rust VEN application |
| VEN Web UI Blueprint | `VEN/ui/ven_web_ui_blueprint.md` | React + MUI VEN dashboard |
| VTN DTO Examples | `VTN/DTO examples/` | JSON/TS sample payloads |
| Integration Tests | `tests/` | behave/Gherkin + Docker Compose test stack |

### 5. VEN Application — Deployed and Running

**Status: COMPLETE**

Three VEN instances running on Pi4-Server, all connecting to the VTN:

| Container | Credentials | Port |
|-----------|-------------|------|
| `ven-ven-1-1` | ven-1/ven-1 | 8081 |
| `ven-ven-2-1` | ven-2/ven-2 | 8082 |
| `ven-ven-3-1` | ven-3/ven-3 | 8083 |

**What was done:**
- Completed Rust source: `main.rs`, `models.rs`, `state.rs`, `vtn.rs`, `config.rs`
- Created `VEN/Dockerfile` (multi-stage rust:1.90-alpine build, nonroot user)
- Created `VEN/docker-compose.yml` with 3 VEN services on external `vtn_openadr-net`
- Registered ven-2 and ven-3 OAuth credentials via VTN API
- VENs poll programs (300s), events (30s), generate fake sensor data (10s), persist state (15s)

### 6. VEN Web UI — Scaffolded (Not Complete)

**Status: IN PROGRESS — components started, not buildable yet**

React/TypeScript files exist:
- `VEN/ui/src/App.tsx`
- `VEN/ui/src/api/client.ts`
- `VEN/ui/src/hooks/usePoll.ts`
- `VEN/ui/src/pages/Dashboard.tsx`, `Events.tsx`, `Programs.tsx`, `Sensors.tsx`
- `VEN/ui/src/components/JsonDialog.tsx`
- `VEN/ui/src/datamodel.ts`

**No `package.json`, no `vite.config.ts`, no Dockerfile** — not buildable yet.

### 7. Integration Test Suite — Complete

**Status: COMPLETE**

End-to-end integration tests using Python `behave` (Cucumber/Gherkin) running inside a self-contained Docker Compose test stack. Tests are black-box HTTP calls — no code linkage to VEN/VTN.

**Test stack** (`tests/docker-compose.test.yml`, project name `openadr-test`):

| Service | Image | Purpose |
|---------|-------|---------|
| `test-db` | postgres:16-alpine | Ephemeral DB (no volume) |
| `test-vtn` | build openleadr-rs | VTN server (auto-migrates) |
| `test-ven-1` | build VEN | Single VEN with 5s poll intervals |
| `test-runner` | build tests/ | Loads fixtures via psql, runs `behave` |

**Test results: 6 features, 12 scenarios, 43 steps — all passing.**

| Feature | Scenarios | What's tested |
|---------|-----------|---------------|
| `vtn_auth` | 2 | Valid/invalid OAuth token requests |
| `vtn_programs` | 3 | Create, list, unauthenticated rejection |
| `vtn_events` | 2 | Create event for program, list events |
| `ven_health` | 1 | Health endpoint returns "ok" |
| `ven_integration` | 3 | VEN reflects VTN programs/events, auto-generates sensors |
| `ven_sensors` | 1 | POST sensor data, GET it back |

**Key design decisions:**
- Isolated `test-net` network, no published ports (no conflict with production)
- VEN poll intervals set to 5s (not 30/300s) for fast test feedback
- Test-runner loads SQL fixtures via `psql` in entrypoint before running behave
- Integration tests use `poll_until()` for eventual consistency checks
- No persistence volume on test VEN (ephemeral)

**Run command:**
```bash
cd /srv/docker/openadr_lab
docker compose -f tests/docker-compose.test.yml up --build \
  --abort-on-container-exit --exit-code-from test-runner
docker compose -f tests/docker-compose.test.yml down
```

### 8. VTN BFF + VTN Web UI — Not Started

**Status: NOT STARTED — blueprints written, no code**

---

## What To Do Next

Based on the system design's implementation order (Section 19) and current state:

### Phase 1: Complete the VEN Application (Priority: HIGH)

This is the next logical step. The VTN is running but there's nothing talking to it.

**Tasks:**
1. **Finish the VEN Rust application** — complete `main.rs`, `models.rs`, `state.rs`, `vtn.rs` to implement:
   - OAuth client (client_credentials grant against `/auth/token`)
   - Program poller (every 5 min)
   - Event poller (every 30 sec)
   - Sensor sampler (simulated telemetry)
   - Local REST API (GET /health, /events, /programs, /sensors)
2. **Create VEN Dockerfile** — multi-stage Rust build (similar to vtn.Dockerfile)
3. **Create VEN docker-compose.yml** — initially for 1 VEN, then scale to 3
4. **Register VEN OAuth clients in VTN DB** — the test fixtures only have `ven-1`; add `ven-2`, `ven-3`
5. **Deploy to Pi4-Server** — build and run alongside VTN stack
6. **Validate end-to-end** — VEN obtains token, discovers programs, polls events

### Phase 2: VEN Web UI (Priority: MEDIUM)

Once VENs are running and their APIs are accessible:

1. **Scaffold the React app properly** — `package.json`, Vite config, MUI
2. **Complete the UI pages** — Dashboard, Events, Programs, Sensors
3. **Add multi-VEN selector** — switch between VEN1/VEN2/VEN3
4. **Containerize** — Nginx-based Docker image
5. **Deploy to Pi4-Server**

### Phase 3: Create Programs and Events via VTN API (Priority: MEDIUM)

The VTN is running but empty — no programs or events exist yet.

1. **Create test programs** via curl/API (using `any-business` credentials)
2. **Create test events** targeted at VENs
3. **Verify VEN polls pick up events**
4. **Verify event distribution across multiple VENs**

### Phase 4: VTN BFF + VTN Web UI (Priority: LOWER)

The operator console for managing the VTN:

1. **Build the Rust BFF** (axum) — OAuth proxy, caching, event composition
2. **Build the React VTN UI** — Dashboard, Programs, VENs, Events, Event Composer
3. **Deploy as additional containers on Pi4-Server**

### Phase 5: Reporting and Telemetry (Priority: LOWER)

1. **Implement report sender in VEN** — submit telemetry to VTN
2. **Verify VTN report ingestion**
3. **Add telemetry charts to VEN UI**
4. **Evaluate whether VTN persists report datapoints** (known open question)

### Phase 6: Hardening and Observability (Priority: FUTURE)

- Structured JSON logging across all containers
- Prometheus metrics endpoints
- Retry/backoff on VEN failures
- Offline telemetry buffering
- Optional TLS via reverse proxy

---

## Architecture Reference

```
Raspberry Pi 4 — Docker Host
├── openadr-net (bridge network)
│
├── vtn-db-1      [postgres:16-alpine]     :5432  RUNNING
├── vtn-vtn-1          [openleadr-rs]           :3000  RUNNING
│
├── ven-ven-1-1        [ven-app]                :8081  RUNNING
├── ven-ven-2-1        [ven-app]                :8082  RUNNING
├── ven-ven-3-1        [ven-app]                :8083  RUNNING
│
├── vtn-bff            [rust axum]              :8090  NOT YET
├── vtn-ui             [react+nginx]            :8080  NOT YET
└── ven-ui             [react+nginx]            :8084  NOT YET
```

---

## Phase 2 Work Log: VEN Deployment (2026-02-06)

### Discovering the Correct VTN API Shapes

The VEN code had been scaffolded with assumed API field names (`name`, `program_id`, `/oauth/token`). To find the actual shapes, I queried the live VTN:

1. **Token endpoint**: Already confirmed in Phase 1 as `POST /auth/token` (not `/oauth/token`).
2. **Programs**: Created a test program via `POST /programs` with `{"programName": "Test DR Program"}` and inspected the response. Discovered the VTN uses `programName` (not `name`) and `programLongName`.
3. **Events**: Created a test event via `POST /events` with `{"programID": "...", "eventName": "...", "intervals": [...]}` and inspected the response. Discovered the VTN uses `programID` (not `program_id`), `createdDateTime` (not `created_at`), and `eventName`. Events have no `status` field — status must be derived from interval timing.

### Discovering the User/VEN Management API

The VTN's test fixtures only included `ven-1`. To add `ven-2` and `ven-3`, I needed to figure out the user management API:

1. **Read the fixture SQL files** on the Pi (`/srv/docker/openadr_lab/openleadr-rs/fixtures/test_user_credentials.sql`) to understand the data model: `"user"` table → `user_credentials` table → `user_ven` table → `ven` table.
2. **Tried `POST /users`** with `user-manager` credentials. Got a 400 error: "missing field `roles`". Added `"roles": []` — success.
3. **Credentials were tricky**: The `user_credentials` table stores argon2 hashes, so direct SQL INSERT wouldn't work. I searched the openleadr-rs source code on the Pi (`grep -n 'credential' .../api/user.rs`) and found `add_credential` is a `POST /users/{id}` with `{"client_id": "...", "client_secret": "..."}`. This auto-hashes the secret.
4. **Created VEN entities**: `POST /vens` with `ven-manager` credentials creates VEN entities.
5. **Role assignment**: Read the Rust source (`jwt.rs`) to find the `AuthRole` enum uses `#[serde(tag = "role", content = "id")]`, so the JSON format is `{"role": "VEN", "id": "<ven-uuid>"}`. Applied via `PUT /users/{id}` with the roles array.

### Complete API sequence for adding a new VEN

```
1. POST /users             (user-manager)  → create user
2. POST /users/{userId}    (user-manager)  → add client_id/client_secret
3. POST /vens              (ven-manager)   → create VEN entity
4. PUT  /users/{userId}    (user-manager)  → assign VEN role with VEN ID
```

### VEN Build and Deploy

- Rewrote `VEN/Dockerfile` to use `rust:1.90-alpine` (matching VTN) with multi-stage build, dep caching, and nonroot user
- Created `VEN/docker-compose.yml` with 3 VEN services sharing the VTN's external network (`vtn_openadr-net`)
- Built VEN on Pi4: ~10 min for dependencies + 1 min for app code (total ~11 min)
- VEN-1 started and immediately:
  - Authenticated with VTN using `ven-1/ven-1`
  - Polled 1 program, 1 event
  - Sensor sampler generating simulated power readings

### Docker Compose Project Name Insight

Docker Compose prefixes container names with the **project name**, which defaults to the parent directory name. Since the VTN compose is in `VTN/`, a service called `vtn-db` resulted in container `vtn-vtn-db-1`. Reverted the service to just `db` so the container is `vtn-db-1`.

---

## Phase 3 Work Log: Integration Test Suite (2026-02-06)

### Design Decisions

Chose Python `behave` (Gherkin/Cucumber) for integration tests — familiar BDD syntax, fast iteration, no need to compile. Tests are pure black-box HTTP calls: they hit the VTN and VEN REST APIs and assert on responses.

The test stack runs in a completely isolated Docker Compose project (`openadr-test`) with its own network (`test-net`), no published ports, and no shared volumes. This means tests can run alongside the production stack without interference.

### Initial Approach: fixture-loader Container

First design used a separate `fixture-loader` service (postgres:16-alpine) that ran `test_user_credentials.sql` and exited. The VEN depended on it via `service_completed_successfully`. Problem: `--abort-on-container-exit` kills ALL containers when ANY container exits, including the fixture-loader. The test-runner never got a chance to start.

### Fix: Load Fixtures in Test-Runner

Moved fixture loading into the test-runner's entrypoint script. Added `postgresql-client` to the Python Alpine image. The entrypoint runs `psql` to load fixtures, then `exec behave`. This means only long-running services (db, vtn, ven) and the test-runner exist — no premature exits.

The VEN starts before fixtures are loaded (it depends on test-vtn healthy, not fixtures). Its poll retry logic handles the initial auth failures gracefully — once fixtures are loaded and the next poll cycle fires (5s), authentication succeeds.

### Duplicate Program Name Bug

The `vtn_events.feature` used a `Background` that created a program named "event-test-program". Since Background runs before **each** scenario, the second scenario hit a unique constraint violation. Fixed by using unique program names per scenario.

### Test Execution Performance

All 12 scenarios complete in ~9 seconds (after services are healthy). The VEN's 5-second poll interval (vs 30/300s in production) keeps the integration tests snappy. The `poll_until()` helper in `wait.py` handles eventual consistency by retrying with a timeout.

---

## Key Learnings

- VTN auto-migrates on first boot — no need for manual `cargo sqlx migrate run`
- Token endpoint is `/auth/token`, not `/oauth/token`
- Token expires in 30 days (2,592,000 sec), not 1 hour
- VTN build takes ~25 min on Pi4 ARM64 (first time); cached builds are fast
- VEN build takes ~11 min on Pi4 ARM64 (first time); cached rebuilds are ~1 min
- SSH to Pi has no interactive terminal — git credentials must be written directly to `~/.git-credentials`
- Role-based access is enforced: wrong role = 403 Forbidden
- Docker Compose project name = directory name; avoid duplicating it in service names
- VTN API field names follow OpenADR 3 spec: `programName`, `programID`, `createdDateTime`, `venName`
- To discover an unfamiliar API: create test data, inspect responses, and read the source when needed
- User credential creation requires the API (not raw SQL) because secrets are argon2-hashed server-side
- `--abort-on-container-exit` kills everything when ANY container exits — don't use one-shot containers alongside it
- Gherkin `Background` runs before EACH scenario, not once per feature — use unique test data names
- VEN poll retry logic handles auth failures gracefully — safe to start before fixtures are loaded
- `poll_until()` with short intervals is the right pattern for testing eventual consistency across services

---

*Last updated: 2026-02-06*
