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

### 6. VEN Web UI — Built, Tested, and Deployed

**Status: COMPLETE**

React + TypeScript SPA served by nginx on port 8084:

| Container | Image | Status | Port |
|-----------|-------|--------|------|
| `ven-ui-1` | ven-ui (node build + nginx) | running | 8084 |

**What was done:**
- Created full Vite build infrastructure (`package.json`, `vite.config.ts`, `tsconfig.json`, `index.html`)
- Replaced manual `usePoll` hook with `@tanstack/react-query` (`useQuery` with `refetchInterval`)
- Created `VenContext` for multi-VEN support — selector switches data across all pages
- Added `data-testid` and `aria` attributes on all interactive/data elements per `REACT_GUIDELINES.md`
- Removed redundant `role` attributes where MUI already provides them natively
- Created `SensorForm` component for POST /sensors injection
- Wrote 31 component tests across 6 test files (Vitest + Testing Library)
- Multi-stage Docker build (node:20-alpine build + nginx:alpine serve) with SPA fallback
- Deployed to Pi4-Server as `ui` service in VEN docker-compose

**Architecture:**
- `src/api/hooks.ts` — 5 react-query hooks (`useHealth`, `usePrograms`, `useEvents`, `useSensor`, `usePostSensor`)
- `src/api/client.ts` — `VenApi` class wrapping fetch calls to VEN REST API
- `src/api/types.ts` — `Program`, `Event`, `SensorSnapshot` types
- Pages: Dashboard (summary cards), Programs (searchable list), Events (filterable table with JSON dialog), Sensors (live data + injection form)

**Also updated:**
- `ReactCodingGuideLines.md` → renamed to `REACT_GUIDELINES.md`
- Guidelines updated: consistent function component signatures (no `FC`), smart `role` usage, Vitest test patterns, react-query v5 object syntax

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

### 8. VTN Seeded with Demo Data

**Status: COMPLETE**

Created `scripts/seed_vtn.py` — a standalone Python script that populates the VTN with realistic demo data via the REST API.

**Programs created (3):**

| programName | Description |
|---|---|
| Summer Peak DR | Demand response for summer peak hours |
| EV Managed Charging | Managed EV charging load shifting |
| HVAC Optimization | Building HVAC pre-cool/pre-heat |

**Events created (6 — 2 per program):**

| Program | eventName | Payload (kW) |
|---|---|---|
| Summer Peak DR | peak-curtail-1 | 5.0 |
| Summer Peak DR | peak-curtail-2 | 10.0 |
| EV Managed Charging | ev-shift-morning | 3.5 |
| EV Managed Charging | ev-shift-evening | 7.0 |
| HVAC Optimization | precool-event | 2.0 |
| HVAC Optimization | preheat-event | 4.0 |

**Script features:**
- Authenticates as `any-business` via `POST /auth/token`
- Idempotent for programs — checks existing by name, skips duplicates
- Takes `--vtn-url`, `--client-id`, `--client-secret` args
- Prints summary of all created/skipped resources

**Verified:**
- All 3 programs and 6 events visible on VTN
- Events flowing to all 3 VENs (within 30s poll cycle)
- Programs visible to VENs (within 300s poll cycle)
- VEN Web UI at port 8084 reflects the data

### 9. VTN BFF — Deployed and Running

**Status: COMPLETE**

Rust axum BFF (Backend-for-Frontend) proxying the VTN API with OAuth token management and TTL caching:

| Container | Image | Status | Port |
|-----------|-------|--------|------|
| `vtn-bff-1` | vtn-bff (rust:1.90-alpine build) | healthy | 8090 |

**Endpoints:**
- `GET /api/health` — BFF status + VTN reachability/auth check
- `GET /api/programs` — cached proxy (30s TTL) via `any-business` credential
- `GET /api/events` — cached proxy (10s TTL) via `any-business` credential
- `GET /api/vens` — cached proxy (10s TTL) via `ven-manager` credential

**Key design decision — dual credentials:**
The VTN enforces role-based access: `any-business` can access `/programs` and `/events` but NOT `/vens` (403), while `ven-manager` can access `/vens` but NOT `/programs`/`/events`. The BFF uses two separate VtnClient instances with independent OAuth tokens to cover all endpoints.

### 10. VTN Web UI — Deployed and Running

**Status: COMPLETE**

React + TypeScript SPA served by nginx on port 8080, with nginx proxying `/api/` to the BFF:

| Container | Image | Status | Port |
|-----------|-------|--------|------|
| `vtn-ui-1` | vtn-ui (node build + nginx) | running | 8080 |

**What was done:**
- Created full Vite build infrastructure mirroring VEN UI patterns
- `BffApi` class with 4 methods (health, programs, events, vens)
- 4 react-query hooks with appropriate polling intervals (10-30s)
- `BffContext` provider (simpler than VEN's — no VEN selector, single BFF)
- nginx reverse proxy: `/api/*` → `bff:8090`, everything else → SPA
- 4 pages: Dashboard (summary cards with VTN health), Programs (searchable list with JSON dialog), Events (searchable table with JSON dialog), VENs (searchable list with JSON dialog)
- 26 component tests across 5 test files (all passing)
- Multi-stage Docker build (node:20-alpine + nginx:alpine)

---

## What To Do Next

Based on the system design's implementation order (Section 19) and current state:

### ~~Phase 1: Complete the VEN Application~~ DONE
### ~~Phase 2: VEN Web UI~~ DONE

### ~~Phase 3: Seed VTN with Programs & Events~~ DONE

### ~~Phase 4: VTN BFF + VTN Web UI~~ DONE

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
├── ven-ui-1           [react+nginx]            :8084  RUNNING
│
├── vtn-bff-1          [rust axum BFF]           :8090  RUNNING
└── vtn-ui-1           [react+nginx]            :8080  RUNNING
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

## Phase 4 Work Log: VEN Web UI (2026-02-06)

### From Scaffold to Buildable App

The VEN UI had been scaffolded (App.tsx, 4 pages, API client, usePoll hook, JsonDialog) but was not buildable — no `package.json`, no Vite config, no `index.html`, no entry point.

### Key Architecture Changes

1. **Replaced `usePoll` with `@tanstack/react-query`**: Per `REACT_GUIDELINES.md`, switched from manual polling + `useState` to `useQuery` with `refetchInterval`. Each page now fetches its own data — App.tsx no longer manages all state centrally.

2. **Created `VenContext`**: Stores `{ venUrl, setVenUrl, api }`. Changing `venUrl` in the selector invalidates all queries via `queryClient.invalidateQueries()`.

3. **Moved types**: `datamodel.ts` → `api/types.ts`, changed `raw: any` to `raw: unknown` for type safety.

4. **Smart `role` attributes**: Initially added `role` to every interactive element per the guidelines. Then updated the guidelines themselves to note that MUI provides native roles (dialog, button, combobox, table, list, etc.) — removed 27 redundant `role` attributes, kept only `role="status"` and `role="alert"` where Typography lacks semantic meaning.

### Vite Build on Windows Subst Drives

Hit a Vite build error: `The "fileName" properties of emitted chunks must not be absolute paths, received "C:/DriveD/..."`. Root cause: `D:` drive is a Windows `subst` of `C:\DriveD`, and Vite resolves the real path internally causing a mismatch. Fixed by building from the real path. Not an issue in Docker (Linux).

### React Guidelines Improvements

Updated `REACT_GUIDELINES.md` (renamed from `ReactCodingGuideLines.md`):
- Unified component signature style: plain `function` (not `FC`)
- Updated `role` guidance: only add when component doesn't provide natively
- Replaced Cypress assertion examples with Testing Library/Vitest
- Added Vitest + Testing Library setup section
- Updated react-query examples to v5 object syntax
- Marked auth/token sections as reference material

### Docker Build Performance

VEN UI builds fast on the Pi (~33s total):
- `npm ci`: ~34s (237 packages)
- `tsc + vite build`: ~33s (963 modules)
- nginx image layer: instant

Much faster than the Rust VEN (~11 min) or VTN (~25 min) builds.

---

## Phase 5 Work Log: Seed VTN with Demo Data (2026-02-07)

### Approach

Created a standalone Python script (`scripts/seed_vtn.py`) rather than ad-hoc curl commands. This makes seeding repeatable and documentable. The script reuses the same API patterns proven in the integration test suite (`tests/features/helpers/api_client.py`).

### Idempotency

The script lists existing programs before creating new ones. If a program with the same `programName` already exists, it's skipped. Events are always created (the VTN doesn't enforce unique event names), so re-running the script adds duplicate events. This is acceptable for a demo environment.

### Verification

After seeding, confirmed:
- VTN shows 4 programs (3 new + 1 "Test DR Program" from earlier integration testing)
- All events visible on VTN
- Events propagated to all 3 VENs within their 30s event poll interval
- Programs propagate within the 300s program poll interval
- VEN Web UI (port 8084) displays the data

---

## Phase 6 Work Log: VTN BFF + VTN Web UI (2026-02-07)

### Dual Credential Discovery

The plan assumed `ven-manager` could access `/programs`, `/events`, AND `/vens`. In practice, the VTN's role-based access is stricter:
- `any-business` → `/programs`, `/events` (but 403 on `/vens`)
- `ven-manager` → `/vens` (but empty arrays from `/programs`, `/events`)

Fixed by giving the BFF two VtnClient instances (`business` and `ven_mgr`), each with its own OAuth token. Programs and events route through `business`, VENs route through `ven_mgr`.

### BFF Build Performance

First build on Pi4: ~11 min (deps cached from VEN build sharing the same base image). Cached rebuilds (source-only changes): ~1 min.

### Port Conflicts

Both port 8090 (BFF) and 8080 (UI) were occupied by unrelated containers (`dokuwiki` and `data_acquisition`). Stopped them before starting the new services.

### VTN UI Architecture

Follows the same patterns as the VEN UI but simpler:
- No VEN selector (single BFF target)
- `BffApi` uses empty `baseUrl` — all `/api/*` calls are same-origin, proxied by nginx
- VTN's native field names used throughout: `programName`, `programID`, `eventName`, `venName`, `createdDateTime`

### Windows Subst Drive Issue (Again)

Vitest failed when run from `D:\Tinker\...` (subst drive) because Vite resolves to the real path `C:\DriveD\Tinker\...`. The `setupFiles` path couldn't be found. Fix: removed `root: resolve(__dirname)` from `vite.config.ts` and run tests from the real path. Updated auto-memory with detailed notes to prevent recurrence.

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
- MUI components provide native ARIA roles — don't duplicate them (e.g. `<Button>` already has `role="button"`)
- Use `role="status"` and `role="alert"` on `<Typography>` for screen reader announcements — these are semantic roles the element doesn't have natively
- Windows `subst` drives cause Vite build failures — Vite resolves to real path internally, creating mismatches. Build from real path or in Docker
- React Query `refetchInterval` is a cleaner replacement for manual `setInterval` polling — handles loading/error states, caching, and query invalidation
- VEN UI Docker build (~33s) is dramatically faster than Rust builds (~11-25 min) since it's just npm + Vite bundling
- `React.FC` is discouraged — use plain `function` with typed props for cleaner, more explicit component signatures
- VTN role-based access is per-endpoint: `any-business` sees programs/events, `ven-manager` sees VENs — a BFF needing all three must use multiple credentials
- nginx reverse proxy (`proxy_pass`) eliminates CORS issues — the browser sees same-origin `/api/` calls
- BFF TTL cache (HashMap + Instant + Duration) is sufficient for 3-4 entries — no need for an external crate
- Vite `resolve(__dirname)` in `root` config triggers real-path resolution on Windows subst drives — omit `root` entirely

---

*Last updated: 2026-02-07*
