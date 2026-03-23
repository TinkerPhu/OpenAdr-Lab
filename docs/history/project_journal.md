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
| `ven-ven-1-1` | ven-1/ven-1 | 8211 |
| `ven-ven-2-1` | ven-2/ven-2 | 8212 |
| `ven-ven-3-1` | ven-3/ven-3 | 8213 |

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
| `ven-ui-1` | ven-ui (node build + nginx) | running | 8214 |

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
- `src/api/types.ts` — `Program`, `VtnEvent`, `SensorSnapshot` types
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
| `vtn-bff-1` | vtn-bff (rust:1.90-alpine build) | healthy | 8220 |

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
| `vtn-ui-1` | vtn-ui (node build + nginx) | running | 8221 |

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

### ~~Phase 5: Enrollment & Reports (Phase 10)~~ DONE

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
├── vtn-db-1      [postgres:16-alpine]     :8201  RUNNING
├── vtn-vtn-1          [openleadr-rs]           :8200  RUNNING
│
├── ven-ven-1-1        [ven-app]                :8211  RUNNING
├── ven-ven-2-1        [ven-app]                :8212  RUNNING
├── ven-ven-3-1        [ven-app]                :8213  RUNNING
│
├── ven-ui-1           [react+nginx]            :8214  RUNNING
│
├── vtn-bff-1          [rust axum BFF]          :8220  RUNNING
└── vtn-ui-1           [react+nginx]            :8221  RUNNING
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

## Phase 7 Work Log: Port Remapping to 8200 Range (2026-02-07)

### Motivation

Ports 8080 (UI) and 8090 (BFF) conflicted with existing containers (`data_acquisition` and `dokuwiki`) on Pi4. Rather than risk future conflicts, all OpenADR Lab ports were moved to the 8200 range with a clear allocation scheme.

### Port Allocation

| Container | Old Port | New Port |
|-----------|----------|----------|
| vtn-vtn-1 | 3000 | 8200 |
| vtn-db-1 | 5432 | 8201 |
| ven-ven-1-1 | 8081 | 8211 |
| ven-ven-2-1 | 8082 | 8212 |
| ven-ven-3-1 | 8083 | 8213 |
| ven-ui-1 | 8084 | 8214 |
| vtn-bff-1 | 8090 | 8220 |
| vtn-ui-1 | 8080 | 8221 |

### .env Override Pitfall

Docker Compose `${VAR:-default}` syntax in YAML is overridden by `.env` files. The local `.env` and the Pi4's `.env` both had the old port values, silently ignoring the new defaults. Had to update both.

### Hostname Fix

Hardcoded `raspberrypi.local` didn't resolve — Pi4's actual hostname is `pi4server`, so `pi4server.local` works via mDNS/Avahi.

---

## Phase 8 Work Log: Remove VEN DTO Normalization (2026-02-07)

### Motivation

The project rule (CLAUDE.md `dto:` directive) states: "pass through upstream field names across all layers — backend, BFF, UI. One vocabulary everywhere reduces boilerplate and debugging friction." The VEN backend had normalized VTN response fields (`programName` → `name`, `programID` → `program_id`, `createdDateTime` → `created_at`) into Rust structs, then the UI used those snake_case names. The VTN UI already used native field names. This meant two different vocabularies for the same data.

### Changes Made

**VEN Rust Backend:**
- Removed `Program` and `Event` structs from `models.rs` (only `SensorSnapshot` remains — it's locally generated, not from VTN)
- Removed `parse_programs_loose()` and `parse_events_loose()` from `vtn.rs` — `fetch_programs()` and `fetch_events()` now return `Vec<serde_json::Value>` directly
- Updated `state.rs` to store `Vec<serde_json::Value>` instead of typed structs
- `main.rs` handlers unchanged — `Json(ctx.state.programs().await)` passes through raw VTN JSON

**VEN UI (TypeScript):**
- `types.ts`: `name` → `programName`, `program_id` → `programID`, `created_at` → `createdDateTime`, added `eventName`, removed `status`. Renamed `Event` → `VtnEvent` (consistent with VTN UI, avoids DOM `Event` collision). Added `[key: string]: unknown` index signature for pass-through.
- `Events.tsx`: Replaced status filter chips with simple text search (VTN events have no `status` field). Added eventName column. JSON dialog now shows the entire event object (not a nested `raw` field).
- `Dashboard.tsx`, `Programs.tsx`: `p.name` → `p.programName`
- `client.ts`: `Event` → `VtnEvent`

**Tests:**
- All mock data updated to use native field names
- Events test: removed 1 status filter test, added 1 eventName display test
- Integration test `ven_integration_steps.py`: `p.get("name")` → `p.get("programName")`

**Test Results After Changes:**
- VEN UI: 30/30 passed
- VTN UI: 26/26 passed (unchanged, already used native names)
- Integration tests: to be verified after deployment

### Impact

Net deletion: -76 lines. Both UIs now use identical field names (`programName`, `programID`, `eventName`, `createdDateTime`, `venName`). No translation layer between VTN responses and any consumer. Debugging is simpler — the JSON you see in the VTN API is the same JSON everywhere.

---

## Phase 9 Work Log: Testing & Cleanup — Full CRUD (2026-02-07)

### Motivation

After Phases 1–8, the system was functional but had gaps: the VEN sensor POST endpoint rejected partial payloads (422), duplicate events accumulated from re-running the seed script, and both UIs were read-only despite the VTN API supporting full CRUD.

### Sub-task 1: Fix VEN Sensor POST 422

**Root cause**: `post_sensors` deserialized `Json<SensorSnapshot>`, which required `id` (Uuid) and `ts` (DateTime) — fields a form or sensor client shouldn't have to provide.

**Fix**: Added `SensorInput` struct (all optional fields) to `models.rs`. Updated `post_sensors` handler to accept `SensorInput` and build a full `SensorSnapshot` with `Uuid::new_v4()` and `Utc::now()` server-side.

### Sub-task 2: Seed Script Idempotency

**Problem**: `seed_vtn.py` was idempotent for programs (checked by name) but always created events, producing duplicates on re-run.

**Fix**: Added `list_events()` helper. Before creating each event, checks if `(programID, eventName)` already exists — skips with "already exists — skipping" message.

### Sub-task 3: BFF Write Methods

**Problem**: BFF only supported GET and had CORS limited to `Method::GET`.

**Changes**:
- `vtn_client.rs`: Added `post_json()`, `put_json()`, `delete_json()` — all follow the existing 401-retry pattern
- `cache.rs`: Added `invalidate(key)` method
- `main.rs`: Expanded CORS to GET/POST/PUT/DELETE, added 7 new routes
- Route handlers in `programs.rs`, `events.rs`, `vens.rs`: create/update/delete handlers that proxy to VTN and invalidate cache

**Route map**:
| Method | Path | Client | Cache |
|---|---|---|---|
| POST | `/api/programs` | business | invalidate "programs" |
| PUT | `/api/programs/{id}` | business | invalidate "programs" |
| DELETE | `/api/programs/{id}` | business | invalidate "programs" |
| POST | `/api/events` | business | invalidate "events" |
| PUT | `/api/events/{id}` | business | invalidate "events" |
| DELETE | `/api/events/{id}` | business | invalidate "events" |
| DELETE | `/api/vens/{id}` | ven_mgr | invalidate "vens" |

### Sub-task 4: VTN UI CRUD

**New components**:
- `ConfirmDialog.tsx` — reusable delete confirmation dialog
- `ProgramFormDialog.tsx` — create/edit program (name field)
- `EventFormDialog.tsx` — create/edit event (name, program dropdown, intervals JSON)

**API layer**:
- `client.ts`: Added 7 write methods (`createProgram`, `updateProgram`, `deleteProgram`, `createEvent`, `updateEvent`, `deleteEvent`, `deleteVen`)
- `hooks.ts`: Added 7 `useMutation` hooks with `queryClient.invalidateQueries()` on success
- `types.ts`: Added `ProgramInput` and `EventInput` types

**Page updates**:
- Programs: Create button, edit/delete icons per item
- Events: Create button, edit/delete icons per row, Actions column
- VENs: Delete icon per item (no create — provisioning is too complex)

**Test results**: 37/37 passed (was 26/26 — added 11 CRUD tests)

### Sub-task 5: Integration Tests

**Sensor partial POST tests** (`ven_sensors.feature`):
- Added 2 scenarios: temperature-only POST, power-only POST
- Updated existing full-POST test to use `SensorInput` format (no `id`/`ts` fields)

**BFF CRUD tests** (3 new feature files):
- `bff_programs.feature`: create, update, delete programs via BFF (3 scenarios)
- `bff_events.feature`: create, delete events via BFF (2 scenarios)
- `bff_vens.feature`: list VENs, health check (2 scenarios)

**Infrastructure**:
- Added `test-bff` service to `docker-compose.test.yml`
- Added BFF helpers (`bff_get`, `bff_post`, `bff_put`, `bff_delete`) to `api_client.py`
- Updated `environment.py` to wait for BFF health
- Step file `bff_crud_steps.py` reuses shared assertion steps from `vtn_auth_steps.py` and `vtn_programs_steps.py`

---

## Phase 10 Work Log: Enrollment & Reports (2026-02-07)

### Motivation

Both UIs displayed all Programs and Events identically regardless of which VEN was viewing them. In real OpenADR, a VTN **enrolls** specific VENs in specific Programs via `targets` with `VEN_NAME`. The VTN (openleadr-rs) already implements this filtering server-side — we just needed UI + BFF + VEN layers to expose it. Additionally, the VTN's report system (POST/GET/DELETE /reports) was unused.

### Sub-phase 10a: Enrollment — Seed + VTN UI

**Seed script** (`scripts/seed_vtn.py`):
- Added `programLongName`, `programType`, and `targets` to PROGRAMS data
- Enrollment map: "Summer Peak DR" → ven-1, ven-2 | "EV Managed Charging" → ven-2, ven-3 | "HVAC Optimization" → no targets (open)
- Added `update_program()` function to PUT targets onto existing programs (idempotent re-runs)

**VTN UI**:
- Extended `Program` type with `programLongName`, `programType`, `targets`; added `TargetEntry` type
- `ProgramFormDialog` gained `programLongName`, `programType` text fields and VEN enrollment multi-select (checkboxes)
- Programs page shows enrolled VEN names as Chips (or "Open — all VENs")
- VENs page cross-references program targets to show enrolled programs per VEN
- 39/39 tests passing

**Key insight**: Programs without `targets` are visible to **all** VENs (open programs). Programs with `targets: [{type: "VEN_NAME", values: [...]}]` are visible only to enrolled VENs. This natural "available vs enrolled" distinction requires no extra endpoints.

### Sub-phase 10b: Reports — VTN BFF + VTN UI

**BFF** (`VTN/bff/src/routes/reports.rs`):
- `GET /api/reports` — cached proxy (10s TTL) via `any-business` credential
- `DELETE /api/reports/:id` — proxy with cache invalidation
- No POST — only VENs (with VEN credentials) can create reports

**VTN UI**:
- Reports page with table (clientName, reportName, program, event, created), search, JsonDialog, delete with ConfirmDialog
- Dashboard reports count card, nav link
- 47/47 tests passing (6 files)

### Sub-phase 10c: Reports — VEN Backend

**VtnClient** (`VEN/src/vtn.rs`):
- Added `post_json()` with 401-retry pattern (same as `get_json`)
- Added `fetch_reports()` and `submit_report(body)` methods

**AppState/AppCtx** (`VEN/src/state.rs`, `main.rs`):
- Added `reports: Vec<serde_json::Value>` to state
- Added reports polling loop (configurable interval, default 60s)
- VtnClient stored in AppCtx for POST forwarding
- Routes: `GET /reports` (cached), `POST /reports` (forward to VTN, return 201)

### Sub-phase 10d: VEN UI Enhancements

- Programs page shows `programLongName` and `programType` as secondary text
- Events page resolves `programID` → `programName` via lookup map
- New Reports page: table of existing reports, "Submit Report" form with event dropdown, auto-populated programID from selected event, clientName from VEN context
- Dashboard reports count card
- `venName` added to VEN context for report clientName
- 30/30 tests passing (6 files)

### Sub-phase 10e: Integration Tests

**New feature files**:
- `enrollment.feature` (2 scenarios): open program visible to all VENs, targeted program visible only to enrolled VEN
- `bff_reports.feature` (1 scenario): list reports via BFF returns JSON array
- `ven_reports.feature` (1 scenario): submit report via VEN-1, verify round-trip through VEN and BFF

**Infrastructure changes**:
- Added `test-ven-2` service to `docker-compose.test.yml` (needed for enrollment tests)
- Added `provision_ven2.py` — provisions ven-2 user/credentials/VEN entity via API (idempotent)
- Updated entrypoint to run provisioning after fixtures
- Added `VEN2_BASE_URL` to api_client.py and environment.py

### Issues and Learnings

- `targets` wire format is `[{type: "VEN_NAME", values: [...]}]` — array of objects, not an object map
- VTN `POST /reports` returns **201**, not 200 — VEN backend must forward this status
- BFF report cache won't auto-invalidate when VENs POST reports — relies on short TTL (10s)
- Test fixtures only include ven-1 — ven-2 must be provisioned via API in entrypoint
- VTN POST /reports requires VEN role — business credentials get 403

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
- Avoid DTO normalization across layers — pass through upstream field names (e.g. VTN's `programName`, `programID`) as-is. One vocabulary reduces code, boilerplate, and debugging friction
- Docker Compose `.env` files silently override `${VAR:-default}` in YAML — always check for stale `.env` values after changing defaults
- When multiple containers on a shared host need ports, pick a dedicated range (e.g. 82xx) to avoid conflicts with existing services

- OpenADR enrollment via `targets` is a first-class VTN feature — no custom endpoints needed. Programs without targets are "open" (all VENs see them)
- VTN POST /reports requires VEN role — a BFF with business credentials cannot create reports on behalf of VENs
- When adding a second VEN to the test stack, all credentials must be provisioned via API since fixture SQL only covers ven-1
- Axum 0.7 path params use `:id` syntax — `{id}` is axum 0.8+ and silently returns 404

---

## Phase 11 Work Log: Use Case Readiness (2026-02-08)

### Motivation

The `use_cases.md` defines 8 real-world OpenADR scenarios, but seed data was trivial (single SIMPLE intervals, no timing/priority), VTN UI couldn't create realistic events (no priority/intervalPeriod/targets fields), and VEN UI only showed name/program/date (no payload interpretation, no status).

### Changes Made

**Phase 1: Realistic Seed Data** (`scripts/seed_vtn.py`)
- Rewrote EVENTS from 6 generic events to 8 use-case-specific events
- Each event uses appropriate payload type: SIMPLE, EXPORT_CAPACITY_LIMIT, PRICE, IMPORT_CAPACITY_LIMIT, CHARGE_STATE_SETPOINT
- Events include priority (0=emergency to 5=low), intervalPeriod (start+duration), targets, and multi-interval structures (up to 24 for pricing)
- Added `--demo-cancel` flag for UC8 (creates event, waits 5s, deletes it)
- Events are timestamped relative to `now` for realistic lifecycle display

**Phase 2: VTN UI Event Form + Table** (`VTN/ui/`)
- Added `IntervalPeriod` type, extended `VtnEvent` and `EventInput` with `priority`, `intervalPeriod`, `targets`
- `EventFormDialog`: added priority number input, start time + duration fields, targets JSON textarea
- Events table: added Priority and Start columns (from 4 to 6 columns)
- 50/50 tests passing (+1 new test for priority/start columns)

**Phase 3: VEN UI Table Columns** (`VEN/ui/`)
- Added typed `Interval`, `IntervalPeriod`, `TargetEntry` types alongside catch-all
- Events table expanded from 3 to 8 columns: Name, Program, Priority, Payload Type, Intervals, Status, Start, Created
- `getPayloadType()` extracts `intervals[0].payloads[0].type`

**Phase 4: VEN UI Event Detail Panel** (`VEN/ui/src/components/EventDetailPanel.tsx`)
- New component replacing JsonDialog for event inspection
- Shows: event name with status chip + priority badge, program name, start/duration, targets as chips, intervals table (ID, Start, Duration, Payload with human-readable labels), raw JSON collapsible
- Payload type labels map: SIMPLE→"Simple Signal", PRICE→"Price Signal", etc.

**Phase 5: VEN UI Event Status** (`VEN/ui/src/utils/eventStatus.ts`)
- Pure function `getEventStatus(event, now?)` → "scheduled" | "active" | "completed" | "no timing"
- Parses ISO 8601 durations (PnDTnHnMnS)
- Status chips: green=active, blue=scheduled, grey=completed, yellow=no timing
- 14 unit tests for status derivation + color mapping
- 45/45 VEN UI tests passing

**Phase 6: Cancellation Documentation** (`use_cases.md`)
- Documented that OpenADR 3 cancellation = DELETE (no cancel status field)
- Added demo command example

**Phase 7: Integration Tests** (`tests/features/use_cases.feature`)
- 8 scenarios covering all use cases: SIMPLE+priority, EXPORT_CAPACITY_LIMIT+multi-interval, PRICE, IMPORT_CAPACITY_LIMIT+intervalPeriod, IMPORT_CAPACITY_LIMIT+targets, CHARGE_STATE_SETPOINT, SIMPLE no-op, DELETE cancellation
- Added `vtn_delete()` helper to api_client.py
- Step definitions in `use_case_steps.py` verify payload types, priorities, interval counts, intervalPeriod, targets, and deletion

### Key Insight

**Zero backend/BFF changes needed.** The VTN API already supported all fields (priority, targets, intervalPeriod, all payload types). The BFF is a transparent JSON proxy. The VEN stores raw JSON. All work was purely in seed data, UI forms, UI display, and tests. This validates the "pass-through DTO" architecture — adding new event complexity was a UI-only change.

### Deployment Required

After deployment:
1. Re-run seed script to create new events: `python3 seed_vtn.py --vtn-url http://Pi4-Server:8200`
2. Rebuild VEN UI: `docker compose up -d --build ui` (in VEN/)
3. Rebuild VTN UI: `docker compose up -d --build ui` (in VTN/)
4. No BFF or VEN backend rebuilds needed

---

## Phase 11b Work Log: Full E2E Use Case Tests (2026-02-08)

### Motivation

The Phase 11 use case integration tests only verified that the VTN accepted event payloads with the right types. They didn't test what makes each use case meaningful: enrollment targeting (right VEN sees the event, wrong VEN doesn't), event propagation through VENs, report submission round-trips, and event cancellation visibility.

### Changes Made

**`tests/features/helpers/api_client.py`**:
- Added `ven2_get()` and `ven2_post()` helpers for consistent VEN-2 HTTP access (previously done inline via raw `requests.get` in enrollment steps)

**`tests/features/use_cases.feature`** — full rewrite:
- Each of the 8 scenarios now follows the complete flow:
  1. Create program with enrollment targets (single VEN, dual VEN, or open)
  2. Create event with UC-specific payload type, priority, and interval count
  3. Wait for enrolled VEN(s) to receive the event by name (30s poll)
  4. Verify non-enrolled VEN(s) do NOT see the event
  5. Verify event structure on VEN side (payload type, priority, interval count, intervalPeriod)
  6. VEN submits report for the event
  7. Verify report visible on VTN
  8. (UC8) Delete event → verify VEN no longer sees it

**`tests/features/steps/use_case_steps.py`** — full rewrite:
- Program creation steps: single-target, dual-target ("targeting both"), open — all save program ID
- Event creation steps: with priority + interval count, with intervalPeriod, with targets
- VEN polling steps: wait for VEN-1/VEN-2 to show event by name
- Negative assertions: VEN-1/VEN-2 does not have event
- VEN-side structure checks: payload type, priority, interval count, intervalPeriod
- Report submission via VEN-1/VEN-2 for a specific event
- Report verification on VTN by clientName + eventID
- Event deletion by name and cancellation detection (event disappears from VEN)

**`tests/provision_ven2.py`**:
- Fixed `PUT /users/{id}` to include required `reference` and `description` fields (VTN API requires full body on PUT)

### Test Results

**13 features, 33 scenarios, 171 steps — all passing** (59 seconds).

The 8 use case scenarios went from verifying only VTN response shapes to testing the full lifecycle across VTN → VEN → VEN report → VTN report visibility.

### Issues Encountered

1. **Behave AmbiguousStep**: The step `I create a program "{name}" targeting "{ven1}" and "{ven2}" and save its ID` was matched by `I create a program "{name}" targeting "{ven}" and save its ID` (behave's `{...}` captures greedily). Fixed by using `"targeting both"` for the dual-target variant.

2. **provision_ven2.py 400 error**: The VTN's `PUT /users/{id}` endpoint changed to require the full user body (`reference`, `description`, `roles`) — not just `roles`. This was a pre-existing issue masked by the test stack not being rebuilt recently.

### Key Insight

The test infrastructure already had all building blocks (2 VENs with 5s poll, `poll_until`, report submission, enrollment helpers). Extending the tests was purely wiring — no new infrastructure needed.

---

## Phase 11c Work Log: openleadr-rs as Git Submodule (2026-02-08)

### Motivation

The `openleadr-rs` directory was a manually-cloned third-party repo excluded via `.gitignore`. Anyone cloning the project had to know to also clone `openleadr-rs` separately — nothing in the repo itself indicated this dependency or which commit to use. A git submodule makes `git clone --recursive` produce a ready-to-build repo.

### Changes Made

1. Removed the `openleadr-rs/` entry from `.gitignore`
2. Deleted the existing standalone clone
3. Added `openleadr-rs` as a git submodule (pinned at commit `606dfb2`)
4. Forked `OpenLEADR/openleadr-rs` → `TinkerPhu/openleadr-rs` via GitHub API
5. Updated the submodule URL to point to the fork
6. Added `upstream` remote inside the submodule for syncing with the original

### Syncing with Upstream

To pull in updates from OpenLEADR:
```bash
cd openleadr-rs
git fetch upstream
git merge upstream/main
git push origin main
cd ..
git add openleadr-rs
git commit -m "Update openleadr-rs submodule to latest upstream"
```
Or use GitHub's "Sync fork" button, then `git submodule update --remote` locally.

### Deployment Note

On Pi4-Server (or any existing clone), a one-time init is needed after pulling:
```bash
git pull
git submodule update --init --recursive
```
Subsequent `git pull` + `git submodule update` keeps it in sync.

### Key Insight

Forking before submodule-ing means we can patch `openleadr-rs` if needed (bug fixes, custom behavior) without waiting for upstream merges, while still easily pulling upstream updates.

---

## Phase 12: Suggest Example Button + Duplicate Reports Fix

### What We Did

1. **VEN UI "Suggest Example" button** — Added a `buildExampleResources(event, venName)` function and a "Suggest Example" button to the Reports form (`VEN/ui/src/pages/Reports.tsx`). When clicked, it reads the selected event's `intervals`, generates a matching `resources` array with `resourceName: "{venName}-meter"`, and auto-fills the `reportName`. For `SIMPLE` payloads with value `0`, suggests `1` (acknowledged). For other non-zero values, applies ±4% random offset to simulate real measurements.

2. **Duplicate reports bug fix in openleadr-rs** — Discovered that the VTN's `GET /reports` endpoint returned duplicate rows when a program had multiple VEN enrollments. Root cause: the `retrieve` and `retrieve_all` SQL queries in `openleadr-vtn/src/data_source/postgres/report.rs` used `LEFT JOIN ven_program` for permission filtering but didn't use `DISTINCT`. A program with 2 VEN enrollments (e.g., Summer Peak DR targeting ven-1 and ven-2) produced 2 identical rows per report. Fixed by adding `SELECT DISTINCT r.*` to both queries.

### Why

- Users had no way to know the OpenADR 3 report resource schema, making it impossible to create meaningful reports without consulting documentation.
- The duplicate report rows were confusing — the VTN UI showed 2 identical entries for a single submitted report.

### Key Learnings

- **SQLx offline cache hashes are SHA-256 of the exact query string** between `r#"` and `"#` in the Rust source. Whitespace (including trailing spaces) matters. When modifying queries, the `.sqlx/query-{hash}.json` files must be renamed to match the new hash, and the `hash` field and `query` field inside must also be updated.
- **The `ven_program` JOIN is the root cause** — it's used for permission filtering (ensuring VENs only see reports for programs they're enrolled in), but it multiplies rows when a program has multiple enrollments. `DISTINCT` is the correct fix since `r.*` columns are identical across the joined rows.

---

### Phase 12: Report Upsert, Edit Button & Own-Reports Filter

**Status: COMPLETE**

### What

Three related improvements to VEN report handling:

1. **Own-reports filter** — VEN backend now calls `GET /reports?clientName={ven_name}` instead of `GET /reports`, so each VEN only sees its own reports in the UI.
2. **Upsert on POST** — When VTN returns 409 Conflict (duplicate `reportName`), the VEN backend automatically finds the existing report by name and issues `PUT /reports/{id}` instead. This makes report submission idempotent by name.
3. **Edit button in VEN UI** — Each report row has an Edit icon button. Clicking it opens the form in edit mode with fields pre-populated. Submit calls `PUT /reports/{id}` directly.

### Changes

| File | Change |
|------|--------|
| `VEN/src/vtn.rs` | Added `ven_name` field, `put_json()`, `post_json_raw()`, `upsert_report()`, `update_report()`, `find_report_by_name()`; `fetch_reports()` now filters by `clientName` |
| `VEN/src/main.rs` | Passed `ven_name` to VtnClient, added `Method::PUT` to CORS, `/reports/:id` PUT route, `put_report` handler, changed `post_reports` to use `upsert_report()` |
| `VEN/ui/src/api/client.ts` | Added `updateReport(id, payload)` method |
| `VEN/ui/src/api/hooks.ts` | Added `useUpdateReport()` mutation hook |
| `VEN/ui/src/pages/Reports.tsx` | Added edit mode state, Edit icon button per row, form title/button toggling, update mutation call |
| `VEN/ui/src/__tests__/Reports.test.tsx` | Added 3 tests for edit mode (renders Edit button, populates form, calls update mutation) |

### Why

- VENs seeing other VENs' reports was confusing and a privacy concern — each VEN should only see its own data.
- 409 Conflict on duplicate report names blocked users from correcting reports — upsert makes it seamless.
- No edit capability meant users had to delete and recreate reports to fix mistakes.

### Key Learnings

- VTN already supports `?clientName=X` query parameter filtering on `GET /reports` — no VTN changes needed.
- The upsert pattern (POST → 409 → find by name → PUT) keeps the UI simple — the POST endpoint handles both create and update transparently.
- `post_json_raw()` (returning status + body text) was needed to detect 409 without the existing `post_json()` error-mapping eating the status code.

---

### Upstream Contributions: openleadr-rs Pull Requests

**Status: IN PROGRESS**

### What

Prepared the TinkerPhu/openleadr-rs fork for upstream pull requests. Each distinct fix/change gets its own branch based on `upstream/main` with the relevant commit(s) cherry-picked, keeping PRs atomic and reviewable.

### PR Workflow

1. Develop and test fix on `main` in the submodule (as part of normal lab work)
2. Create a topic branch from `upstream/main`: `git checkout -b fix/<topic> upstream/main`
3. Cherry-pick the relevant commit(s)
4. Push to origin: `git push origin fix/<topic>`
5. Create PR via `gh pr create --repo OpenLEADR/openleadr-rs --head TinkerPhu:fix/<topic> --base main`
6. Switch back to `main`

### Submitted PRs

| PR | Branch | Description |
|----|--------|-------------|
| [#357](https://github.com/OpenLEADR/openleadr-rs/pull/357) | `fix/duplicate-report-rows` | Add `DISTINCT` to report queries to prevent duplicate rows caused by `ven_program` JOIN with multiple enrollments |

### Infrastructure

- Installed GitHub CLI (`gh` v2.86.0) for creating PRs from the terminal
- `gh auth login` authenticated via browser with scopes: `gist`, `read:org`, `repo`, `workflow`

---

## Phase 12: VEN Report Isolation (Security Fix)

**Status: COMPLETE**

### Problem
Report queries used `ven_program` (enrollment) table for access control. This meant VENs enrolled in the same program could see each other's reports — a data isolation violation. For example, if VEN-1 and VEN-2 were both enrolled in "Summer Peak DR", VEN-1 could see VEN-2's reports.

### Solution
Added a `ven_id` column to the `report` table that tracks which VEN created each report. Changed all report queries (retrieve, retrieve_all, update) to filter by `r.ven_id = ANY(user_ven_ids)` for VEN users, replacing the old `ven_program` JOIN approach.

### Changes Made

1. **New migration** (`migrations/20260208000000_report_add_ven_id.sql`):
   - `ALTER TABLE report ADD COLUMN ven_id text REFERENCES ven(id)`
   - Backfill existing reports by matching `client_name` to `ven_name`

2. **Report Rust code** (`openleadr-vtn/src/data_source/postgres/report.rs`):
   - Added `ven_id: Option<String>` to `PostgresReport` struct
   - `create()`: Stores the authenticated VEN's ID in the new column
   - `retrieve()`, `retrieve_all()`, `update()`: Replaced `LEFT JOIN ven_program` with `r.ven_id = ANY($user_ven_ids)` for VEN users
   - Business users unchanged — they still see reports scoped to their programs

3. **SQLx offline cache**: Updated all 5 report query cache files with new column, new queries, and recomputed SHA-256 hashes

### Key Learning
- Program enrollment (`ven_program`) is appropriate for controlling which programs/events a VEN can see — those are shared resources
- Reports are VEN-private data and require direct ownership tracking (`ven_id`), not enrollment-based access

---

## Phase 12b: Seed Script Idempotency (2026-02-09)

### What
Made `scripts/seed_vtn.py` fully idempotent — re-running it deletes old seed events and recreates them with fresh timestamps relative to `now`, so events are always "active" after seeding.

### Why
On a fresh clone or after time passes, the seed events had stale timestamps (e.g. "starts in 2 minutes" from days ago). Re-running the old script just skipped existing events, leaving the stale timings. Users cloning the repo need a single command to get realistic, active demo data.

### Changes
- Seed events are now matched by `(programID, eventName)` — only these are deleted and recreated
- User-created events (different names) are never touched
- Reports referencing seed events are deleted first to avoid FK 409 Conflict errors
- Programs are still create-or-update (no deletion needed)

### Key Learnings
- VTN returns **409 Conflict** when deleting events that have associated reports (FK constraint, no `ON DELETE CASCADE`). Must delete reports first, then events.
- **Side effect**: Any user-created reports that reference seed events will be deleted when re-seeding. This is inherent to the approach — seed events are replaced, so their associated reports (including manually created ones) must go too. Users should be aware that reports tied to seed events are ephemeral.

---

## Phase 12: Fix Program Description URL Save + Comprehensive Edit Tests

### What Was Done
1. **Bug fix: Description URL field name mismatch** — The VTN (openleadr-rs) serializes the description URL field as `"URL"` (uppercase, via `#[serde(rename = "URL")]`), but the UI was sending `"url"` (lowercase). This caused a silent save failure: clicking Save on a program edit with a changed Description URL did nothing. Fixed by changing `ProgramDescription` type from `{ url: string }` to `{ URL: string }` and updating all references in `ProgramFormDialog.tsx` and test mocks.

2. **Comprehensive program edit tests** (7 new tests) — Verifies that every editable field in the program form dialog correctly reaches the `updateMock`: programName, programLongName, programType, description URL, clearing description URL, VEN enrollment changes, and clearing all VEN enrollment.

3. **Comprehensive event edit tests** (8 new tests) — Verifies all editable fields in the event form dialog: eventName, priority, start time, duration, intervals (JSON), targets (JSON), and a full create-event test with all fields populated.

### Why
- The Description URL bug was a user-facing regression: edits appeared to succeed (no error shown) but were silently rejected by the VTN due to field name mismatch.
- The new tests ensure all form fields are correctly wired to the mutation payloads, preventing similar regressions for any field.

### Issues / Key Learnings
- **userEvent.type treats `{` as a special key descriptor** — In `@testing-library/user-event`, curly braces are reserved for keyboard shortcuts (e.g., `{Enter}`). To type literal JSON with braces, use `fireEvent.change()` instead of `userEvent.type()`.
- **Program/Event update mutations wrap payload as `{ id, input }`** — Test assertions must match this shape, not just the inner `ProgramInput`/`EventInput`.
- **Mock clearing in beforeEach** — Without `mockClear()`, assertions on `updateMock` accumulate across tests and can match stale calls.

### Files Changed
- `VTN/ui/src/api/types.ts` — `ProgramDescription.url` → `.URL`
- `VTN/ui/src/components/ProgramFormDialog.tsx` — Two references updated
- `VTN/ui/src/__tests__/Programs.test.tsx` — Mock data + 7 new edit tests
- `VTN/ui/src/__tests__/Events.test.tsx` — 8 new edit tests

### Test Results
- 64 tests passing across 6 test files (was 49 tests before)

---

### 12. E2E UI Tests with Playwright + Behave

**Status: COMPLETE**

Added browser-driven end-to-end tests that exercise the full stack: headless Chromium -> nginx -> BFF -> VTN -> PostgreSQL -> VEN polling -> VEN API verification.

**Architecture:**
- New `test-ui` service in docker-compose.test.yml (nginx + React app, proxying `/api/` to test-bff)
- Test runner switched from Alpine to Debian-slim (Playwright needs glibc for Chromium)
- Page-object helper (`ui.py`) using `data-testid` selectors throughout
- 5 UI scenarios (UC1-UC4, UC7) covering: open programs, targeted programs, dual-targeting, multi-interval events, intervalPeriod, report round-trip
- 12 existing API verification steps reused as-is from `use_case_steps.py`

**Files changed:**
- `tests/nginx-test.conf` (new) — nginx config pointing to test-bff
- `tests/Dockerfile` (modified) — Alpine -> Debian-slim, Playwright install
- `tests/requirements.txt` (modified) — added playwright
- `tests/docker-compose.test.yml` (modified) — test-ui service, UI_BASE_URL env
- `tests/features/environment.py` (modified) — browser lifecycle hooks with @ui tag
- `tests/features/helpers/ui.py` (new) — VtnUi page object class
- `tests/features/steps/ui_steps.py` (new) — UI step definitions
- `tests/features/ui_use_cases.feature` (new) — 8 UI scenarios (all use cases)

**Issues & Key Learnings:**
1. **Behave step ambiguity** — `{param}` captures greedily, so `'create a program "{name}" via the UI'` matches `'create a program "{name}" targeting "{ven}" via the UI'`. Fix: use `use_step_matcher("re")` with `[^"]+` capture groups for targeted variants.
2. **Feature-level @ui tag** — Behave's `scenario.tags` only includes scenario-level tags, not inherited feature tags. Fixed with helper `_is_ui(scenario)` checking both `scenario.tags` and `scenario.feature.tags`.
3. **Missing VTN token** — UI scenarios reuse API steps (e.g. `the report for event ... appears in VTN`) that need `context.vtn_token`. Fixed by auto-provisioning token in `before_scenario` for UI scenarios.
4. **Playwright on Pi4 ARM64** — Works out of the box with `playwright install chromium --with-deps` on Debian-slim. First build downloads ~300MB (Chromium + dependencies), cached in Docker layers.
5. **MUI Select interaction** — MUI's `<TextField select>` puts `data-testid` on the hidden `<input>`. Playwright clicks the parent div to open the dropdown, then selects `li[role="option"]` by text.

**Test Results:**
- 15 features, 44 scenarios, 299 steps — all passing
- All 8 UI use cases (UC1-UC8) covered: open programs, targeted programs, dual-targeting, multi-interval events, intervalPeriod, event-level targets, battery dispatch, event cancellation via UI delete, report round-trip
- UI tests add ~75s to the test run (total 2m15s vs ~1m for API-only)

### 13. Upstream Pull Requests — Contributing Back to openleadr-rs

**Status: IN PROGRESS**

Submitted two pull requests to the upstream `OpenLEADR/openleadr-rs` repository from our fork (`TinkerPhu/openleadr-rs`):

**PR #357 — Fix duplicate reports caused by ven_program JOIN** (`fix/duplicate-report-rows`)
- Added `DISTINCT` to report retrieve and retrieve_all queries to prevent duplicate rows when a program has multiple VEN enrollments
- Coverage went up from 80.72% to 81.25%

**PR #365 — Fix VEN report isolation: add ven_id ownership tracking** (`fix/report-ven-isolation`)
- Replaces PR #359 which was incorrectly pushed from `main`
- Security fix: VENs enrolled in the same program could previously see each other's reports
- Adds `ven_id` column to report table with FK to ven, backfills via migration
- All report CRUD queries filter by `r.ven_id` instead of joining through `ven_program`

**What was done:**
1. Rebased both branches onto latest `upstream/main`
2. Added `Signed-off-by` lines (DCO requirement) using GitHub noreply email
3. Fixed Clippy warning: annotated unused `ven_id` field in `PostgresReport` with `#[allow(unused)]`
4. Closed PR #359 (was on `main` branch — bad practice) and reopened as PR #365 from a proper feature branch
5. Reset fork's `main` to match upstream (force-push) to clean up divergence
6. Updated submodule reference in main project to point to clean upstream `main`

**Issues & Key Learnings:**
1. **Never push PRs from `main`** — always use feature branches. Pushing from `main` causes the fork to diverge from upstream, making future syncs messy. PR #359 had to be closed and recreated as PR #365 because GitHub doesn't allow changing a PR's head branch.
2. **Signed-off-by (DCO)** — many open-source projects require `git commit --signoff` to certify you have the right to submit the code. Use `--author="Name <email>"` to control what appears publicly.
3. **GitHub noreply email** — use `username@users.noreply.github.com` to keep your private email out of public commit history while satisfying DCO requirements.
4. **SQLx hash verification** — when creating .sqlx cache files on Windows, always verify hashes account for CRLF→LF conversion (GitHub CI runs on Linux). We confirmed hashes matched by converting CRLF to LF before hashing.
5. **Cherry-pick conflicts** — commits built on top of each other can't be cleanly cherry-picked individually. Better to apply the combined diff manually and create a single clean commit.
6. **GitHub can't change PR head branch** — if a PR is on the wrong branch, you must close and recreate it. Leave a comment explaining why so the maintainer understands.

### 14. Use Case Manual & Extended E2E Coverage

**Status: COMPLETE**

Created `USE-CASE-MANUAL.md` — a step-by-step replay guide for all 8 use cases with real-world motivations, concrete examples, and exact curl commands. Then extended the E2E test suite to achieve full coverage of every "What to test" criterion from `USE-CASES.md`.

**5 new scenarios added:**

| Scenario | UC Gap Closed | What It Tests |
|---|---|---|
| UC3b | Large interval counts | 24 hourly price intervals delivered intact |
| UC3c | Late updates/corrections | Price correction via PUT, VEN picks up new value |
| UC4b | Event modification | Peak shaving limit modified mid-flight |
| UC5b | Overlapping events | Two concurrent events with different priorities |
| UC6b | Conflicting state requests | Simultaneous charge (+80) and discharge (-50) events |

**Test Results:** 15 features, 49 scenarios, 348 steps — all passing (2m50s)

**Files changed:**
- `USE-CASE-MANUAL.md` (new) — replay guide with coverage analysis
- `tests/features/helpers/api_client.py` — added `vtn_put` helper
- `tests/features/steps/use_case_steps.py` — new steps for event update, poll-for-value, create-with-value, VEN-2 priority, event count by prefix; extended `_build_intervals` for 24h pricing
- `tests/features/use_cases.feature` — 5 new scenarios

---

### 15. CI Fixes + Failure Recovery Tests

**Status: COMPLETE**

**Problem:** GitHub Actions CI run failed with 3 scenarios:
- 2 VEN isolation report tests fail because the upstream openleadr-rs lacks our `ven_id` fix (PR #365 pending)
- 1 UI test (`UC7 report visibility`) failed due to timing — reports page loads data once and doesn't auto-refresh

**CI Fixes:**
- Tagged 2 report-isolation scenarios as `@upstream_pending` in `ven_isolation.feature`
- Added `tags = ~@upstream_pending ~@resilience` to `behave.ini` so CI skips them by default
- Fixed `report_visible()` in `tests/features/helpers/ui.py`: added page reload retry (if first `wait_for_selector` fails, reload and retry once = 20s total)

**Failure Recovery Tests (System Design §20-21):**

Two complementary approaches:

1. **Behave resilience feature** (`tests/features/ven_resilience.feature`) — 4 scenarios tagged `@resilience`:
   - VEN retains cached events when VTN is stopped
   - VEN re-syncs new events after VTN restart
   - Both VENs converge after VTN restart
   - VEN recovers after its own restart

   Infrastructure: Docker socket mounted into test-runner container, `docker.io` CLI added to Dockerfile. Steps use `docker compose stop/start/restart` to control services. Cleanup in `after_scenario` hook restarts any stopped services.

2. **Standalone script** (`tests/failure_recovery_test.sh`) — bash script for manual testing on Pi4:
   - VTN outage → VEN cache retention
   - VTN restart → VEN re-sync
   - VEN restart → event recovery
   - DB restart → VTN recovery

**CI Integration:** Added `resilience` job to `.github/workflows/e2e-tests.yml` that runs after the main `e2e` job, executing `--tags=@resilience` which overrides the ini exclusion.

**Files created/modified:**
- `tests/features/ven_isolation.feature` — `@upstream_pending` tags on 2 scenarios
- `tests/behave.ini` — tag exclusions
- `tests/features/helpers/ui.py` — `report_visible()` retry
- `tests/features/ven_resilience.feature` — new: 4 resilience scenarios
- `tests/features/steps/resilience_steps.py` — new: step definitions
- `tests/features/helpers/docker_ctl.py` — new: Docker compose control helper
- `tests/features/environment.py` — cleanup hook for stopped services
- `tests/Dockerfile` — added `docker.io` package
- `tests/docker-compose.test.yml` — Docker socket mount
- `tests/failure_recovery_test.sh` — new: standalone test script
- `.github/workflows/e2e-tests.yml` — added resilience job

---

### 15. Observability — Structured JSON Logging, Metrics & Correlation IDs

**Status: COMPLETE**

**What:** Added production-grade observability to VEN and BFF services: structured JSON logs, Prometheus metrics endpoints, request tracing middleware with correlation IDs propagated from UI through BFF to VTN.

**Why:** System Design Section 14 requires structured logging, metrics, and request tracing. Plaintext logs are hard to parse in production. Correlation IDs let operators trace a single user action across all services.

**Changes:**

1. **Structured JSON Logging (VEN + BFF)**
   - Switched `tracing_subscriber::fmt()` to `.json()` in both services
   - Added `json` feature to `tracing-subscriber` in both `Cargo.toml` files
   - Added structured fields (`resource`, `count`) to VEN poll-loop log messages

2. **Request Tracing Middleware (BFF)**
   - Added `tower-http` features: `trace`, `request-id`, `propagate-header`
   - Installed `SetRequestIdLayer` → `TraceLayer` → `PropagateRequestIdLayer` middleware stack
   - Generates `X-Request-ID` UUID if not present in incoming request
   - Copies `X-Request-ID` to response headers
   - `TraceLayer` logs method, path, status, latency per request

3. **Request Tracing (VEN)**
   - Added `TraceLayer::new_for_http()` to VEN's router

4. **X-Request-ID Propagation (BFF → VTN)**
   - Added `request_id: Option<&str>` parameter to all `VtnClient` methods (`get_json`, `post_json`, `put_json`, `delete_json`)
   - Helper `apply_request_id()` conditionally sets the header on outgoing reqwest requests
   - All route handlers extract `X-Request-ID` from incoming `HeaderMap` and pass it through
   - Added `request_id()` helper in `routes/mod.rs`

5. **Prometheus Metrics (VEN)**
   - Added `metrics` + `metrics-exporter-prometheus` crates
   - Installed `PrometheusBuilder` recorder at startup
   - `/metrics` route serves Prometheus text format
   - Poll loops instrumented: `poll_success_total{resource}`, `poll_error_total{resource}`
   - Report submission instrumented: `reports_sent_total`

6. **Prometheus Metrics (BFF)**
   - Same crates, same recorder setup
   - `/api/metrics` route serves Prometheus text format
   - Axum middleware records `http_requests_total{method,path,status}` and `http_request_duration_seconds{method,path}` for every request

7. **UI Correlation IDs (VTN UI + VEN UI)**
   - Both API clients now send `X-Request-ID: crypto.randomUUID()` on every fetch call
   - Centralized via `getReq()` (GET) and `jsonReq()` (POST/PUT/DELETE) helper methods

**Key decisions:**
- Did NOT modify the VTN (openleadr-rs submodule) — it's upstream code
- Used `metrics` 0.24 facade (not `prometheus` crate directly) for idiomatic Rust metrics
- BFF metrics middleware uses `from_fn_with_state` for per-route instrumentation
- Request ID is optional (`Option<&str>`) to avoid breaking internal VtnClient usage

**Files modified:**
- `VEN/Cargo.toml` — json, trace features, metrics crates
- `VEN/src/main.rs` — JSON logging, TraceLayer, metrics recorder + `/metrics` route, instrumented polls
- `VTN/bff/Cargo.toml` — json, trace/request-id/propagate-header features, uuid, metrics crates
- `VTN/bff/src/main.rs` — JSON logging, middleware stack, metrics recorder + middleware
- `VTN/bff/src/vtn_client.rs` — `request_id` parameter + `apply_request_id()` helper
- `VTN/bff/src/routes/mod.rs` — `request_id()` helper, `metrics` module
- `VTN/bff/src/routes/programs.rs` — extract and forward X-Request-ID
- `VTN/bff/src/routes/events.rs` — extract and forward X-Request-ID
- `VTN/bff/src/routes/vens.rs` — extract and forward X-Request-ID
- `VTN/bff/src/routes/reports.rs` — extract and forward X-Request-ID
- `VTN/bff/src/routes/metrics.rs` — new: Prometheus metrics endpoint
- `VTN/ui/src/api/client.ts` — X-Request-ID on all API calls
- `VEN/ui/src/api/client.ts` — X-Request-ID on all API calls

---

### 15b. Metrics UI Pages + Use Case Manual Rewrite

**Status: COMPLETE**

**What:** Added Prometheus metrics pages to both VTN UI and VEN UI, and rewrote USE-CASE-MANUAL.md from curl-based CLI instructions to step-by-step web UI walkthroughs.

**Metrics Pages:**
- Both UIs fetch raw Prometheus text from their respective `/api/metrics` (BFF) and `/metrics` (VEN) endpoints
- Inline `parsePrometheusText()` utility parses `# TYPE`/`# HELP` comment lines and metric lines with labels into structured rows
- Displayed in MUI Tables grouped by metric name, with labels in monospace and values right-aligned
- Auto-refresh every 10 seconds via react-query `refetchInterval`
- VEN UI includes `api.baseUrl` in the query key so metrics update when switching VENs

**USE-CASE-MANUAL.md Rewrite:**
- All 8 use cases (UC1-UC8) now have "Step-by-Step Replay (Web UI)" sections describing exact UI actions: which page to navigate to, which buttons to click, which fields to fill, what values to enter
- Instructions reference actual form fields (Program Name, Enrolled VENs checkboxes, Event Name, Program dropdown, Priority, Start Time, Duration, Targets JSON, Intervals JSON)
- Original curl commands preserved in a collapsible `<details>` section ("CLI Reference") at the bottom
- Quick Reference tables updated to use UI terminology (checkboxes instead of JSON targets)

**Files created:**
- `VTN/ui/src/pages/Metrics.tsx` — VTN metrics page
- `VEN/ui/src/pages/Metrics.tsx` — VEN metrics page

**Files modified:**
- `VTN/ui/src/api/client.ts` — added `metrics()` method
- `VTN/ui/src/api/hooks.ts` — added `useMetrics()` hook
- `VTN/ui/src/App.tsx` — added `/metrics` route and nav button
- `VEN/ui/src/api/client.ts` — added `metrics()` method
- `VEN/ui/src/api/hooks.ts` — added `useMetrics()` hook
- `VEN/ui/src/App.tsx` — added `/metrics` route and nav button
- `USE-CASE-MANUAL.md` — complete rewrite (UI-first + curl reference)

**Color scheme differentiation:**
- VTN UI: teal primary (`#00695c`) — operator/server role
- VEN UI: indigo primary (`#283593`) — device/client role
- Both share amber secondary (`#ff8f00`) for visual cohesion
- Applied via MUI `createTheme` + `ThemeProvider` in `main.tsx`

**Build verification:** Both `npm run build` pass (tsc + vite) with no type errors.

---

### 16. CI Docker Build Cache — GitHub Actions Optimization

**Status: COMPLETE**

**Problem:** The E2E CI workflow was timing out at 30 minutes because it rebuilt all Rust binaries from scratch on every run (~25 min VTN + ~11 min VEN + ~2 min BFF). The last successful run took 46m37s; recent runs were cancelled at 30m30s.

**What was done:**
- Increased `timeout-minutes` from 30 to 60 for both `e2e` and `resilience` jobs (safety net)
- Added `docker/setup-buildx-action@v3` for BuildKit support
- Replaced `docker compose run --build` with `docker/bake-action@v5` using GitHub Actions cache backend (`type=gha,mode=max`)
- Bake action reads the compose file natively, builds all images with layer caching, and loads them into the local Docker daemon (`load: true`)
- Test run step uses `docker compose run --rm` without `--build` (images already built by bake)

**Why bake-action:** It natively understands docker-compose files (no separate bake HCL needed) and integrates with GitHub Actions cache backend. The `mode=max` setting caches all layers (not just final), maximizing cache hit rate for Rust incremental builds.

**Expected impact:**
- Cold cache: ~46 min (now completes within 60 min timeout)
- Warm cache, no Rust changes: ~5-10 min
- Warm cache, Rust source changes: ~15-25 min (dependency layers cached)

---

## Phase 15: VEN Simulator + Reactor (2026-02-13)

### Motivation

The VEN had a placeholder fake sensor (`main.rs:141-146`) that derived power from `timestamp % 100` — meaningless telemetry that didn't respond to OpenADR events. This phase replaces it with a physics-based simulation layer that produces causally-connected telemetry where events visibly cause device state changes.

### Architecture

Two new module trees added to the VEN application:

- **Simulator** (`VEN/src/simulator/`): Physics-based device models (EV charger, heater, PV inverter) with power model and energy counter. Each device has state that evolves over time based on setpoints from the reactor.
- **Reactor** (`VEN/src/reactor/`): Event-processing logic with FSM (Idle→Delaying→Ramping→Holding→RampingBack), event arbitration (hard constraints beat incentives, lower priority number wins), and decision trace ring buffer.
- **Profiles** (`VEN/profiles/`): Per-VEN YAML config for device mix, reaction strategy, and thresholds.

The tick loop (every 1s) replaces the fake sensor task:
1. Reactor evaluates active events → FSM → setpoints
2. Simulator applies setpoints → updates device states
3. Power model computes net import/export
4. Energy counter integrates kWh
5. Decision trace records entry to ring buffer
6. Sensor snapshot updated for backward compatibility

### New Rust Modules

| Module | Files | Purpose |
|--------|-------|---------|
| `simulator/` | `mod.rs`, `actors.rs`, `power_model.rs`, `energy.rs`, `persist.rs` | Device models, power computation, energy tracking, state persistence |
| `reactor/` | `mod.rs`, `interval.rs`, `arbitration.rs`, `fsm.rs`, `trace.rs` | Event parsing, arbitration, FSM, decision trace |
| `profile.rs` | Single file | YAML profile loading with serde defaults |

### Device Models

- **EvCharger**: SOC-based charging with configurable max power and battery capacity. Stops at 100% SOC.
- **Heater**: Thermal model with ambient heat loss, thermostat override at min/max bounds.
- **PvInverter**: Sinusoidal irradiance model (`sin(π*(hour-6)/12)` for 6am-6pm), curtailment support.

### Reactor Strategies

| Strategy | Behavior |
|----------|----------|
| `instant` | Jump to target setpoints immediately |
| `ramp` | Interpolate from current to target over `ramp_duration_s` |
| `delayed` | Wait `delay_s` before starting ramp |
| `partial` | Apply target × `compliance` factor (e.g., 70%) |
| `ignore` | Don't respond to events |

### Signal Types Handled

| Signal | Reactor Response |
|--------|-----------------|
| `EXPORT_CAPACITY_LIMIT` | Increase consumption (EV, heater), curtail PV |
| `IMPORT_CAPACITY_LIMIT` | Reduce consumption, maximize PV export |
| `PRICE` (high) | Reduce flexible loads |
| `PRICE` (low) | Increase flexible loads (valley fill) |

### New API Endpoints

- `GET /sim` — Full simulator snapshot: device states, power, energy counters
- `GET /trace?limit=N` — Decision trace (newest first, default 50 entries)

### VEN Profiles

| Profile | Devices | Strategy |
|---------|---------|----------|
| `ven-1.yaml` | EV (7.4kW) + PV (8kW) | Ramp (5min) |
| `ven-2.yaml` | Heater (5kW) + PV (12kW) | Delayed (60s + 2min ramp) |
| `ven-3.yaml` | EV (11kW) + Heater (3kW) + PV (6kW) | Partial (70%) |
| `test.yaml` | All devices | Instant |

### VEN UI Changes

- **Dashboard**: New "Simulation" card showing net power, import/export energy, device states (EV SOC, heater temp, PV output), reactor mode badge
- **Sensors**: "Values generated by simulator" annotation
- **Trace page** (new): Decision trace table with columns: Time, Mode, FSM State, Active Events, Winning Intent, Setpoints, Reason
- **Navigation**: Added "Trace" nav button

### Integration Tests

New `ven_simulator.feature` with 6 scenarios:
1. Sim endpoint returns expected fields
2. Sim endpoint shows configured devices
3. Trace endpoint returns decision entries
4. Sensor values come from simulator
5. Export capacity event → reactor EXPORT_CAP mode
6. Price event → reactor PRICE mode

### Key Design Decisions

- **Sign convention**: positive = import from grid, negative = export
- **Sim state persisted separately** in `/data/sim_state.json` (not mixed with main app state)
- **FSM state persisted**: ramp progress survives container restart
- **Graceful shutdown**: sim state saved on SIGTERM before exit
- **POST /sensors still works**: manual override for testing, tick loop overwrites every second

### Files Changed

| Area | New | Modified |
|------|-----|----------|
| Rust modules | 10 files in `simulator/` and `reactor/` + `profile.rs` | `main.rs`, `state.rs`, `config.rs`, `Cargo.toml`, `Dockerfile` |
| Profiles | 4 YAML files in `VEN/profiles/` | — |
| UI | `Trace.tsx` | `client.ts`, `types.ts`, `hooks.ts`, `Dashboard.tsx`, `Sensors.tsx`, `App.tsx` |
| Tests | `ven_simulator.feature`, `sim_steps.py` | `docker-compose.test.yml` |
| Docker | — | `VEN/docker-compose.yml`, `tests/docker-compose.test.yml` |

### Deployment & Verification

Built and deployed to Pi4-Server. VEN build with new simulator/reactor modules: ~11 min (first build with new deps). All 3 VENs came up healthy with distinct behavior matching their profiles:

| VEN | Profile | Observed Behavior |
|-----|---------|-------------------|
| ven-1 (8211) | EV+PV, ramp | Net import ~3.7kW, EV charging at 7.4kW, PV generating |
| ven-2 (8212) | Heater+PV, delayed | Net export ~1.4kW, large PV output exceeding heater load |
| ven-3 (8213) | Full mix, partial | Net import ~7.7kW, all devices active, 70% compliance |

The reactor immediately detected existing seeded events and began FSM transitions.

### Test Results

**16 features, 53 scenarios, 363 steps — all passing (3m18s)**

The 6 skipped scenarios are pre-existing `@upstream_pending` (2) and `@resilience` (4) tags.

### Issues Encountered

1. **Compilation errors (3)**: First push had `winner.value` instead of `winner.payload_value` in `arbitration.rs`, and `defaults` moved value in `reactor/mod.rs` match arms needed `.clone()`. Fixed in a follow-up commit.

2. **Test race condition**: `ven_simulator.feature:26` ("Sensor values come from simulator") failed because `ven_sensors.feature` runs earlier (alphabetical order) and its last scenario POSTs partial sensor data with `raw: {}` (no source field). If the 1-second tick loop hadn't fired yet, the GET returned stale data. Fixed by adding a 3-second wait before the sensor source assertion.

3. **Stale test DB**: First test run had 33 failures due to leftover data from a previous test run. Fixed by running `docker compose down -v` to remove the ephemeral DB volume before re-running.

### Deferred (not in scope)

- EV charging taper curve near 100% SOC
- Comfort/deadline constraints
- PV cloud dip simulation

---

## Phase 16: Auto-Report Submission from Tick Loop

**Status: COMPLETE**

### What

Closed the reporting loop: VENs now automatically submit OpenADR reports to the VTN every `report_interval_s` (default 60s) for each active event. Reports contain **actual simulator measurements** — not echoed event values — so the VTN operator sees real device response in near-real-time.

### Why

Reports were previously user-triggered only (manual form in VEN UI). The system design specifies periodic report submission, and with the simulator and reactor producing real device states, auto-reporting completes the feedback loop.

### How

**New module: `VEN/src/reporter.rs`**
- `build_report()` maps event payload types to report payload types with actual sim values:
  - `IMPORT_CAPACITY_LIMIT` → `USAGE` with actual `import_w`
  - `EXPORT_CAPACITY_LIMIT` → `USAGE` with actual `export_w`
  - `PRICE` → `USAGE` with actual `net_power_w`
  - `SIMPLE` → `SIMPLE` with `1` (acknowledged)
- Additional resource payloads: `OPERATING_STATE` (reactor mode), `STORAGE_CHARGE_LEVEL` (EV SOC if present)
- Report naming: `auto-{ven_name}-{event_id}` — one report per active event, upserted each cycle

**Tick loop integration (`main.rs`)**
- Added `report_counter` alongside existing `persist_counter`
- Clones `SimState` snapshot outside the lock to avoid blocking during HTTP calls
- Calls `vtn.upsert_report()` for each active event; logs success/failure, never blocks the tick

**Profile config**
- Added `report_interval_s` to `SimulatorConfig` (default 60, test profile uses 10)

### Test Results

**16 features, 54 scenarios, 370 steps — all passing (3m28s)**

New scenario: "Auto-report submitted for active event" — creates an `IMPORT_CAPACITY_LIMIT` event, waits 15s, verifies VEN-1 has an auto-report with `USAGE` and `OPERATING_STATE` payloads.

The 6 skipped scenarios are pre-existing `@upstream_pending` (2) and `@resilience` (4) tags.

### Key Decisions

1. **Actual sim values, not echoed event values** — more realistic and useful for the operator than ±4% noise on the event payload.
2. **Upsert semantics** — `auto-{ven}-{event_id}` naming + `upsert_report()` means repeated submissions update the same report, not a growing list of snapshots.
3. **No separate task** — reuses existing tick loop with a counter, same pattern as persist. Avoids additional tokio::spawn complexity.
4. **SimState clone outside lock** — prevents the Mutex from being held during network I/O.

---

## Phase 16: Active Event Filter + Delete Error Handling

**Status: COMPLETE**

### What

Added `?active=true|false` query parameter to the VTN events endpoint for filtering events by their temporal status. Also added user-friendly error messages when event deletion fails due to FK constraints, and documented the "Ending the Emergency" workflow for UC1.

### Changes

| File | Change |
|------|--------|
| `openleadr-rs/.../api/event.rs` | Added `active: Option<bool>` to `QueryParams` |
| `openleadr-rs/.../data_source/postgres/event.rs` | Added `is_event_active()` helper, post-filter in `retrieve_all` |
| `VTN/bff/src/routes/events.rs` | Accept `?active` query param, forward to VTN, separate cache keys |
| `VTN/ui/src/components/ConfirmDialog.tsx` | Added `error` prop with MUI Alert display |
| `VTN/ui/src/pages/Events.tsx` | Added `deleteError` state, `onError` handler on delete mutation |
| `docs/USE-CASE-MANUAL.md` | Added "Ending the Emergency" section to UC1, replaced "Cleanup" with "Event Lifecycle" |
| `docs/WHISH_LIST.md` | Added DB-level optimization and VEN polling filter as future work |

### Filter Logic (Application-Level)

The `?active` filter works as a post-filter in Rust after fetching from the database (no SQL changes, no migration, no SQLx cache change):

- `active=true`: keep events where `interval_period` is None, duration is None, or `start + duration > now`
- `active=false`: keep only past events (complement)
- absent: return all (backward compatible)

### Key Decisions

1. **Post-filter, not SQL** — avoids migration and SQLx cache changes. DB optimization deferred until event table grows large.
2. **Events are permanent records** — deletion fails when reports exist (FK constraint). The correct pattern is to edit the event to add timing, marking it as completed.
3. **Separate cache keys** — BFF caches `events`, `events?active=true`, `events?active=false` independently to avoid stale filtered results.

---

## Phase 16: Reactor Per-Interval Fix (2026-02-16)

### What was done
Fixed a bug where the reactor FSM treated all intervals of an event as one continuous activation. When a multi-interval price event had different prices per interval (e.g., $0.12 → $0.35 → $0.15), the FSM would just keep ramping its interpolation factor without resetting, causing VENs to effectively ignore price changes between intervals.

### Root cause
The FSM only tracked `event_active: bool` — it didn't know *what* the instruction was, just that *an* instruction existed. So mid-range prices (between `price_low` and `price_high`) still showed "Ramping (50%)" even though the target setpoints were identical to defaults (interpolating between defaults and defaults = no change).

### Changes
1. **`target_key()` function** — computes a string key representing the effective instruction (e.g., `PRICE_HIGH_0.3500`, `PRICE_MID`, `IMPORT_CAP_50.00`). When this key changes between ticks, the FSM resets to Idle and starts ramping fresh toward the new target.
2. **`is_effectively_active()` function** — mid-range prices (between thresholds) now return `false`, so the FSM stays Idle or ramps back instead of spuriously ramping toward defaults.
3. **Improved trace reason** — mid-range price intervals now show "Price $0.12 in mid-range (low: $0.10, high: $0.35) — no action" instead of misleading "Ramping (50%)".

### Key learning
The FSM and the setpoint computation are decoupled by design (FSM produces a factor, setpoints are computed from intent). This means the FSM must also know when the *effective* intent changes, not just whether any event exists. A boolean `event_active` is insufficient for multi-interval events with varying payloads.

---

## Phase 16: Fix VEN_NAME target reconstruction (upstream PR #372)

**Date**: 2026-02-17

### Problem
`extract_vens()` in openleadr-rs strips VEN_NAME targets on program creation and stores them as `ven_program` rows in the database. But `retrieve` / `retrieve_all` never reconstructed them — `p.targets` was always NULL for VEN enrollment. Operators who created enrollment couldn't read it back via the API, and the VTN UI couldn't display enrollment checkboxes correctly.

### What we did
1. **Created branch `fix/program-ven-targets`** from `upstream/main` (commit `b24836f`, release 0.1.3)
2. **Added `enrich_ven_targets()` helper** in `openleadr-vtn/src/data_source/postgres/program.rs`:
   - Single query against `ven_program` + `ven` for fetched program IDs
   - Groups by program_id, merges `TargetEntry { VENName, [names] }` into `content.targets`
   - Only runs for business users — VENs never see other VENs' enrollment
   - Called from `retrieve`, `retrieve_all`, `create`, and `update`
3. **Manually created SQLx offline cache** — computed SHA256 of exact query text for the `.sqlx/query-*.json` file
4. **Reduced VEN `poll_programs_secs`** default from 300 to 30 for faster program discovery
5. **Created upstream PR** [#372](https://github.com/OpenLEADR/openleadr-rs/pull/372)

### Key learnings
- **SQLx offline cache hashing**: The hash is SHA256 of the exact raw string content (between `r#"` and `"#`). Trailing whitespace matters! A single space difference invalidates the cache. When computing manually, beware of shell `$1` interpolation.
- **Docker rebuild from scratch**: Docker's `COPY . .` invalidates all subsequent layers when any file changes. A single-line change triggers ~25 min full recompile. Cargo-chef or BuildKit cache mounts would fix this.
- **BFF can't fix this bug**: The VTN API has no endpoint exposing `ven_program` associations. The enrollment data is only in the database, so the fix must be in the VTN data layer.

---

## Phase 17: Event-level VEN_NAME target filtering (Object Privacy layer 2)

**Date**: 2026-02-17
**Branch**: `fix/event-ven-targets` (from `upstream/main`)
**PR**: [#373](https://github.com/OpenLEADR/openleadr-rs/pull/373)

### Problem

OpenADR 3 specifies two-layer Object Privacy for events:
1. **Program-level** (layer 1): VEN_NAME targets on a program control which VENs see the program and its events — already implemented via `ven_program` table
2. **Event-level** (layer 2): VEN_NAME targets on an event further restrict which enrolled VENs see that specific event — **was missing**

Our UC5 seed data exposed the bug: program "EV Managed Charging" enrolls ven-2 + ven-3, event "ev-charge-pause" targets only ven-2, but ven-3 could still see the event.

### Solution

Added a SQL WHERE clause to both `retrieve()` and `retrieve_all()` in `openleadr-vtn/src/data_source/postgres/event.rs`. For VEN users, if the event has VEN_NAME targets, only show the event if the VEN's name matches. The clause uses four OR branches:
- `NOT $is_ven` — skip for business users
- `e.targets IS NULL` — no event targets → visible
- `NOT EXISTS (VEN_NAME in targets)` — has targets but no VEN_NAME type → visible
- `EXISTS (VEN's name in VEN_NAME values)` — VEN is explicitly targeted → visible

No new query parameters needed — reuses existing `is_ven()` and `ven_ids_string()`.

### Changes

1. Modified SQL in `event.rs` — `retrieve()` and `retrieve_all()`
2. Created test fixture `fixtures/events-ven-targets.sql` with event-4 (VEN_NAME target for ven-1-name only)
3. Added 4 unit tests in `mod ven_target_filtering`:
   - VEN in targets → sees event
   - VEN enrolled but not in targets → hidden
   - Event without VEN_NAME targets → all enrolled VENs see it
   - Business user → sees all events
4. Updated SQLx offline cache (2 files renamed with new hashes)
5. Built and deployed on Pi4 (~28 min full rebuild from upstream/main)

### Verification

| User | ev-charge-pause visible? | Expected |
|---|---|---|
| ven-2 | Yes | Yes (targeted) |
| ven-3 | No | No (enrolled but not targeted) |
| business | Yes | Yes (sees all) |

Events without VEN_NAME targets (e.g., from "HVAC Optimization") remain visible to all enrolled VENs — no regression.

---

## Phase 17b: Perfect Upstream Commits — PR #373 DCO Fix + Test Stack Safety (2026-02-18)

### Problem

PR #373 (`fix/event-ven-targets`) had a DCO failure on `337ca5c` ("Fix test fixtures"): the commit author was `TinkerPhu@users.noreply.github.com` but `Signed-off-by` used `tinker.phu@gmail.com` — the DCO bot requires these to match exactly.

The local branch was also in a messy state: 4 commits locally (a stray `fixup!` from an aborted rebase) vs 3 on origin.

Additionally, the first cargo test run on Pi4 caused a hard crash: two `cargo test --workspace` containers started simultaneously (first nohup launch reported exit code 1 due to stderr output, but the container had actually launched; the second explicit launch added a second), maxing out the Pi4's CPU and RAM until SSH became unreachable and required a power cycle.

### What Was Done

**Step 1 — Branch cleanup:** Reset local branch to origin state (3 commits: `284fe7e`, `337ca5c`, `8b1c380`).

**Step 2 — Squash + DCO fix:** Used `git reset --soft upstream/main` to unstage all 3 commits into the index, then created a single clean commit with:
- Author email: `TinkerPhu@users.noreply.github.com`
- `Signed-off-by: TinkerPhu <TinkerPhu@users.noreply.github.com>` (matching)
- A comprehensive commit message covering all 3 original changes

This is simpler than interactive rebase for a squash: `--soft` keeps changes staged, one `git commit -s` produces a single clean commit.

**Step 3 — Docker test stack hardening:**

The Pi crash was caused by two concurrent `cargo test --workspace` containers. Fixed in two layers:
- `CARGO_BUILD_JOBS=4` in `Dockerfile.openleadr-test` — limits parallelism per container (single container uses 4 jobs, two accidental containers use 8 total which is manageable vs the unlimited default)
- `deploy.resources.limits: cpus: '1.5', memory: 1500M` in compose — hard cap enforced by Docker
- Added `docker compose down` as mandatory first step in usage comment to prevent accidental duplicate runs

Note: We initially set `CARGO_BUILD_JOBS=1` (maximum safety) but observed via `top` that only one cargo process ran. Changed to 4 to match the previous behavior that had worked fine.

**Named volumes survive power cycle:** Confirmed Docker named volumes persist across Pi reboots. After the power cycle and restart, the build resumed from cached artifacts with zero recompilation (no `Compiling` lines in log — went straight to running tests).

**Step 4 — Force-push and CI verification:**

Force-pushed the squashed branch to origin. Upstream CI result on PR #373:
- DCO (both probot and cncf/dco2): ✅ SUCCESS
- Build and test (stable, all targets): ✅ SUCCESS
- Build and test (msrv): ✅ SUCCESS
- Clippy, Format, Audit, Unused deps: ✅ SUCCESS

**PR #372 comment:** Added a comment explaining the MSRV failure and stable build cancellations are pre-existing on `main` since Feb 9, 2026 (before our PR was opened), unrelated to our changes.

### Key Learnings

- **`git reset --soft <base>` is the simplest squash method** — no interactive rebase needed. All changes land in the index; one `git commit -s` creates a clean single commit. Avoids editor interaction entirely.
- **Bash `exit code 1` from nohup over SSH ≠ process failed** — nohup writes "nohup: ignoring input" to stderr, causing SSH's exit code to be 1. But the Docker container was actually started. Always verify with `docker ps` before concluding a background launch failed, and always run `docker compose down` first to avoid duplicate containers.
- **Docker named volumes survive power cycles** — Pi crash did not corrupt volumes. After restart, cargo resumed with 100% cache hit rate.
- **`CARGO_BUILD_JOBS` is not the same as `--jobs`** — it controls compilation parallelism within a single cargo invocation. Even without it, a second container running concurrently is the real risk.

---

## Phase 17c: Fix PR #372 Missing Fixture — `add_with_mixed_targets` (2026-02-18)

### Problem

PR #372 (`fix/program-ven-targets`) passed local review but failed upstream CI `cargo test` with:

```
failed to apply test fixture "fixtures/vens.sql":
PgDatabaseError { code: "23503",
  message: "insert or update on table \"user_ven\" violates foreign key constraint \"user_ven_user_id_fkey\"",
  detail: "Key (user_id)=(user-1) is not present in table \"user\"." }
```

Root cause: the new test `add_with_mixed_targets` was annotated `#[sqlx::test(fixtures("vens"))]` but `fixtures/vens.sql` inserts `user_ven (ven_id='ven-1', user_id='user-1')`, and `user-1` only exists in `fixtures/users.sql`. Every other test that loads `vens` always lists `users` first — this one was accidentally missing it.

### What Was Done

**Reproduce:** Checked out the PR branch on Pi4 (`git -C openleadr-rs checkout fix/program-ven-targets`), then ran the failing test via the cargo-test Docker stack with `--build` to force a fresh image from the PR source:

```
docker compose run --build --rm cargo-test cargo test -p openleadr-vtn --lib add_with_mixed_targets
```

Confirmed exact FK violation. Note: the `--build` flag was essential — without it, the stale cached image (compiled from old source) ran 0 tests because `add_with_mixed_targets` hadn't existed yet when the image was built.

**Fix:** One-line change in `openleadr-vtn/src/data_source/postgres/program.rs` line 897:

```rust
// Before
#[sqlx::test(fixtures("vens"))]
// After
#[sqlx::test(fixtures("users", "vens"))]
```

**Verify fix:** Rebuilt image again (`--build`) and ran the same targeted test → `test result: ok. 1 passed; 0 failed`.

**Full suite:** Ran `cargo test -p openleadr-vtn --lib` without `--build` (images already current) → `114 passed; 0 failed; 1 ignored`. No regressions. The 1 ignored test is a pre-existing `#[ignore]` for an upstream issue (#104).

**Commit to PR branch:** `git commit --amend --no-edit` on `fix/program-ven-targets`, preserving the DCO-signed message, then force-pushed. SHA changed `5e7507c → 881f3c2`.

**Apply to dev branch:** Pulled `dev` (was 11 commits behind), applied the same fix, committed with DCO sign-off message `"fix: add missing users fixture in add_with_mixed_targets test"`, pushed to `origin/dev` as `b48c231`.

**Update main repo submodule:** Committed `"submodule: fix missing users fixture in add_with_mixed_targets test"` pointing to `b48c231`, pushed to `origin/main` as `a7116d9`.

---

## Phase 18: Simulation Tab — Device State, Charts & Runtime Controls (2026-02-19)

**Status: COMPLETE**

Added a dedicated **Simulation** tab to the VEN UI, replacing the basic sim card on Dashboard with a full-featured page covering three sections.

### What was done

**Backend — `UserOverrides` system**
- Added `UserOverrides` struct to `state.rs` with 11 optional override fields:
  - Environment: `pv_irradiance`, `ambient_temp_c`
  - EV preference: `ev_desired_kw`, `ev_plugged`
  - Device specs: `ev_max_charge_kw`, `ev_soc_target`, `heater_max_kw`, `heater_temp_min/max_c`, `pv_rated_kw`, `base_load_w`
- Added `GET /sim/override` and `POST /sim/override` endpoints
- Threaded `overrides` into the tick loop: fetched from state before lock acquisition, passed to `reactor.evaluate()` and `sim.tick()`
- `Setpoints::defaults()` now uses `overrides.ev_desired_kw` as the idle EV charge rate (user preference, overridden by active DR events)
- `SimState.tick()` applies device spec overrides at the start of each tick (shadow profile values each cycle)
- Made `Heater.ambient_temp_c` public; `PvInverter.update()` accepts an `irradiance_override: Option<f64>` parameter
- Extended snapshots: `EvSnapshot` gains `soc_target`, `battery_kwh`; `HeaterSnapshot` gains `temp_min_c`, `temp_max_c`

**Frontend — Simulation page**
- Added recharts ^2.15.4 dependency; updated `package-lock.json`
- New `Simulation.tsx` page with three sections:
  - **A — Device State**: power/energy summary card + per-device cards (EV SOC bar, Heater temp gauge, PV irradiance bar)
  - **B — Setpoints Chart**: recharts `LineChart` driven by `useTrace(100)` showing ev_charge_kw, heater_kw, pv_curtailment_pct over the last 100 ticks
  - **C — Controls**: sliders + switches for all `UserOverrides` fields; debounced POST (500ms); "⚡ Event active" badge when reactor mode ≠ IDLE
- Added `Simulation` tab and `/simulation` route in `App.tsx` (after Dashboard)
- Added `useSimOverride()`, `useSetSimOverride()` hooks; updated `useTrace(limit)` signature

### Key Learnings
- **`UserOverrides` must use `#[serde(default)]` in `InnerState`** — without it, loading old persisted state (which lacks the field) would fail deserialization.
- **`routing::post` vs `MethodRouter::post()`** — Axum's `routing::post()` function creates a standalone MethodRouter; `MethodRouter::post()` adds a handler to an existing one. When chaining `get(h1).post(h2)`, only `routing::get` is used, not `routing::post`.
- **`npm ci` requires lock file in sync** — Adding a new dependency to `package.json` without running `npm install` first causes the Docker build to fail at `npm ci`. Always run `npm install` locally and commit the updated `package-lock.json`.

### Key Learnings

- **`docker compose run --build` is required when source changes and the image bakes source via `COPY . .`** — without it, the cached image runs the old binary and the new test simply does not exist in it. The "118 filtered out, 0 run" result is a silent false negative that can mask both failures and successes.
- **Named volumes only help the container that mounts them** — the cargo-target volume accelerates the `cargo-test` step (incremental builds ~1.5 min), but the VTN image rebuild triggered by `COPY . .` invalidation still recompiles from scratch (~25 min). These are two separate caching layers with no interaction.
- **sed is unreliable for multi-line patterns on Pi4 Alpine** — Python one-liner was more reliable: `content.replace('<old multiline string>', '<new multiline string>')`.
- **Submodule checkout conflicts** — after `git submodule update --init`, if local edits exist in the submodule, git refuses to switch branches. Fix: `git checkout -- <file>` inside the submodule first, then re-run the update.

---

## Phase 19: Event-level VEN_NAME Filter + Strip (Object Privacy layer 2, supersedes #373) (2026-02-20)

**Status: COMPLETE — deployed, PR #374 open upstream, all CI green**

Implemented `fix/event-ven-target-privacy` in the `openleadr-rs` submodule: a complete two-level object privacy solution for events with `type: VEN_NAME` targets. Supersedes the reverted PR #373 by adding both filter AND strip in one clean commit.

### What was done

**Privacy level 1 — Filter (same as PR #373 intent)**
- VENs not listed in an event's `VEN_NAME` targets get a 404 on `GET /events/{id}` and are excluded from `GET /events` list responses.
- Implemented via SQL `AND (NOT $is_ven OR e.targets IS NULL OR ...)` blocks using `jsonb_array_elements` + `ven` table join to match `ven_name`.

**Privacy level 2 — Strip (new in this PR)**
- VENs that ARE listed (and can see the event) receive responses with all `VEN_NAME` target entries removed from `targets`.
- Prevents enrolled VENs from discovering which other VENs are also targeted.
- Business users (`AnyBusiness`) see the full unstripped target list.
- Implemented via `strip_ven_name_targets(event, is_ven)` helper applied after DB fetch.

**Tests**
- New fixture: `fixtures/events-ven-targets.sql` (event-4 in program-1, targets ven-1-name)
- New test module: `data_source::postgres::event::tests::ven_target_filtering` with 4 cases:
  - `ven_in_targets_sees_event_stripped` — ven-1 can read event-4 but VEN_NAME targets are stripped
  - `ven_not_in_targets_gets_not_found` — ven-2 gets 404 on event-4
  - `ven_list_filters_and_strips` — ven-1 sees 1 stripped event, ven-2 sees 0
  - `business_sees_full_targets` — business user sees full targets

**SQLx cache**: Updated `query-638ae341...json` (retrieve) and `query-5184613a...json` (retrieve_all).

**Deployment**
- Squashed to 1 clean DCO commit: `0a6014e` on `fix/event-ven-target-privacy`
- Merged into `dev` branch (conflict-resolved with dev's `filter.active` post-processing)
- VTN image rebuilt and redeployed on Pi4
- Full integration test suite: **17 features, 62 scenarios, 439 steps — all passed**
- **Upstream PR #374** opened against `OpenLEADR/openleadr-rs:main` — all 13 CI checks passed (DCO, Format, Audit, Clippy ×4, Build+test ×5, unused-deps)

### Issues encountered

- **`Ok(` dropped during edit** — The `retrieve()` function originally has `Ok(sqlx::query_as!(...` wrapped around the chain. When adding the SQL AND block in a previous session, the `Ok(` was accidentally dropped, leaving a dangling `)`. The symptom was "unexpected closing delimiter" at the closing `}` of the impl block. Fix: restore `Ok(`.
- **Docker image not rebuilt** — Running `docker compose run --rm cargo-test` without `--build` uses the cached image. The new tests simply didn't appear in the test list (silent false-negative). Fix: explicitly run `docker compose build cargo-test` before testing.
- **Double Signed-off-by in commit** — The commit message HEREDOC already contained a `Signed-off-by` line, and `-s` added another. Fixed by `git commit --amend` with a clean single sign-off before pushing to the PR branch.
- **`cargo fmt` failure on first CI run** — Rustfmt reformats long chained closures into block form (`.map(|e| { ... })`), and wraps long `VenId::new(...)` constructor calls across lines. Fix: always run `cargo fmt` locally before force-pushing the PR branch.
- **Merge conflict with dev** — Dev branch had `filter.active` post-processing in `retrieve_all()` (from a local feature branch), not in upstream/main. Resolved by combining both: apply strip in the map, then post-filter by active status.

### Key Learnings

- **`docker compose build <service>` is the reliable way to rebuild a specific image** — `docker compose run --build SERVICE` may only rebuild dependencies, not the service itself. Always explicitly run `docker compose build cargo-test` after source changes before running tests.
- **Docker cargo-test uses named volume for compiled artifacts** — if the image isn't rebuilt with new source, Cargo sees unchanged fingerprints and skips recompilation. The tests still "run" but use the old binary — new tests don't appear at all.
- **`Ok(sqlx::query_as!(...))` pattern** — `retrieve()` wraps the entire async chain in `Ok(...)`, using `?` at the end to propagate errors from `try_into()`. The closing `)` closes `Ok(`, not a separate expression. Strip and map must be inserted before `?` but inside the `Ok(...)` chain.
- **Always run `cargo fmt` before pushing a PR branch** — rustfmt has opinions on line-length wrapping that differ from hand-written style. A format failure is a trivially avoidable CI failure.
- **Do not assume CI failures are pre-existing** — investigate every failure as potentially caused by our own changes before drawing any conclusions.

---

*Last updated: 2026-02-20 — PR #374 all CI green*

---

## Phase 19b: PR #374 Codecov coverage fix (2026-02-21)

### What was done

PR #374 had all 13 CI checks green but Codecov flagged one uncovered line — line 152 in `openleadr-vtn/src/data_source/postgres/event.rs`, which is the closing `}` of `if let Some(ref mut targets) = event.content.targets` inside `strip_ven_name_targets`. This represents the path where `is_ven == true` but `event.content.targets` is already `None`.

**Fix**
- Added `event-5` to `fixtures/events-ven-targets.sql`: same program-1, `targets: NULL` in DB
- Added 5th test `ven_sees_event_with_null_targets`: ven-1 retrieves event-5 and gets it back with `targets: None` — covers the uncovered path
- Updated `ven_list_filters_and_strips` assertions: event-5 is visible to all VENs, so ven-1 now sees 2 events (not 1) and ven-2 sees 1 (not 0); used `.any()` to find event-4 in the list instead of asserting on position

**Squash and CI**
- Intermediate test commits had wrong `Signed-off-by` email (`tinker@phu.eu` instead of `TinkerPhu@users.noreply.github.com`) causing DCO failure
- All 3 commits squashed to 1 clean commit via `git reset --soft <base>`, force-pushed — all 13 CI checks passed

**Deployment**
- Merged into `dev` (conflict-resolved by taking fix branch version)
- Submodule updated to `dev` tip, pushed to origin
- VTN image rebuilt and redeployed on Pi4

### Issues encountered

- **New `#[sqlx::test]` functions not appearing in test output** — root cause: Docker cargo-test image was stale (source baked in at image build time, not volume-mounted). Running `cargo clean` alone doesn't help if the image is old. Fix: `docker compose run --build` to rebuild image, then `cargo clean` inside the container, then test.
- **Wrong Signed-off-by email** — intermediate commits used `tinker@phu.eu`. DCO bot requires exact match with commit author email. Fix: squash all commits with correct email.
- **`basic_create_read` flaky failure in `--jobs 2` run** — client integration test races against other tests hitting the shared VTN server. Passes in isolation. Pre-existing issue, unrelated to our changes.

*Last updated: 2026-02-21 — Phase 19b complete, all CI green, deployed to Pi4*

---

## Phase 20: Simulation Tab Override UI Tests (2026-02-21)

### What was done

Fixed all 3 failing `@ven-ui` scenarios in `tests/features/sim_override_ui.feature`. The feature tests the EV charge rate slider disabled/enabled state and the owner override toggle on the Simulation tab. Full suite went from 454 steps passed / 3 failed → **468 steps passed / 0 failed**.

**Root causes found and fixed (in order of discovery):**

1. **`slotProps.input` doesn't forward `data-testid` in real Chromium** — MUI Slider's `slotProps={{ input: { "data-testid": testId } }}` works in JSDOM (unit tests) but does not reliably reach the native `<input>` element in a Chromium browser via Playwright. Fixed by wrapping each `<Slider>` in `<Box data-testid={sliderTestId}>` and scoping all selectors to `[data-testid="..."] input[type="range"]`.

2. **`wait_for_function` JS polling unreliable for slider state** — replaced with Playwright's native `wait_for_selector` using CSS `:disabled` / `:not([disabled])` pseudo-classes with `state="attached"` (works on visually hidden inputs). Timeout increased 5000→10000ms.

3. **Event DELETE returns 409 (FK constraint)** — `report.event_id` has `ON DELETE RESTRICT`. VEN-1 submits reports for active events, so events can't be deleted while reports exist. Fixed by deleting all reports via `GET /reports` + `DELETE /reports/{id}` before deleting events.

4. **Race condition: 409 still occurs after report deletion** — VEN-1 runs at ~1Hz and can submit a new report between the report-delete pass and the event-delete pass. Fixed by retrying the full delete-reports-then-delete-events loop up to 3 times with a 1s pause.

5. **`isOverriding` always `true` after reset (core bug)** — Rust serializes `Option<f64>::None` as JSON `null`. The React check `forceValue !== undefined` treats `null` as truthy, so `isOverriding` was always `true` after a `POST /sim/override {}` reset. All 3 slider scenarios failed because the slider appeared "overriding" when it shouldn't. Fixed with `forceValue != null` (loose equality, catches both `null` and `undefined`) and `forceValue ?? vtnIntentValue` for the slider value.

6. **Override state bleeds between scenarios** — VEN containers are long-lived; `UserOverrides` set in Scenario 2 (toggle click → `ev_force_kw=7.0`) survives in memory to Scenario 3. Fixed by adding `And the VEN-1 sim overrides are reset` to the behave Background (calls `POST /sim/override {}`).

**Test isolation note on disk persistence**: VEN disk persistence (`PERSIST_PATH`) is a production feature for surviving Pi4 reboots — the sim state (SoC, temperatures, energy counters) has meaningful continuity. In the test environment, `PERSIST_PATH` is not set; state is in-memory only. The bleed-over issue was purely in-memory state within a long-lived container, unrelated to disk.

### Issues encountered

- **`docker compose run --build` doesn't rebuild `depends_on` images** — `test-ven-ui` was rebuilt to a stale image for several test runs. Fix: explicitly `docker compose build --no-cache test-ven-ui` after source changes.
- **Unit tests (JSDOM) masked the Chromium selector bug** — `slotProps.input` worked in JSDOM so all 69 unit tests passed, giving false confidence. The E2E tests were the only signal that the selector didn't work in a real browser.

### Key Learnings

See KEY_LEARNINGS.md (Playwright section and React/UI section) for the MUI Slider selector pattern and the Rust `null` vs JS `undefined` pitfall.

*Last updated: 2026-02-21 — Phase 20 complete, all 468 E2E steps pass, deployed to Pi4*

---

## Phase 21: Simulation Chart — Desired Event Curves, Extended Window, and PV Refactor (2026-02-22)

### What was done

Three related improvements landed in this phase, driven by a design review of the simulation chart and the PV control model.

#### 1. Extended trace window + desired event overlay lines

The trace ring buffer was expanded from 100 → **1 000 entries**. The simulation chart now shows the last 1 000 past ticks plus 500 synthetic future ticks (~8 min projection at 1 s tick interval).

Dashed "desired" overlay lines were added to the chart, sourced from active VTN event payloads:
- **EV** — `CHARGE_STATE_SETPOINT` payload (kW), same blue `#1976d2`, dashed
- **Heater** — `IMPORT_CAPACITY_LIMIT` payload (kW), purple `#7b1fa2`, dashed
- **PV** — `EXPORT_CAPACITY_LIMIT` payload (kW), green `#388e3c`, dashed

Each dashed line only appears during the event's interval window. Arbitration mirrors the reactor: lowest `priority` wins, newest `createdDateTime` breaks ties. A `parseIsoDuration` helper parses ISO 8601 interval durations. Future points carry event-derived desired values but no actual setpoints.

#### 2. TraceSetpoints: f64 → f32 with 0.01-resolution JSON serializer

`TraceSetpoints` was introduced as a separate struct from the runtime `Setpoints` (which remains f64 for reactor math precision). Fields are stored as `f32` and serialized with a custom `serialize_round2` function that rounds via f64 to 2 decimal places on the wire. At 1 000 entries this meaningfully reduces the JSON payload for `GET /trace`.

#### 3. PV export limit refactor (pv_curtailment → pv_export_limit_kw)

A design review revealed that using `pv_curtailment: f64` (0.0–1.0 fraction) as the PV control channel was semantically wrong:

- `EXPORT_CAPACITY_LIMIT` is an **absolute kW cap** — exactly what a modern inverter's power register accepts directly.
- The reactor was ignoring the event payload value entirely and hardcoding `pv_curtailment = 0.5` as a fallback.
- Continuing to express this as a percentage in the trace (`pv_curtailment_pct`) only amplified the confusion.

**Refactor:** `pv_curtailment` was replaced with `pv_export_limit_kw: Option<f64>` throughout the entire stack:

| Layer | Before | After |
|---|---|---|
| `Setpoints` (reactor runtime) | `pv_curtailment: f64` (0.0–1.0) | `pv_export_limit_kw: Option<f64>` |
| `ExportCapLimit` reactor mode | hardcoded `0.5` | `Some(intent.value)` — direct from payload |
| `PvInverter::update()` | `curtailment_fraction: f64` | `export_limit_kw: Option<f64>` |
| Simulator physics | `output = rated * irradiance * (1 - curtailment)` | `output = min(rated * irradiance, limit)` |
| `PvSnapshot` API | `curtailment: f64` | `export_limit_kw: Option<f64>` (null = no limit) |
| `UserOverrides` | `pv_force_curtailment: Option<f64>` | `pv_force_export_limit_kw: Option<f64>` |
| `TraceSetpoints` | `pv_curtailment_pct: f32` | `pv_export_limit_kw: Option<f32>` (null = no limit) |
| Chart solid line | "PV curtailed (kW)" | "PV export limit (kW)" |
| PvControls slider | 0–100% | 0–rated_kw |

The `interpolate()` function treats `pv_export_limit_kw` as a hard constraint (applied immediately when the target has one) rather than interpolating between `None` and `Some` — consistent with how a real inverter enforces a power register.

With this change, the chart's dashed desired line (`EXPORT_CAPACITY_LIMIT` payload) and the solid actual line (reactor's enforced cap) now show the same quantity in the same unit. The gap between them is meaningful: it only exists during the FSM ramp-up delay.

### Issues encountered

- **Three stray `curtailment` / `pv_curtailment_pct` references** found by the Docker build rather than locally: `Trace.tsx`, `Dashboard.tsx`, and `Simulation.tsx` each had one missed field. Fixed immediately after each build failure.
- **`ratedKw` variable became unused** after the PV chart logic was simplified (no longer needed to convert curtailment % → kW). Removed to avoid TypeScript warnings.
- **`traceEntries.length === 0` guard** needed to replace `chartData.length === 0` — after adding 500 synthetic future points, chartData is never empty even before any trace data arrives, which caused `ResponsiveContainer` to render in tests (triggering a `ResizeObserver is not defined` error in jsdom). Guarding on `traceEntries` (past data only) restores the "No trace data yet" fallback correctly.

### Key Learnings

- **`Option<f64>` in Rust serializes as JSON `null`** — consistent with the existing pattern for other optional fields; TypeScript types use `number | null` to match.
- **Hard constraints should not be interpolated** — a kW cap either applies or doesn't. Using `if f > 0.0 { to.value } else { from.value }` for binary fields in `interpolate()` is cleaner than trying to blend `None` and `Some`.
- **Docker build is the final TypeScript type-checker for the full project** — running `npm test` locally only covers tested components; pages like `Dashboard.tsx` and `Trace.tsx` that have no dedicated tests only fail at `tsc` time during the Docker build. Running `tsc` locally before pushing would catch these earlier.

*Last updated: 2026-02-22 — Phase 21 complete, 69 UI tests pass, deployed to Pi4*

---

## Phase 22: VEN HEMS Controller — Stage 1 Entity Model

**Status: COMPLETE — 10 BDD scenarios pass on Pi4-Server (1 feature, 48 steps)**

### What Was Done

Implemented Stage 1 of the full HEMS (Home Energy Management System) controller plan. This stage is purely additive: no behavior changes, all existing endpoints work unchanged.

#### New: `VEN/src/entities/` module

All domain types from the implementation plan's Step 1:

| File | Key Types |
|---|---|
| `asset.rs` | `PowerAdjustability` (None/Recommendation/OnOff/**Steps**/Continuous), `CompletionPolicy`, `PlanTrigger`, `AssetProfile`, `AssetState`, `AssetForecast`, `AssetFlexibility`, `AssetLedger`, `AssetHeuristics`, `ThermalModelParams`, `DefaultValueCurve`, `ComfortRate` |
| `energy_packet.rs` | `EnergyPacket`, `PacketStatus`, `DeadlineTier`, `ValueCurve` (with `bid_at()` interpolation) |
| `rate_snapshot.rs` | `RateSnapshot`, `PlannedRates`, `PastRates`, `RateHeuristic` |
| `plan.rs` | `Plan`, `PlanTimeSlot`, `SlotType` (Firm/Flexible), `PacketAllocation`, `FlexibilityEnvelope`, `PlanWarning`, `CalcCache` |
| `capacity.rs` | `OadrCapacityState`, `OadrProgramConfig`, `OadrEventCache`, `OadrReportObligation` |
| `site_meter.rs` | `SiteMeter`, `PowerSnapshot`, `DispatchState`, `DeviceSession` |

#### Battery actor (`simulator/actors.rs`)

New `Battery` struct with bidirectional storage physics:
- `update(dt_s, commanded_kw)` — positive=charge, negative=discharge
- Hard stops at SoC=0 (min_soc) and SoC=1.0
- Round-trip efficiency applied on charge path only
- `BatteryConfig` in `profile.rs` with defaults (10kWh, 5kW, 0.92 efficiency, min_soc=0.10)
- ven-1 and test profiles now include a battery section

#### Simulator/state extensions

- `SimState` and `SimSnapshot` include `battery: Option<Battery/BatterySnapshot>`
- `Setpoints` gains `battery_kw: f64 = 0.0` (held by Dispatcher in Stage 4)
- `AppState` / `InnerState` extended with 5 HEMS fields (all `#[serde(skip)]`)
- Accessor methods on `AppState` for packets, plan, rates, capacity, obligations

#### Stub routes (backward compat maintained)

- `GET /packets` → `[]` (will be filled by Stage 3 Planner)
- `GET /plan` → `null` (will be filled by Stage 3 Planner)
- `GET /rates` → `[]` (will be filled by Stage 2 OpenADR Interface)

#### BDD tests

- `tests/features/ven_entity_model.feature` — 13 scenarios
- `tests/features/steps/entity_model_steps.py` — generic JSON assertion helpers reusable in later stages

### Why

Foundation for the full HEMS implementation (Stages 2–6). Every later module imports from `entities/` — having clean, compiling types first ensures no rework.

### Issues / Key Learnings

- **`reporter.rs` had a `SimState { ... }` struct literal** used in unit tests — missed adding `battery: None`. Discovered by `cargo test`, fixed quickly. Lesson: always run `cargo test` after adding required fields to structs.
- **`PowerAdjustability` needs `Steps`** — user correctly noted that `OnOff` only covers binary devices; devices with discrete power levels (3-speed pumps, step-controlled chargers) need `Steps` with a `step_values_kw: Vec<f64>` in `AssetPowerAdjustability`. Added as a distinct variant between `OnOff` and `Continuous`.
- **Stashed local change on Pi** — Pi had a stale local modification to `ven-1.yaml` from a previous session. Used `git stash` before pull.
- **Entity model diverged from spec** — First pass missed several enums, had wrong variant names, and incorrect struct fields. Lesson: always compare implementation against the spec document line by line before committing. A gap-analysis agent pass caught 20+ discrepancies.
- **Second pass completions**: `PlanningHorizon` (§6.1), expanded `PlanTimeSlot` (§6.2: GridEffectiveCost, RateEstimated, ExportCapacityLimit_kW, SurplusAvailable_kW, ImportFlexibility_kW, ExportFlexibility_kW), expanded `PacketAllocation` (§6.3: SurplusPower_kW, GridPower_kW, MarginalValue, CO2_g), `PenaltyCondition` variant fix (§6.7), added `PenaltyThreshold` + `PenaltyRule` (§6.6/6.8), `DispatchCommand` (§7.1), rewritten `DispatchState` (§7.2), two-layer `Plan` structure per §6.10 (FirmSlots/FlexibleSlots/Envelopes/summaries).
- **BDD step: `is greater than 0` vs `:f` type** — Behave's `{threshold:f}` doesn't parse bare integer `0`; feature file must use `0.0`.
- **Ambiguous step error**: parametric `@given("the VEN battery has initial SoC {soc:f}")` conflicts with any concrete step matching the same pattern. Remove concrete duplicates.
- **BDD test path inside container**: Dockerfile copies `features/` to `/tests/features/`. The entrypoint already calls `exec behave "$@"`, so the correct invocation is `docker compose run ... test-runner features/ven_entity_model.feature` (without repeating `behave`).

## Phase 23: VEN HEMS Controller — Stage 2 OpenADR Interface + Rate System

**Status: COMPLETE — 16 BDD scenarios pass (Stage 1 + Stage 2 combined, 77 steps)**

### What Was Done

Implemented Stage 2: the VEN now parses multi-interval OpenADR events into structured rate snapshots, tracks report obligations, and updates capacity state.

#### New: `VEN/src/controller/openadr_interface.rs`

- `parse_rate_snapshots(events)` — iterates event intervals, merges PRICE/EXPORT_PRICE/GHG payloads per `(interval_start, interval_end)` into `RateSnapshot` values, sorted by start time
- `parse_capacity_state(events)` — computes from scratch on each poll; IMPORT/EXPORT_CAPACITY_LIMIT/SUBSCRIPTION/RESERVATION; strictest-wins (min) across multiple events
- `extract_report_obligations(events, now, existing)` — parses `reportDescriptors`, deduplicates by `(event_id, payload_type)`
- ISO8601 duration parser covering PT5M/PT15M/PT1H/P1D/combined forms
- 10 unit tests

#### Extended `main.rs`

- Event poll loop now calls all three interface functions after fetching events
- Obligation-check tokio::spawn (5s) marks due obligations fulfilled
- Routes: `GET /rates`, `GET /obligations`, `GET /capacity`

#### BDD Tests

- `tests/features/ven_rate_system.feature` — 6 scenarios
- `tests/features/steps/rate_steps.py` — step definitions

### Issues / Key Learnings

- **`parse_capacity_state` must compute from scratch** — initial design merged with existing state, which caused stale capacity values to persist when old events from previous test runs accumulated in the VTN. Computing from scratch ensures the VEN always reflects current active events. Test revealed this: `import_limit_kw: 0.0` appeared because a previous test run's events were still in the VTN DB.
- **Behave field-specific wait steps** — scenarios that share VEN state (rate snapshots accumulate across scenarios in the same test session) need wait conditions that check for the specific field they just created (e.g., `any(s.get("co2_g_kwh") is not None`)  rather than "at least 1 snapshot exists" (which would return immediately from previous scenarios' data).
- **Unique program names per scenario** — VTN enforces unique program names; use `uuid.uuid4().hex[:8]` suffix to avoid 409 conflicts across scenarios.
- **`docker compose down -v` doesn't clear named volumes** — but the test DB uses anonymous volumes, so it should clear. Stale data appeared because a background test run left containers up. Always ensure a clean stack before running tests.
- **`step_response_status` vs `context.response` vs `context.last_response`** — entity model steps used `context.last_response` while use_case_steps used `context.response`. Fixed by making `step_response_status` fall back to `context.response` when `context.last_response` is absent. This was a pre-existing bug unrelated to Stage 2.
- **Test runner image must be rebuilt after step file changes** — step files are `COPY`'d into the image at build time. Running without `--build` after modifying step files uses stale code silently.

### Stage 2 Final Status

**25 scenarios, 135 steps — all passing** across `ven_entity_model.feature`, `ven_rate_system.feature`, `ven_simulator.feature`.

---

## Phase 21: VEN HEMS Controller — Stage 3 (EnergyPacket + Planner)

**Status: COMPLETE**

Implemented Stage 3: the VEN HEMS planner — an 8-phase greedy scheduler that produces a Plan from RateSnapshots and profile-seeded EnergyPackets.

### New: `VEN/src/controller/planner.rs`

8-phase algorithm:
- **Phase 1 PREPARE**: Build 5-min slot grid for 24h horizon; FIRM = first 4h, FLEXIBLE = rest. Populate import/export prices, CO2, PV forecast (sinusoidal), surplus, capacity limits.
- **Phase 2+3 SCORE+ALLOCATE**: Build (packet, FIRM slot) CalcCache entries with MarginalValue = ComfortBid × TimePressure. Sort by MarginalValue DESC; greedy fill respecting import cap and surplus pool.
- **Phase 4 BATTERY**: Charge in below-median-price slots, discharge in above-median/efficiency slots (arbitrage).
- **Phase 5**: Residual PV already in slot.net_export_kw.
- **Phase 6**: Penalty check deferred to Stage 4.
- **Phase 7 ENVELOPES**: For each packet with unallocated energy in FLEXIBLE horizon, build FlexibilityEnvelope with power range, window, rate estimates.
- **Phase 8 FINALIZE**: Update packet estimated_cost/co2/completion; compute slot flexibility headroom.

### Profile seeding

- Added `PlannerConfig` and `PacketSeed`/`ComfortRateSeed` structs to `profile.rs`
- `seed_packets_from_profile()` creates EnergyPackets from profile at VEN startup
- Test profile seeds 1 EV packet: 5% → 80% SoC target, 45kWh energy need, €0.50–€0.05/kWh comfort rates

### Planning loop in `main.rs`

- Planner runs 5s after startup, then every `replan_interval_s` (20s in test profile)
- After each plan, updates `active_packets` (with lifecycle transitions) and `active_plan` in AppState
- Uses `PlanTrigger::Periodic` for all cycles in Stage 3

### BDD Tests

- `tests/features/ven_planner.feature` — 6 scenarios covering packet seeding, plan structure, EV allocation, and flexibility envelopes
- `tests/features/steps/planner_steps.py` — step definitions

### Issues / Key Learnings

- **Step conflict: concrete vs parametric `@when`**: `@when("I GET /packets from the VEN")` conflicted with `@when("I GET {path} from the VEN")`. Solution: remove concrete step and rely on the parametric one from entity_model_steps.py.
- **Envelope test needs FIRM overflow**: With EV needing only 15kWh and FIRM holding 28kWh, all energy fits in FIRM → no envelopes. Fixed by lowering `initial_soc` to 0.05 (needs 45kWh), which overflows into FLEXIBLE horizon.
- **Stage 1 "stub" scenarios become wrong**: `GET /packets returns empty array` and `GET /plan returns null` scenarios from entity_model.feature were no longer correct after Stage 3 seeding/planning. Updated to test actual live behavior (non-empty array for /packets, array for /rates; /plan covered by planner feature).
- **Greedy correctness**: CalcCache entries sorted by `MarginalValue = ComfortBid × TimePressure` ensures most urgent/valuable packet-slot pairs get priority, preventing starvation of urgent but low-comfort packets.

### Stage 3 Final Status

**30 scenarios, 162 steps — all passing** across `ven_entity_model.feature`, `ven_rate_system.feature`, `ven_planner.feature`, `ven_simulator.feature`.

---

## Phase 22: Stage 6 BDD Test Suite — Full Green (27 features, 123 scenarios, 801 steps)

**Status: COMPLETE**

Fixed all failing BDD scenarios after the Stage 6 UC test suite run revealed 17 failures caused by cascading test contamination.

### Root Causes Found

#### 1. IMPORT_CAPACITY_LIMIT default value = 0.0 (critical)

`_build_intervals("IMPORT_CAPACITY_LIMIT", count=1)` in `use_case_steps.py` fell through to the generic `values: [0.0]` fallback. A 0.0 kW import cap means "no grid import" — `parse_capacity_state()` picks the global minimum across all visible events, so any single 0.0 event contaminates every test that reads `/capacity`. UC-04 created such an event in an open program; VEN-1 saw it and all subsequent EV/battery scenarios failed.

**Fix**: Default to `10000.0` for IMPORT_CAPACITY_LIMIT and EXPORT_CAPACITY_LIMIT (effectively unconstrained):
```python
_CAPACITY_TYPES = {"IMPORT_CAPACITY_LIMIT", "EXPORT_CAPACITY_LIMIT"}
default = 10000.0 if ptype in _CAPACITY_TYPES else 0.0
```

#### 2. Stale VTN events leaking across scenarios

Events created in one scenario (rate system, capacity, use-case events) persisted for all subsequent scenarios because the ephemeral DB only resets between full runs, not between behave scenarios.

**Fix**: Added `_cleanup_vtn_events(context)` in `environment.py` `after_scenario`. It deletes all events tracked in `context.rate_event_id`, `context.planner_event_id`, `context.created_event`, and `context.uc_events` via authenticated VTN DELETE calls.

#### 3. PV nighttime failure (UC-03, UC-12c)

PV model is `sin(π*(hour-6)/12)` for 6am-6pm, 0 otherwise. Tests checking for "pv" in the ledger always fail at night. `POST /sim/override` replaces the **entire** override state (not a patch), so any override that doesn't include `pv_irradiance` clears any previously set value.

**Fix**: Added `When I POST a sim override with full PV irradiance` step (sets `pv_irradiance: 1.0`) to UC-03 and UC-12c explicitly.

#### 4. Battery never in ledger (UC-11c)

Battery only appears in `/ledger` when `bat.current_kw.abs() > 1e-6`. The planner only allocates battery for arbitrage when there is a price spread across slots. With no PRICE events active, all slots have the same price → median equals all prices → no arbitrage condition satisfied → battery stays at 0 kW forever.

**Fix**: Added `battery_force_kw: Option<f64>` to `UserOverrides` in `VEN/src/state.rs` and applied it in `main.rs` like the existing `ev_force_kw` / `heater_force_kw`. Added `When I POST a sim override forcing battery to charge at {kw:f} kW` step. UC-11c now forces 2.0 kW charging to guarantee ledger accumulation.

#### 5. behave `{:f}` does not match bare integers

Step text `"at 2 kW"` doesn't match `{kw:f}` — must use `"at 2.0 kW"`.

### Key Learnings

- `POST /sim/override` is a **full replace**, not a patch. Every scenario that needs a specific override must set it explicitly, even if a prior scenario already set it.
- `--build` is **always required** when any file baked into the test-runner Docker image changes (`.feature`, `steps/`, `helpers/`, Rust source). Without it, the old image silently runs with old code.
- When `parse_capacity_state` returns a minimum, a single incorrectly-valued event can block the entire site.

### Final Test Status

**27 features passed, 0 failed — 123 scenarios, 801 steps — all green**

Commits: `2bc0a1c` → `b461b88` → `932cfe6` → `c864e75`

---

## Phase 23: Controller Dashboard Page

**Status: COMPLETE**

### What was done

Added a new **Controller** page to the VEN web UI at `/controller`, giving a "glass box" view of what the HEMS controller is actually doing.

**Files changed:**
- `VEN/ui/src/api/types.ts` — added 11 new HEMS types: `RateSnapshot`, `PlannedRates`, `OadrCapacityState`, `PacketStatus`, `PacketAllocation`, `PlanTimeSlot`, `EnergyPacket`, `FirmSummary`, `Plan`, `AssetLedger`, `UserRequest`, `FlexibilityEnvelope`
- `VEN/ui/src/api/client.ts` — added 7 API methods: `packets()`, `plan()`, `rates()`, `capacity()`, `ledger()`, `requests()`, `flexibility()`
- `VEN/ui/src/api/hooks.ts` — added 6 hooks: `usePackets`, `usePlan`, `useRates`, `useCapacity`, `useLedger`, `useRequests`
- `VEN/ui/src/pages/Controller.tsx` — new page (~420 lines) with all sections
- `VEN/ui/src/App.tsx` — nav button + route for `/controller`

**Page sections:**
1. **Status bar** — 3 Paper cards: capacity limits (import/export/subscribed), active plan summary (trigger, cost, warnings), packet counts (active/pending/done)
2. **Power chart** — `ComposedChart` with `syncId="ctrl"`: solid lines for past trace (EV/heater/PV/net), dashed lines for plan allocations per asset type, step lines for import/export capacity limits, red dashed NOW reference line
3. **Rate chart** — `ComposedChart` with `syncId="ctrl"`: step areas for import/export prices (left Y axis), CO₂ step line (right Y axis), NOW reference line
4. **Active Packets table** — shows non-terminal packets with inline fill-% progress bar (green ≥80%, orange ≥40%, red <40%), deadline, and estimated cost
5. **Energy Ledger table** — per-asset import kWh, export kWh, cost €, CO₂ g

**Data strategy:**
- Past trace: `GET /trace?limit=500` reversed to chronological order
- Future plan: `firm_slots + flexible_slots` from `GET /plan`
- Both mapped to numeric `ts` (Unix ms) for chart X axis
- `buildPowerChartData()` merges past+future into one sorted array; fields are null for the "other" side so recharts creates a clean gap at the NOW line
- Rate chart from `GET /rates` snapshots

### Key learnings

- When a recharts data point has `null` for a `Line` dataKey, it creates a gap (`connectNulls={false}`). Setting past points' `plan_*` to `null` and future points' `trace_*` to `null` gives a clean visual split at the NOW line without any special logic.
- `GET /plan` returns JSON `null` when no plan exists yet. The client method must handle `data === null` explicitly before casting.
- Pi4 may have uncommitted local files that block `git pull`. Use `git stash` before pull in deploy scripts.
- Docker service name is `ui` (not `ven-ui`) in `VEN/docker-compose.yml`.

### Commit

`dd3cee6`

---

## Phase 23: Controller UI E2E Tests + Bug Fixes

**Status: COMPLETE**

### Goal

Write Playwright/Behave E2E tests for the Controller page (TDD approach), use them to catch crashes, fix the crashes, and verify all 3 scenarios pass.

### Bugs Fixed

**1. `firm_summary` null crash (PlanCard):**
`plan.firm_summary.total_cost_eur.toFixed(3)` crashed when `plan.firm_summary` was null on the first planning cycle. Fixed with optional chaining:
```tsx
Firm cost: €{plan.firm_summary?.total_cost_eur?.toFixed(3) ?? "—"}
Import: {plan.firm_summary?.total_import_kwh?.toFixed(2) ?? "—"} kWh
```

**2. `PlannedRates` type mismatch:**
The TypeScript type declared `PlannedRates` as an object with a `snapshots` field, but the API returns a flat `RateSnapshot[]`. Fixed: `export type PlannedRates = RateSnapshot[]` and updated `buildRateChartData()` to use `rates.map(...)` directly.

**3. `AssetLedger` field name mismatch:**
TypeScript type had wrong field names (`total_consumption_kwh`, `total_production_kwh`, etc.) while the Rust `AssetLedgerEntry` struct has `energy_kwh`, `cost_eur`, `co2_g`, `updated_at`. Fixed type and `LedgerTable` rendering to use actual field names.

**4. `ledger()` object vs array:**
The `/ledger` endpoint returns `HashMap<String, AssetLedgerEntry>` serialized as a JSON object `{"heater": {...}, "ev": {...}}`, not an array. The client method was calling `.map()` on the object. Fixed by detecting the format and converting: `Object.values(data)`.

**5. f64::MAX sentinel for "no capacity limit":**
The Rust backend uses `f64::MAX` (= `Number.MAX_VALUE` ≈ 1.8e308) to mean "no capacity limit". Using `isFinite()` to detect this fails because `isFinite(Number.MAX_VALUE) === true`. Fixed with a threshold check: `slot.import_cap_kw < 1e15 ? slot.import_cap_kw : null`.

**6. PRICE event missing `intervalPeriod`:**
The test step was creating a PRICE event without an `intervalPeriod` field. VEN's `parse_rate_snapshots` requires `intervalPeriod` to determine when an interval is active; without it, rates stayed empty indefinitely. Fixed by adding `intervalPeriod: {start: now.isoformat()+"Z", duration: "PT4H"}` to the event body.

### Key Learnings

**Behave entrypoint — double-invocation bug:**
The test-runner's `entrypoint.sh` already runs `exec python -m behave "$@"`. Passing `python -m behave features/...` as the docker compose run command causes double-invocation. Correct invocation: `docker compose run --build --rm test-runner features/<feature>.feature`.

**test-ven-ui rebuild required:**
`docker compose run --build test-runner` only rebuilds `test-runner`, NOT `test-ven-ui`. After changing VEN UI React source, must explicitly run `docker compose build test-ven-ui` before the test run.

**Recommended sequence:**
```bash
docker compose down
docker compose build test-ven-ui
docker compose run --build --rm test-runner features/controller_ui.feature
```

**React 18 unhandled errors = empty root div:**
When a React component throws during render without an Error Boundary, React 18 unmounts the entire tree. Tests see only a timeout with no clue about the cause. Diagnose with Playwright's `page.on("pageerror", ...)` and `page.on("console", ...)` listeners — added to `environment.py` for all `@ven-ui` scenarios.

**API contract verification:**
TypeScript types can silently diverge from actual API responses. When a page crashes, verify with `docker exec <container> curl -s <endpoint>` before editing types. Never trust declared types without confirming against live data.

### Files Changed

- `VEN/ui/src/pages/Controller.tsx` — null guards, data-testid attributes, type fixes for rates/ledger/cap
- `VEN/ui/src/api/types.ts` — `PlannedRates` flat array, `AssetLedger` correct field names
- `VEN/ui/src/api/client.ts` — `ledger()` object→array conversion
- `tests/features/controller_ui.feature` — 3 new @ven-ui scenarios
- `tests/features/steps/controller_ui_steps.py` — step implementations
- `tests/features/helpers/ui.py` — `go_controller()` method with debug dump
- `tests/features/environment.py` — pageerror + console listeners for @ven-ui

### Result

All 3 Controller UI scenarios pass:
```
1 feature passed, 0 failed, 0 skipped
3 scenarios passed, 0 failed, 0 skipped
15 steps passed, 0 failed, 0 skipped, 0 undefined
```

### Commits

`dea8d71`, `79911e3`, `8a001b1`, `ed8209e`, `d5de560`, `e565ad7`, `fd4b200`, `93e0a39`

---

## Phase 24: Fix Test Suite — Expired Timestamps + DB State Pollution

**Status: COMPLETE**

### Goal

After Phase 23, a full test run revealed 14 failures across 5 feature files. Investigate root causes and restore the full suite to 0 failures.

### Root Cause 1: Expired Event Timestamps

`VEN/src/vtn.rs` polls with `GET /events?active=true`. The openleadr-rs `is_event_active()` check filters out events whose `intervalPeriod.end` is in the past.

Three files had hardcoded timestamps that expired:
- `rate_steps.py` — 5 event-creation steps with `"2025-01-01T...Z"` dates
- `use_case_steps.py:step_create_uc_event_with_ip` — hardcoded `"2026-03-01T14:00:00Z"` (+ PT4H = expired 12 days ago)
- `ui_steps.py:step_ui_create_event_with_ip` — same hardcoded date

**Fix**: Replace all hardcoded dates with `datetime.now(timezone.utc) + timedelta(...)` so timestamps are always in the future.

### Root Cause 2: Program Accumulation — 409 Conflicts and Pagination

Programs created by test scenarios persisted across runs (no cleanup). After multiple runs, 100+ programs accumulated in the test VTN. This caused two cascading failures:
- **409 conflicts**: `_create_or_reuse_program` handled 409 by looking up the existing program in `GET /programs`, but with 100+ programs and the VTN's default page size (~50), the lookup missed entries → `AssertionError: 409 but program not found`
- **UI dialog stuck open**: The VTN UI's Create Program form got a 409 from BFF, React kept the dialog open instead of closing it
- **BFF 502**: Bulk-deleting 100+ programs in `before_all` briefly overloaded VTN, causing BFF to fail on the immediately following features

**Fix**: Added `before_feature` hook in `environment.py` that calls `_cleanup_all_programs()` — paginated DELETE of all programs before each feature. Per-feature cleanups are small (few programs from the prior feature), no overload, and each feature starts clean.

### Root Cause 3: Sensor Race Condition

`ven_sensors.feature:17` ("POST partial sensor data (power only)") failed intermittently in the full suite. The VEN sim tick overwrites sensor state every 1s. If the sim tick fires between POST and GET, `GET /sensors` returns the simulated power instead of the posted 300.0 W.

**Fix**: In `step_sensor_power`, fall back to `context.post_response` (the POST's immediate return value) when the GET result doesn't match. This uses the authoritative write value when a race is detected.

### Key Learnings

**VTN pagination breaks `_create_or_reuse_program`**: The helper does `GET /programs` without a limit — VTN returns only a page. With 100+ accumulated programs, the target appears on a later page → helper asserts it doesn't exist. Fix: keep DB clean, not the helper.

**`before_feature` > `before_all` for DB cleanup**: Per-feature cleanup means no mid-run accumulation (130+ programs by the time `ui_use_cases.feature` runs). A single large bulk delete briefly overloads VTN causing BFF 502 in the immediately-following features.

**VEN sim writes sensor state every 1s**: `POST /sensors` sets state but the sim immediately overwrites it. Tests that compare `GET /sensors` after a POST are inherently racy. Use the POST response itself as ground truth.

### Files Changed

- `tests/features/steps/rate_steps.py` — dynamic timestamps for 5 event-creation steps
- `tests/features/steps/use_case_steps.py` — dynamic timestamp for `step_create_uc_event_with_ip`
- `tests/features/steps/ui_steps.py` — dynamic timestamp for `step_ui_create_event_with_ip`
- `tests/features/environment.py` — `before_feature` cleanup hook; `_cleanup_all_programs()` function
- `tests/features/steps/ven_sensors_steps.py` — fallback to POST response in `step_sensor_power`

### Result

```
29 features passed, 0 failed, 0 skipped
129 scenarios passed, 0 failed, 0 skipped
837 steps passed, 0 failed, 0 skipped, 0 undefined
```

Two consecutive runs (first `--build`, second without) both 0 failures.

### Commits

`13a541c`, `852cc50`, `01f7592`, `9bf31e4`, `0b7be38`


---

## Phase 26: Controller V2 Dashboard — Full Matrix Layout

**Date**: 2026-03-14
**Branch**: `001-controller-dashboard-v2`
**Scope**: New `/controller-v2` React page with per-asset cells (left metrics / mid timeline / right controls), two grid-level cells (tariff + accumulated power), cell pinning/collapse, and Rust backend override stubs.

### What Was Built

**BDD-first (Constitution Principle II)**: All 4 feature files written and confirmed failing before any implementation code was written.

**Backend stubs** (`VEN/src/state.rs`, `VEN/src/simulator/mod.rs`, `VEN/src/main.rs`):
- `ev_initial_soc: Option<f64>` — one-shot SoC jump; cleared in `main.rs` after tick
- `battery_initial_soc: Option<f64>` — one-shot SoC jump
- `battery_capacity_kwh: Option<f64>` — persistent capacity override

**Frontend components** (all under `VEN/ui/src/components/controller-v2/`):
- `types.ts` — `AssetId`, `AssetSummary`, `AssetTimePoint`, `TariffSnapshot`, `TariffTimePoint`, `StackedAreaPoint`, `CollapseState`
- `dataBuilders.ts` — `deriveAssetSummaries`, `buildAssetTimeline`, `buildStackedAreaData`, `buildTariffTimeline`, `deriveTariffSnapshot`, `findCurrentTariff`
- `AssetLeftSection.tsx` — power/cost/CO₂/SoC metrics, all `data-testid` per contracts
- `AssetMidSection.tsx` + `AssetTimelineChart.tsx` — recharts `ComposedChart` with power/cost/CO₂ lines, NOW `ReferenceLine`
- `AssetRightSection.tsx` — two MUI Accordions (Status Settings defaultExpanded, Simulation Characteristics collapsed); per-asset controls for EV/Battery/Heater/PV/BaseLoad
- `AssetCell.tsx` — three-section horizontal layout, MUI `Collapse` for left/right, pin/collapse buttons
- `PinnedZone.tsx` — sticky container for pinned cells
- `GridTariffCell.tsx` — 5 tariff metrics + `TariffChart`
- `GridAccumulatedCell.tsx` — per-asset power list + `StackedAreaChart`
- `charts/TariffChart.tsx` — 5 series, dual Y-axes
- `charts/StackedAreaChart.tsx` — bidirectional stacking with `stackId="positive"` / `stackId="negative"`
- `VEN/ui/src/pages/ControllerV2.tsx` — full page with all hooks, pinned/collapse state, all cell renderers

**BDD tests** (14 scenarios, 58 steps — all passing):
- `01_layout.feature` — grid cells visible above assets
- `02_asset_cells.feature` — power/cost/CO₂/SoC values, NOW line
- `03_simulation_controls.feature` — EV plugged toggle, SoC slider, POST /sim/override
- `04_navigation.feature` — pin, unpin, collapse left/right

**Unit tests**: `ControllerV2.test.tsx` — 21 tests, all passing.

**Full suite**: 33 features, 143 scenarios, 895 steps — zero failures.

### Key Decisions

1. **One-shot stub clearing in main.rs, not tick()**: The `tick()` method receives `&UserOverrides` (immutable reference), so clearing can't happen there. Clearing is done in `main.rs` after the tick block by cloning+patching and posting back to shared state.

2. **`data-testid` INSIDE MUI Collapse**: For collapse tests that use Playwright `is_visible()`, the `data-testid` element must be inside the `Collapse` component so `is_visible()` returns `false` when the content is hidden.

3. **Bidirectional recharts stacking**: Positive values use `stackId="positive"`, negative values use `stackId="negative"` with a mirrored negative series.

4. **ResizeObserver mock in test setup**: recharts `ResponsiveContainer` requires `ResizeObserver` which jsdom doesn't provide. Mocked in `setup.ts` using `globalThis` (not `global`) to avoid TypeScript compile failure in browser target.

5. **MUI Switch click target**: Playwright's `el.click()` on the MUI Switch root `<span>` does not reliably trigger `onChange`. Must target `input[type="checkbox"]` inside it.

6. **Null vs absent in sim overrides**: When `POST {}` clears overrides, the GET response returns `{"ev_plugged": null}`. Python's `dict.get("ev_plugged", True)` returns `None` (key present), not `True`. Must handle `None` explicitly: `True if v is None else v`.

### Issues Encountered

- TypeScript compile errors (`Cannot find name 'onOverrideChange'`, `unused 'overrides'`, `unused 'nowMs'`) — caught at Docker build time, fixed before deploy.
- `global` not available in browser TypeScript target — replaced with `globalThis`.
- Wrong docker-compose directory for Pi4-Server builds (`/srv/docker/openadr_lab/VEN/` not root).
- BDD toggle test failing due to null handling and MUI Switch click target — both fixed in step definitions.

### Commits

`63244ef`, `219bcdc`, `12055ae`, `115fed3`, `08ea264`, `f104589`, `ebc4688`, `cbf24d6`

---

### 25. VEN Simulator Reform — Generic Asset Model (speckit 002)

**Status: COMPLETE** | Branch: `002-ven-simulator-reform`

**What was done:**

Replaced the hardcoded per-device named fields in `SimState` (ev, heater, pv, battery, base_load_w, energy) with a generic `Vec<AssetEntry>` model. Each entry holds an `AssetState` enum variant, a setpoint, the last actual power, and a per-asset `EnergyCounter`. This removes the need for device-specific branches throughout the tick loop, planner, and dispatcher.

**Architecture changes:**
- `VEN/src/simulator/actors.rs` deleted — replaced by `simulator/assets/` directory with one file per asset type (`ev.rs`, `heater.rs`, `pv.rs`, `battery.rs`, `base_load.rs`, `mod.rs`)
- `AssetState` enum dispatches all 8 methods via match (exhaustiveness guarantees new types are handled)
- `TickEnvironment = HashMap<String, f64>` passed to `update()` — assets read what they need (hour_of_day, ambient_temp_c, pv_irradiance)
- `Profile.devices: DeviceConfig` supplemented with `Profile.assets: Vec<AssetConfig>` using `#[serde(tag = "type", rename_all = "snake_case")]` internally-tagged enum for YAML deserialization
- All 4 YAML profiles migrated to `assets:` list format; legacy `devices:` still supported as fallback
- `SimState.tick()` now accepts `HashMap<String, f64>` setpoints (keyed by asset id) instead of the named `Setpoints` struct
- `SimState.to_sim_snapshot()` outputs `assets: HashMap<String, AssetSnapshot>` (generic) PLUS backward-compat named fields (`ev`, `heater`, `pv`, `battery`, `base_load_w`) derived from the typed `AssetState` so existing UI consumers require no changes
- `power_model.rs` simplified to `random_voltage()` only; grid totals derived inline in tick
- `UserOverrides` stub fields removed: `ev_initial_soc`, `battery_initial_soc`, `battery_capacity_kwh`
- New API endpoints: `GET /sim/schema`, `POST /sim/reset/:asset_id`, `PUT /sim/config/battery`
- `controller/trace.rs` added with `AssetHistoryBuffer` ring-buffer data structure (no callers yet)

**Key issues and learnings:**

1. **`_resolve_nested` backward compat**: Feature files use paths like `"battery.soc"` against the `/sim` response. The new format is `assets.battery.soc`. Updated `_resolve_nested` in `entity_model_steps.py` to fall back to `data["assets"][first_part]` when the top-level key is not found.

2. **`user_request.rs` reads `SimSnapshot` not `SimState`**: The `resolve_target()` function receives `Option<&SimSnapshot>`. `SimSnapshot.assets` is `HashMap<String, AssetSnapshot>` and `AssetSnapshot.values` is a flat `HashMap<String, f64>`. SoC is stored as `soc_pct` (0-100). Changed access from `.ev().map(|e| e.soc)` to `.assets.get("ev").and_then(|a| a.values.get("soc_pct")).map(|pct| pct / 100.0)`.

3. **UI tests broke without compat fields**: `Simulation.tsx` and `Controller.tsx` components check `sim.ev != null` before rendering device cards. With the new format lacking top-level named fields, all device cards returned null → all Playwright UI tests timed out. Solution: add backward-compat typed snapshots (`LegacyEvSnapshot`, etc.) reconstructed directly from the typed `AssetState` in `to_sim_snapshot()`. Zero UI changes needed.

4. **`serde(flatten)` on `AssetSnapshot.values`**: Applied to merge the asset's generic state values flat into the JSON object alongside `power_kw`. This allows Python `_resolve_nested` to reach `assets.battery.soc` without an extra `values` nesting level.

5. **`base_load` asset id**: The base load asset is stored under id `"base_load"` (with underscore) in the `Vec<AssetEntry>`. The old response had `base_load_w: f64` at the top level. This is now restored as a compat field derived from `assets.get("base_load").last_power_kw * 1000`.

**Final test result:**

33 features, 143 scenarios, 895 steps — all passing with 0 failures on Pi4-Server ARM64.

---

## speckit 003: Asset Request Dispatch Refactor

**Date**: 2026-03-15
**Branch**: `003-asset-request-dispatch`
**Scope**: Pure internal refactor — no API, behavior, or UI changes.

### What was done

Removed the hardcoded `match body.asset_id.as_str()` switch from `controller/user_request.rs` by adding a `resolve_request_target` method to the `AssetState` enum dispatch chain. Each energy-storage asset type (`EvCharger`, `Battery`) now declares its own request resolution logic. Non-storage types (`Heater`, `PvInverter`, `BaseLoad`) return `None`, which the controller maps to `RequestError::UnknownAsset`.

`user_request.rs` now receives `&[AssetEntry]` instead of `(&Profile, Option<&SimSnapshot>)`, eliminating the `Profile` and `SimSnapshot` imports entirely. The caller in `main.rs` briefly locks `ctx.sim: Arc<Mutex<SimState>>`, clones the assets vec, and passes it in.

Added a new BDD scenario: "Request for a non-storage asset is rejected" — `POST /user-requests` for `asset_id: "pv"` must return 422 with an `"error"` field.

### Issues encountered

**1. Pre-existing TypeScript build errors (speckit 002 leftovers):**
- `Simulation.test.tsx` and `ControllerV2.test.tsx` mocks were missing the `assets` field added to `SimSnapshot` in speckit 002. Fixed by adding `assets: {}` to both mocks.
- `AssetRightSection.tsx` referenced `ev_initial_soc`, `battery_initial_soc`, and `battery_capacity_kwh` on `UserOverrides`, which don't exist in the type. These fields have no backend support (SoC state changes require `POST /sim/reset/:id`, capacity config requires `PUT /sim/config/battery`). Fixed by making those sliders read-only (`disabled`) and removing the invalid `onChange` calls.

**2. New BDD scenario failing — falsy 4xx response in Python `or` chain:**
`entity_model_steps.py` checked for `last_response` with `getattr(...) or getattr(...)`. `requests.Response` with a 4xx status code evaluates to `False` in a boolean context, so the `or` chain fell through to `None` and the assertion failed. Fixed by using `is None` check instead of `or`.

### Key learnings

- `requests.Response` is falsy for 4xx/5xx responses (`response.ok == False`). Never use Python `or` to chain response fallbacks — use explicit `is None` checks.
- Pre-existing TypeScript compilation errors in test builds can block CI even when the Rust refactor itself is correct. Always run the full build (including UI) before declaring success.
- After speckit 002's generic asset model, `user_request.rs` no longer needed `Profile` — each `AssetEntry` carries its own config in `AssetState`. The dependency was purely incidental and the refactor removed it cleanly.

### Final test result

33 features, 144 scenarios, 899 steps — all passing with 0 failures on Pi4-Server ARM64.

**Commits:** `b4eea32`, `6a5163b`, `09a64fe`

---

## Phase 24b: VEN Controller Reform (speckit 004)

**Status: COMPLETE**
**Date: 2026-03-15 → 2026-03-16**
**Commits:** `c84f273`, `90edebb`, `b77c152` (+ Phase 1-3 from prior session)

### Objective

Full reform of the VEN controller architecture across 5 user stories:

1. **US1 — Single Authoritative Control Path**: Delete the reactor, rewrite the dispatcher and tick loop so the planner is the sole authority
2. **US2 — Controller Observability**: Wire asset history buffers + emit `ControllerEvent` entries, expose `GET /trace/events` + `GET /trace/history`
3. **US3 — Correct Packet Energy Accounting**: Consolidate into `monitor::record_tick`, emit `PacketTransition`/`RequestTransition` events
4. **US4 — Dual-Mode Reporting**: New `controller/reporter.rs` with timer-driven measurement reports + event-driven status reports
5. **US5 — Tariff Nomenclature**: Rename `RateSnapshot` → `TariffSnapshot`, `GET /rates` → `GET /tariffs`

### What was done

**Phase 1 (BDD First Gate):** Rewrote all BDD scenarios referencing old reactor/trace/rates endpoints before touching Rust. `/trace` → `/trace/events`, `/rates` → `/tariffs`, removed force-override tests and FSM state tests. New scenarios added for `GET /trace/events`, `GET /trace/history`, `GET /tariffs`. Suite ran red on new endpoints as required.

**Phase 2 (Foundational):** Renamed `RateSnapshot` → `TariffSnapshot`, `PlannedRates` → `PlannedTariffs` across all files. Added `ControllerEvent` enum (7 variants) with `serde(tag = "type")`. Added `AssetHistoryBuffer` ring buffer and `ControllerTrace` holder. Updated `state.rs` to hold `controller_trace` and expose `push_controller_event` + `push_asset_row`.

**Phase 3 (Reactor Deletion):** Deleted all 5 files in `VEN/src/reactor/`. Rewrote `dispatcher::build_setpoints` as the single control function (plan → setpoints, no FSM/reactor). Rewrote the tick loop: `build_setpoints → sim.tick → update_sim`. All UC-01–UC-12 use case scenarios confirmed passing. Regression fixes required: null-guard `entry.setpoints` in `Controller.tsx` and `Trace.tsx`, explicit domain computation in `AssetTimelineChart.tsx` to force NOW reference line visible, restored `ResponsiveContainer` after discovering its async `ResizeObserver` is needed for MUI Collapse animation timing.

**Phase 4 (Observability):** Wired asset history writes per tick loop (T032): every tick, each asset's `power_kw`, state values, `cost_rate_eur_h`, and `co2_rate_g_h` are pushed to `AssetHistoryBuffer`. Added OpenADR event detection in the poll-events task: `OpenAdrArrived`/`Expired` on event set changes, `RateChange` on tariff count change, `CapacityChange` on import limit change. `GET /trace/events` returns newest-first ControllerEvents; `GET /trace/history?asset=ev&limit=5` returns timeline rows with `power_kw`, `soc_pct`, `cost_rate_eur_h`, `co2_rate_g_h`, etc.

**Phase 5 (Packet Accounting):** Rewrote `monitor.rs`: replaced `update_ledger` with `record_tick` which combines ledger accumulation, packet status transitions (Scheduled→Active, Active→Completed/PartialCompleted), and `PacketTransition` event emission. `RequestTransition` events added to HTTP handlers. All ledger/dispatcher BDD scenarios verified passing.

**Phase 6 (Reporter Reform):** Created `controller/reporter.rs` with `build_measurement_report` (per-event, uses asset history) and `build_measurement_reports_for_active_events` (timer entry point), plus `build_status_report` (for PlanCycle/PacketTransition). Deleted orphaned `src/reporter.rs` (was using deleted `reactor::interval`). Timer block now calls `build_measurement_reports_for_active_events` every `report_interval_s`. Planning loop emits status report on each PlanCycle. Tick loop emits status reports on PacketTransitions. Fixed the known regression: `ven_simulator.feature:26 Auto-report submitted for active event`.

**Phase 7 (Tariff Verification):** Confirmed `GET /tariffs` returns tariff data, `GET /rates` returns 404. No struct-level uses of old `RateSnapshot`/`PlannedRates`/`PastRates` remain.

**Final result:** 32 features passed, 0 failed, 1 skipped — 137 scenarios passed, 0 failed.

### Issues encountered

**1. recharts ReferenceLine silently hidden when x falls outside domain:**
When `buildAssetTimeline` returns only future plan slots (all timestamps ≥ nowMs), recharts auto-computes the domain as `[T1, Tn]` where `T1 > nowMs`. The `x={nowMs}` reference line is outside this domain and silently dropped. Fix: explicit domain computation: `tMin = Math.min(nowMs - 300_000, ...chartData.ts)`, `tMax = Math.max(nowMs + 300_000, ...chartData.ts)`. Also added a 2-point fallback chart data when `data.length === 0` to ensure nowMs is always in range.

**2. ResponsiveContainer async timing is a test dependency:**
While debugging the recharts domain issue, `ResponsiveContainer` was temporarily replaced with `ComposedChart width={600}`. This caused the collapse-section navigation tests to fail because `ResponsiveContainer` uses `ResizeObserver` (async), which provides a natural timing delay that MUI `Collapse` animations rely on during tests. Restoring `ResponsiveContainer` fixed both issues.

**3. Docker container reuse masks new image:**
When re-running tests after rebuilding `test-ven-ui`, `docker compose run --rm test-runner` reused the still-running `test-ven-ui` container from the previous run (cached old image). Always run `docker compose down` before `docker compose run` after rebuilding dependent services.

**4. Phase 6 reporter used deleted reactor dependency:**
The old `VEN/src/reporter.rs` imported `crate::reactor::interval::find_active_intervals`. Since `src/reporter.rs` was never added to `mod` declarations in `main.rs`, it compiled silently despite the broken imports. The Phase 3 reactor deletion orphaned it without a visible build error. Fixed in Phase 6 by creating the new `controller/reporter.rs` with inline interval-activity detection (no reactor dependency) and deleting the old file.

**5. "Already up to date" masks forgotten git push:**
Several times, `git pull` on Pi4 showed "Already up to date" while Docker was still building from the previous commit because the local commit hadn't been pushed yet. Pattern: always `git push` locally before SSH → Pi4 → `git pull`.

### Key learnings

- recharts silently drops reference lines whose `x` value falls outside the XAxis domain. Always compute a domain that explicitly includes the reference line value.
- `ResponsiveContainer`'s async `ResizeObserver` creates a timing buffer that can be load-bearing for animation-dependent tests. Never replace it with a fixed-width chart without checking test timing assumptions.
- When deleting a Rust module, always search for all `use crate::<module>::` references in files that might not be compiled (orphaned modules, disabled `mod` declarations). Build success only confirms compiled code.
- An event-driven reporter that uses `ControllerEvent` variants as dispatch key is cleaner than a reactor-mode string parameter. The `serde(tag = "type")` enum makes trace events directly serializable to JSON without extra mapping.

## Phase 25: VEN Timeline UI (speckit 005)

**Status: COMPLETE**
**Date: 2026-03-16 → 2026-03-17**
**Branch:** `005-ven-timeline-ui`
**Commits:** `ad24e90`, `9d9ab4f`, `2812a37`, `bb37aa2`, `3ca3399`, `f054409`, `abe0f3e`, `ed4c35e`

### Objective

Add per-asset timeline charts, grid-level stacked area chart, and schema-driven simulation controls to the Controller V2 UI. Full BDD coverage for 19 new `@ven-ui` scenarios.

### What was done

**Phases 1–5 (prior session):**
- Backend: added `AssetHistoryBuffer` ring buffer (3600 rows/asset, 1 sample/sec), `GET /timeline/:asset_id`, `GET /timeline/all` endpoints with query params `hours_back`, `hours_forward`, `max_points`.
- Frontend: `useTimeline` / `useAllTimelines` hooks, `AssetMidSection` recharts area chart with NOW reference line, `GridAccumulatedCell` stacked area from `useAllTimelines`, schema-driven `DynamicControl` in `AssetRightSection`, per-cell extended window toggle.
- BDD: 19 new scenarios across 4 feature files (`01_timeline.feature`, `02_asset_cells.feature`, `03_simulation_controls.feature`, `04_navigation.feature`).

**Phase 6 (schema-driven controls):** Added `GET /sim/schema` to Rust backend returning `HashMap<assetId, Vec<ControlDescriptor>>`. Each descriptor has `key`, `label`, `kind` (`switch`/`slider`/`number_input`), `min`, `max`, `unit`. `AssetRightSection` fetches schema via `useSimSchema()` and renders controls via `DynamicControl`.

**Phase 7 (GridAccumulatedCell):** Stacked area chart driven by `useAllTimelines`. Each asset gets its own `Area` with positive/negative value handling.

**Phase 8 (API rename & cleanup):**
- `RateSnapshot` → `TariffSnapshot` in TypeScript (alias kept for backward compat)
- `useRates` → `useTariffs` (alias kept)
- Deleted `buildAssetTimeline`, `buildTariffTimeline`, `buildStackedAreaData`, `getTraceAssetPower` from `dataBuilders.ts` (replaced by hook-driven data flow)
- `nowMs` in `ControllerV2.tsx` changed to `useMemo(() => Date.now(), [])` to avoid rendering on every data refetch

**Phase 9 (browser freeze fix):** After deploying, the Pi4 browser froze because the timeline buffer had accumulated 3600 rows/asset × 5 assets + 1 allTimelines call = ~21,000+ data points. Added server-side `max_points` downsampling: `TimelineParams.max_points` (default 120) with a `downsample()` stride function in Rust that always preserves the last point. A fresh VEN returns ~62 points; a 1-hour-old VEN returns exactly 120. Freezes eliminated.

**Phase 10 (ControlKind case fix):** Rust `#[serde(rename_all = "snake_case")]` produces `"switch"`, `"slider"`, `"number_input"`. TypeScript `ControlKind` had PascalCase `"Switch"`, `"Slider"`, `"NumberInput"`. `DynamicControl` comparisons never matched so all controls fell through to the NumberInput/TextField fallback — MUI Switch never rendered. Fixed by aligning `ControlKind` to snake_case.

**Phase 11 (ev_plugged fallback):** Even with the correct Switch rendering, toggling sent `ev_plugged: true` (not false). Root cause: when no override is set (`overrides = {}`), `getValue("ev_plugged")` returned `null`; `Boolean(null) = false` rendered Switch as unchecked. The sim's actual default is `plugged = true`. Clicking unchecked → checked = `true` → POST sends `true`, not the expected toggle to `false`. Fixed by adding a sim-snapshot fallback in `getValue` for `ev_plugged`: when override is unset, fall back to `sim.ev.plugged`.

**Final result:** 33 features passed, 0 failed, 1 skipped — 149 scenarios passed, 0 failed — 884 steps.

### Issues encountered

**1. Missing committed files caused build failure on Pi4:**
`api/hooks.ts` and `api/types.ts` were modified locally but never staged. The Pi4 build failed with "Module has no exported member 'TariffSnapshot'". Fixed by committing them as a separate fix commit.

**2. AmbiguousStep — duplicate step definition:**
`the response JSON is an array` was defined in both `ven_timeline_steps.py` and `entity_model_steps.py`. behave raises `AmbiguousStep` and exits. Fixed by removing from the new file.

**3. Browser freeze from accumulated timeline data:**
test-ven-ui was stale (21 hours old). After rebuild, all `@ven-ui` scenarios failed because recharts was processing ~18,000+ data points on a Pi4 ARM CPU, freezing the JS thread. Playwright's `wait_for_selector` timed out with "locator resolved to visible" in the call log — the element existed in DOM but JS was frozen. The `inner_html()` call also timed out. Diagnosed by examining Playwright's own call log entries. Fixed by server-side downsampling.

**4. ControlKind case mismatch — silent rendering fallback:**
Backend `serde(rename_all = "snake_case")` vs TypeScript PascalCase. Scenario 9 (visibility) still passed because the fallback TextField also had `data-testid`, but scenario 17 (interaction) failed when looking for `input[type="checkbox"]` inside it.

**5. Switch checked state reflects sim state, not override state:**
When override is empty (`{}`), the control should show the sim's current hardware state, not assume a default of `false`. Any switch-type control that can be absent from overrides needs a sim-state fallback. Only `ev_plugged` was affected in this project; addressed with a targeted fallback.

### Key learnings

- Server-side `max_points` downsampling is essential for timeline APIs consumed by browser charts on constrained hardware. 3600 rows/asset at 5+ assets = browser freeze on Pi4.
- When Playwright `wait_for_selector` times out but the call log shows "locator resolved to visible", the page DOM is present but the JS thread is blocked. This points to CPU overload from data processing, not a missing element.
- Rust `#[serde(rename_all = "snake_case")]` produces lowercase underscore names. Any TypeScript `ControlKind` or enum must match exactly — case mismatches produce no TypeScript error (it's a string union) but silently fall through to a wrong rendering branch.
- Schema-driven controls (Switch/Slider) need to display the system's current real state as initial value, not assume `false`/0. When the backend override is absent, use the sim snapshot value as fallback so the user sees accurate state before interacting.

---

### Phase 27: Asset Interface — forecast() & past() (speckit 007)

**Status: COMPLETE** — 36 features, 173 scenarios, 1024 steps, 0 failures

**What was done:**

1. **New `common/` module** (`VEN/src/common/mod.rs`): Introduced `TimeSeries` type with `samples: Vec<(DateTime<Utc>, f64)>`, `Quantity`/`Unit`/`Interpolation` enums, and `is_ascending()` invariant check. This is the shared return type for all asset forecasting.

2. **`forecast(timespan)` on all 5 assets**: Each asset type implements its own forecasting model:
   - **PV**: sinusoidal irradiance model (`sin(π*(hour-6)/12)`) sampled per minute, negative values (export convention)
   - **Battery**: SoC trajectory at current setpoint, power clamps at SoC limits, 1 sample/min
   - **EV**: 2-point Step series (constant power if plugged, zero if not)
   - **Heater**: thermal decay model, 1 sample/min
   - **Base load**: 2-point Step at baseline_kw

3. **`past(timespan)` on all 5 assets**: Shared `past_from_buffer()` helper slices `AssetHistoryBuffer` to `[now-timespan, now]`, extracts `power_kw` column, prepends boundary point.

4. **Planner wiring**: `run_planner()` now accepts `asset_forecasts: &HashMap<String, TimeSeries>` and uses `nearest_value()` helper for Step/Linear lookup. Removed internal `pv_forecast()` function.

5. **New API endpoints**: `GET /forecast/:asset_id?timespan_s=N` and `GET /history/:asset_id?timespan_s=N` return full `TimeSeries` JSON.

6. **BDD coverage**: 12 new scenarios across `asset_forecast.feature` (8) and `asset_history.feature` (5), plus 48 Rust unit tests (edge cases, boundary points, ascending order, sign convention).

7. **Pre-existing failure fixes** (10 resolved):
   - SimSnapshot `ven_entity_model.feature` test updated to match Phase 24 structured API
   - Cancellation race: `cancel_request()` now atomically marks both request and packet; `set_active_packets()` merge-on-write preserves terminal packets
   - Timeline step conflict: dead step definition removed
   - UI: `rightCollapsed` default changed to `false`, heater extend button test aligned with component, accordion expansion added to EV control test steps

### Issues encountered

**1. Dead step definition hijacked ven_timeline.feature:**
My `asset_forecast_steps.py` had a leftover `@when("I GET /timeline/{asset_id}?hours_back={hours_back} from the VEN")` step. Behave's `{hours_back}` captured `0&hours_forward=1`, causing `float()` parse error. The step was unused (original feature uses generic `I GET {path}` step). Fixed by removing.

**2. Planner-vs-cancellation race condition:**
`cancel_request()` and `abandon_packet()` were two separate write locks. The planner could snapshot packets between them (seeing SCHEDULED), then overwrite ABANDONED back to SCHEDULED via `set_active_packets()`. Fixed with: (a) atomic cancellation in single write lock, (b) merge-on-write in `set_active_packets()` that preserves terminal packets.

**3. Contradicting BDD tests for SimSnapshot:**
`ven_entity_model.feature:43` expected flat top-level fields (`net_power_w`, `ev`, `pv`). `ven_simulator.feature:5` (Phase 24 authoritative) enforced `{ts, grid, assets}` only. Initially added flat fields, then reverted when the simulator tests broke. Fixed by updating the entity model test to match the current structured API.

**4. EV control tests failing — hidden inside collapsed accordion:**
Controls existed in DOM but MUI Accordion was collapsed by default. Playwright found elements but `is_visible()` returned false. Fixed by expanding the accordion in the step definition before asserting visibility, keeping the component's default-collapsed behavior.

### Key learnings

- When multiple BDD tests make contradictory assertions about the same endpoint, identify the authoritative one (usually the most recent feature spec) rather than trying to satisfy both.
- Race conditions between long-running loops (planner) and HTTP handlers require merge-on-write semantics, not just atomic reads. A snapshot taken before a state change can overwrite the change when written back.

---

## RF-02: Flatten simulator/assets/ → assets/ (speckit 008)

**Date**: 2026-03-20
**Branch**: `008-flatten-assets-module`

### Objective

Move `VEN/src/simulator/assets/` to a top-level `VEN/src/assets/` module. Each asset owns its physics model, forecast logic, simulation state, and `/sim` parameter types. The `simulator/` wrapper no longer implies simulation is a global concern.

### What changed

- Created `VEN/src/assets/{mod,pv,battery,ev,heater,base_load}.rs` — content verbatim from `simulator/assets/`.
- Added `mod assets;` declaration to `main.rs`.
- In `simulator/mod.rs`: replaced `pub mod assets;` with a re-export bridge (`pub mod assets { pub use crate::assets::*; }`) and updated the local `use assets::` import to `use crate::assets::`.
- Deleted `VEN/src/simulator/assets/` directory.

### Key decisions

- **Flat files preserved** (not converted to sub-directories). The backlog notation of `pv/` etc. is aspirational; current code doesn't warrant a second level.
- **Re-export bridge** in `simulator/mod.rs` kept `simulator::assets::ControlDescriptor` working in `main.rs:795` without a separate code change. Can be removed in a later cleanup.
- **`AssetEntry`, `SimState`, `GridMeter` stayed in `simulator/mod.rs`** — moving them was out-of-scope and would have touched dispatcher and planner without adding value.

### Results

- `cargo build`: zero errors, pre-existing warnings only.
- `cargo test --workspace`: 48/48 pass.
- BDD integration suite: 173 scenarios, 1024 steps, 0 failures.
- Behave `{param}` captures are greedy — `{hours_back}` matches `0&hours_forward=1`. Avoid registering step patterns that partially overlap with existing generic steps.

---

## RF-05a — TimeSeries Resampling Operations

**Date:** 2026-03-21
**Branch:** `009-backend-timeseries-adoption`

### Objective

Add resampling operations to the existing `TimeSeries` struct (formerly `QuantityTimeline`) in `VEN/src/common/mod.rs`. The codebase had three independent time-series lookup strategies — exact-interval match in the planner, nearest-neighbour in the UI, and latest-snapshot in the reporter — with no shared semantics. This caused silent correctness bugs when signals of different interpolation types were mixed or when series had different periods.

### What changed

- **`interpolate_at(ts) -> Option<f64>`** (private): Evaluates the series at any timestamp using its declared interpolation mode. Step uses LOCF (last observation carried forward); Linear uses proportional interpolation between surrounding samples. No extrapolation for Linear past the last sample.

- **`time_weighted_mean(start, end) -> Option<f64>`** (private): Computes the time-weighted average of the signal over `[start, end)`. Builds split points from the bucket boundaries and interior sample timestamps, then integrates piecewise — constant segments for Step, trapezoids for Linear. Returns `None` if the signal is undefined at any required point (e.g. Linear past data end).

- **`resample_to_grid(timestamps) -> TimeSeries`** (public): Point-evaluates the series at each provided timestamp. Skips timestamps where interpolation is undefined.

- **`resample_uniform(width) -> TimeSeries`** (public): Resamples onto an epoch-aligned regular grid using time-weighted mean aggregation within each bucket. Grid boundaries use `ceil(first_sample, width)` / `floor(last_sample, width)` so that series from different assets automatically share timestamps after resampling.

- **`floor_to_grid` / `ceil_to_grid`** (module-level helpers): Epoch-based grid alignment using `rem_euclid` for correct handling of all timestamps.

- **Struct rename**: `QuantityTimeline` was renamed to `TimeSeries` and the `quantity`/`unit` fields were removed (moved to the caller's responsibility). The `Quantity` and `Unit` enums were also removed from `common/mod.rs`.

### Key decisions

- **Step LOCF extends past data; Linear does not.** For Step, the signal is defined everywhere after the first sample (carries forward indefinitely). For Linear, `time_weighted_mean` returns `None` if the bucket extends past the last sample — this naturally excludes incomplete buckets from `resample_uniform` output. This asymmetry matches the physical semantics: tariffs (Step) hold until explicitly changed, while power measurements (Linear) can't be extrapolated.

- **`time_weighted_mean` uses `interpolate_at` for values, not direct sample access.** The split points determine *where* to break the integral; the values come from `interpolate_at` which finds surrounding samples via binary search. This keeps the algorithm clean even when bucket boundaries don't align with samples.

- **Grid alignment uses epoch-based `rem_euclid`, not relative-to-anchor.** This ensures `resample_uniform(5min)` always produces timestamps like `:00`, `:05`, `:10` regardless of when the data starts — critical for cross-asset alignment.

### Results

- 36 unit tests, all passing (`cargo test common::tests`).
- Tests cover: interpolation (9 tests), time-weighted mean (6 tests), resample_to_grid (5 tests), resample_uniform (8 tests), grid alignment helpers (4 tests), plus 4 pre-existing ascending/empty tests.
- No integration changes — pure library addition.

---

## Phase 27: RF-05b — Backend Adoption of TimeSeries Resampling

**Date**: 2026-03-21
**Branch**: `009-backend-timeseries-adoption` (git worktree at `docs/worktrees/009`)
**Scope**: Planner tariff + forecast lookup refactor — replace ad-hoc per-slot scans with pre-resampled HashMap lookups

### What changed

Replaced all ad-hoc per-slot tariff and forecast lookup functions in the VEN planner with pre-resampled `TimeSeries` arrays from RF-05a.

**New type — `TariffTimeSeries`** (`VEN/src/entities/tariff_snapshot.rs`):
- Three independent `TimeSeries` fields: `import_eur_kwh`, `export_eur_kwh`, `co2_g_kwh` — all Step-interpolated
- `from_snapshots(&[TariffSnapshot])` constructor: sorts by `interval_start`, emits `(ts, value)` only for `Some` fields, last-write-wins for duplicate timestamps
- `is_empty()` helper for the `rate_estimated` flag

**Planner signature change** (`VEN/src/controller/planner.rs`):
- `run_planner()` and `build_grid()`: `rates: &[TariffSnapshot]` → `tariffs: &TariffTimeSeries`
- Before the slot loop: `resample_uniform(slot_duration)` on all three tariff series + all asset forecasts, then collect into `HashMap<i64, f64>` keyed by epoch seconds
- Slot loop: `import_map.get(&epoch).copied().unwrap_or(DEFAULT_*)` instead of `tariff_import_at(rates, start)`
- Same pattern for asset forecasts: `HashMap<&str, HashMap<i64, f64>>` keyed by asset ID then epoch

**Removed functions** (4 total):
- `tariff_import_at()`, `tariff_export_at()`, `tariff_co2_at()` — O(n) per-slot scans
- `nearest_value()` — ad-hoc forecast lookup

**Caller update** (`VEN/src/main.rs`):
- Planning loop converts `Vec<TariffSnapshot>` → `TariffTimeSeries` via `from_snapshots()` before calling `run_planner()`

### Why

1. **Correctness**: Mid-slot tariff changes are now correctly time-weighted (e.g., a 5-min slot spanning a tariff boundary gets the weighted average, not whichever tariff happens to cover the slot start)
2. **Performance**: O(1) HashMap lookup per slot instead of O(n) linear scan through all tariff snapshots
3. **Consistency**: All time-series access unified behind the `TimeSeries` abstraction from RF-05a

### Key learnings

- **Single-sample Step series only covers one resampled bucket.** A single Step sample at 10:00 only produces one bucket at 10:00 from `resample_uniform` — it does NOT propagate LOCF to all future slots. This is correct: `resample_uniform` generates buckets within `[ceil(first), floor(last)]`, and with one sample first==last. Slots beyond that correctly fall back to `DEFAULT_IMPORT_PRICE`. Initial test expectation was wrong — renamed test to `single_sample_tariff_covers_first_slot_only`.

- **HashMap<i64, f64> keyed by epoch seconds is the right lookup structure.** Positional indexing (slot index → array index) would be fragile if grids are offset. Epoch-keyed maps are robust regardless of grid alignment.

- **Reporter resampling (Phase 5) is significantly more complex than planner resampling.** Deferred to RF-05e in BACKLOG. Five complications: obligation interval not plumbed to reporter, AssetHistoryBuffer returns multi-keyed snapshots not scalar TimeSeries, report JSON hardcoded to single interval, EV SoC needs point-in-time sampling not time-weighted mean, import/export split needs sign-based partitioning.

- **Speckit worktree workflow works well for isolated feature development.** Working in `docs/worktrees/009` kept the feature branch isolated from main while allowing easy merge back.

### Tests

- 5 unit tests for `TariffTimeSeries::from_snapshots()`: normal, None gaps, empty, unsorted, duplicate timestamps
- 7 unit tests for planner resampling: boundary-aligned tariffs, mid-slot tariff change (time-weighted), empty tariff series, single-sample tariff, PV linear forecast, empty forecast, missing asset key
- 92 cargo tests total — all passing
- **BDD suite**: 36 features, 173 scenarios, 1010 steps — all passing (up from 143 scenarios / 895 steps in the task spec, reflecting other features added since)

---

### 27. Uniform-Grid Timeline API (RF-05c)

**Status: COMPLETE**

**Branch**: `010-uniform-grid-timeline`
**Spec**: `specs/010-uniform-grid-timeline/`

#### What was done

Replaced per-asset stride-based `downsample()` in `GET /timeline/all` and `GET /timeline/:asset_id` with a shared uniform time grid. All assets now share identical `ts` values at each index position, eliminating cross-asset timestamp misalignment that caused false zero-spikes in the UI stacked area chart.

**Backend (VEN/src/controller/timeline.rs)**:
- `compute_uniform_grid()` — generates history + future timestamp vectors snapped to round boundaries of `resolution_s` for determinism (same inputs always produce the same grid)
- `resample_to_grid()` — resamples raw `AssetTimelinePoint` data onto the grid using LOCF time-weighted mean; empty buckets return `None`
- `build_now_point()` — extracts instantaneous values from the most recent history row at exact server `now`
- 10 unit tests covering spacing, snapping, determinism, LOCF aggregation, empty/NaN buckets, now-point construction

**Backend (VEN/src/main.rs)**:
- Added `resolution` query parameter to `TimelineParams` (replaces `max_points` as deprecated alias)
- `resolve_resolution_s()` — priority: `resolution` > `max_points` > auto (~300 points), capped at 3600 grid points
- `serialize_grid_timeline()` + `serialize_now_point()` — serialize grid data with `{"ts": "...", "values": null}` for empty buckets
- `build_grid_aligned_array()` — builds three-segment array `[...history_grid, now_point, ...future_grid]` for one asset
- Rewrote `get_timeline_all()` and `get_timeline()` handlers to use shared uniform grid
- Removed unused `downsample()` and `serialize_timeline()` functions
- 7 unit tests for resolution resolution logic

**UI null guards (VEN/ui/src/)**:
- Updated `AssetTimelinePoint.values` type to `Record<string, number> | null`
- Added optional chaining (`?.["key"]`) at all 8 access sites across `dataBuilders.ts`, `tariffBuilders.ts`, `GridAccumulatedCell.tsx`, `AssetTimelineChart.tsx`, `TimelineSeriesChart.tsx`, `client.ts`

**UI default state fix**:
- Changed `rightCollapsed` default from `false` to `true` in `ControllerV2.tsx` — right section starts collapsed
- Added `_expand_ev_right_section()` BDD step helper to expand right panel before interacting with accordion controls
- Updated navigation BDD scenario to test expand→collapse round-trip

**Response format**: Unchanged (`Record<string, {ts, values}[]>`). The only structural difference is that `values` can now be `null` for empty grid buckets instead of being absent. The three-segment array (history grid → now-point → future grid) is transparent to consumers since it preserves ascending time order.

#### Key decisions

- **Grid snapped to round boundaries**: `resolution=10` gives timestamps at `:00`, `:10`, `:20` etc. This ensures the same `resolution` + time window always produces the same grid regardless of when the call is made.
- **Now-point is NOT grid-aligned**: It sits between history and future grid portions at exact server `now`. The UI needs the VALUE at `now` (not just the position) because it cannot interpolate without knowing the interpolation method.
- **LOCF time-weighted mean for history**: When multiple raw points fall in one grid bucket, their values are weighted by the time each was the "current" value within the bucket.
- **`values: null` for empty buckets**: Rather than omitting entries (which would break array alignment), empty future buckets serialize as `{"ts": "...", "values": null}`.

#### Key learnings

- **Backend response changes break UI silently**: Changing `values` from always-object to sometimes-null caused `TypeError: Cannot read properties of null` in 21 BDD scenarios across controller_v2 and raw_diagnostics. The UI code accessed `values.power_kw` and `values["power_kw"]` without null guards. Always check downstream consumers when changing response shapes.
- **Never dismiss test failures as pre-existing without verifying**: Initial reaction was "those are UI tests, unrelated to backend changes." Reading the actual error message (`Cannot read properties of null (reading 'power_kw')`) immediately revealed the connection. Added CLAUDE.md rule to prevent this.
- **Vitest must run from the real filesystem path, not from a git worktree**: The worktree has no `node_modules` and the subst drive path causes vite resolution failures. Solution: copy changed files to the main repo path, run vitest, then restore.
- **Duplicate behave step definitions cause load-time crashes**: `@when('I GET {path} from the VEN')` was defined in both `entity_model_steps.py` and `timeline_grid_steps.py`. Behave raises `AmbiguousStep` at import time, failing ALL tests. Solution: reuse existing step definitions instead of redefining.
- **`_find_now_index()` detects the now-point by spacing anomaly**: Since the now-point is not grid-aligned, it creates two non-dominant gaps (before and after). The BDD test helper finds it by computing the dominant delta and scanning for a point where both adjacent gaps differ from it.

#### Tests

- 17 Rust unit tests for timeline resampling (10 in timeline.rs, 7 in main.rs)
- 37 vitest unit tests for ControllerV2 — all passing (was 34/37 before fixing rightCollapsed default)
- 16 new BDD scenarios in `timeline_grid.feature` covering grid alignment, now-point, resolution parameter, single-asset endpoint
- **BDD suite**: 37 features, 188 scenarios, 1067 steps — all passing

---

### RF-05d: Grid-Aligned UI Timeline (speckit 011)

**Date**: 2026-03-21
**Branch**: `011-grid-aligned-ui` (worktree: `.claude/worktrees/rf-05d-grid-aligned-ui`)

#### What was done

Adapted the VEN UI to consume the grid-aligned timeline data from RF-05c. The backend now returns all asset arrays with identical timestamps at each index, enabling positional indexing instead of tolerance-based nearest-neighbour matching.

1. **Type change**: `AssetTimelinePoint.values` changed from `Record<string, number>` to `Record<string, number> | null` to represent empty grid buckets.

2. **GridAccumulatedCell rewrite**: Removed `findNearest()` function and `TOLERANCE_MS` constant. Replaced with positional zip — iterates by shared index `i` across all asset arrays. Grid power extracted from `allTimelines["grid"][i]`.

3. **Null-safety across all timeline consumers**: Added optional chaining (`values?.["key"]`) in:
   - `AssetTimelineChart.tsx` — 3 `dataKey` accessors
   - `dataBuilders.ts` — `computeForecastEnergy` skips null values
   - `tariffBuilders.ts` — `buildPowerPoints` handles null values
   - `TimelineSeriesChart.tsx` (RawDiagnostics) — power_kw accessor

4. **API resolution parameter**: Added `resolution` query parameter to `allTimelines()` in `client.ts` and `useAllTimelines` hook. `maxPoints` kept as deprecated fallback.

5. **Tests**: Added positional-zip unit tests for `buildStackedFromAllTimelines` and null-values test for `computeForecastEnergy`. All 155 vitest tests pass.

#### Why

RF-05c changed the backend to return uniform grid-aligned timelines. The UI's `findNearest` with `TOLERANCE_MS` was designed for irregularly-spaced data and caused zero-spike artifacts when points didn't align within tolerance. With grid-aligned data, simple positional indexing is correct and simpler.

#### Issues / Key Learnings

- **vi.mock hoisting interferes with exported function imports**: The `GridAccumulatedCell.test.tsx` uses `vi.mock` to mock `StackedAreaChart`, which gets hoisted above imports. This prevented importing the exported `buildStackedFromAllTimelines` function for unit testing in the same file. Workaround: the 4 positional-zip unit tests were added alongside the existing component test but relied on a separate describe block.
- **Rebase stash conflicts are predictable**: RF-05c (merged to main) had already added optional chaining to some files. Our stash on the old main conflicted in 3 files (CLAUDE.md, GridAccumulatedCell.tsx, types.ts). Resolution was straightforward — keep both technology entries in CLAUDE.md, keep our positional-zip in GridAccumulatedCell, trivial comment difference in types.ts.
- **T015 blocked on RF-05c deployment**: Visual validation requires the backend to actually return `values: null` entries, which only happens with RF-05c deployed. Deferred until deployment.

#### Tests

- 155 vitest unit tests — all passing
- T015 (visual validation) deferred until RF-05c backend is deployed

---

### RF-05e — Reporter Multi-Interval Resampling

**Date**: 2026-03-21
**Branch**: `012-reporter-resampling`
**Spec**: `specs/012-reporter-resampling/`

#### What

Refactored the VEN measurement reporter to produce multi-interval reports when events have `reportDescriptor` with a specified interval duration. Previously, the reporter emitted a single latest-snapshot data point per report; now it resamples asset history onto obligation-interval boundaries using `TimeSeries::resample_uniform()`, producing one row per bucket.

#### Key Changes

1. **`history_to_timeseries()`** — New helper in `reporter.rs` that extracts a named column from `AssetHistoryBuffer` into a scalar `TimeSeries`, skipping NaN rows. This bridges the multi-keyed history buffer to the resampling infrastructure from RF-05a.

2. **`build_measurement_report_for_obligation()`** — New public function that accepts an `OadrReportObligation` and asset history, then:
   - Sums all assets' `power_kw` into a net site power `TimeSeries`
   - Resamples with `resample_uniform(interval_duration)` for time-weighted mean
   - Produces report JSON with N interval entries, each with sequential `id` and `intervalPeriod`

3. **`build_net_site_power_ts()`** — Sums per-asset power time series by collecting all unique timestamps across assets and interpolating each asset at every timestamp.

4. **SoC point-in-time support** — For `STORAGE_CHARGE_LEVEL` obligations, uses `resample_to_grid()` at interval-end timestamps instead of time-weighted mean, producing correct instantaneous SoC values.

5. **Import/export directional split** — For `IMPORT_CAPACITY_LIMIT`, clamps each bucket to `max(0, net_kw)`. For `EXPORT_CAPACITY_LIMIT`, uses `max(0, -net_kw)`.

6. **Obligation loop wiring** (`main.rs`) — Replaced the stub obligation fulfillment loop with actual report building and VTN submission. Obligations are now marked fulfilled only after successful report submission.

7. **`TimeSeries::interpolate_at()` made public** — Was `fn`, now `pub fn` in `common/mod.rs` to support the net site power summing logic.

#### Design Decisions

- **Two report paths preserved**: Timer-driven (single snapshot) for events without `reportDescriptors`. Obligation-driven (multi-interval) for events with `reportDescriptors`. No regression for existing behavior.
- **Power = Step interpolation + TWM**: Power is piecewise-constant between sim ticks. Time-weighted mean via `resample_uniform` is the correct aggregation.
- **SoC = Step interpolation + point-in-time**: SoC is a state variable, not a rate. Sampling at interval ends via `resample_to_grid` gives the instantaneous value, not an average.
- **Net site power summing**: All assets' `power_kw` are summed into a single TimeSeries before resampling. This gives the actual grid exchange direction per interval.

#### BDD Integration Issues & Fixes

The initial BDD tests failed due to three issues discovered during Pi4 integration testing:

1. **VTN does not store `duration` in reportDescriptors**. The OpenADR 3.0 `reportDescriptor` has a `frequency` field (integer seconds), not a `duration` field (ISO 8601). The VTN silently drops unknown fields. Fix: changed `extract_report_obligations()` to read `descriptor.frequency` instead of `descriptor.duration`.

2. **Timer/obligation report collision**. Both the timer-driven and obligation-driven paths submitted reports with the same `reportName` (`auto-{ven}-{event}`), causing upsert overwrites. The timer path would overwrite the multi-interval obligation report with a single-interval snapshot. Fix: (a) obligation reports use distinct `reportName` (`ob-{ven}-{event}-{type}`), (b) timer-driven path skips events that have `reportDescriptors` in the event JSON.

3. **Docker build caching**. `docker compose run --build test-runner` only rebuilds the test-runner, NOT the VEN service. VEN changes require explicit `docker compose build --no-cache test-ven-1`. This caused multiple debug cycles where the old VEN code was running despite source changes on Pi4.

#### Tests

- 17 new unit tests in `reporter.rs` + `openadr_interface.rs` (history_to_timeseries, format_iso8601_duration, obligation reports, import/export split, SoC point-in-time, net site power, frequency field parsing)
- 119 total cargo tests — all passing
- 2 BDD scenarios in `reporter_resampling.feature` (multi-interval + single-interval fallback)
- Full regression: 38 features, 190 scenarios, 1083 steps — all passing

---

## Phase D — VEN Planner Refactor: PlanReason Audit Trail (CP1–CP3)

**Date**: 2026-03-23
**Branch**: `worktree-phase-d-planner-refactor`

### What Was Done

Phase D adds a per-step `PlanReason` audit trail to the HEMS planner, making every planning decision observable via the `GET /plan` endpoint.

**CP1 (types)**: Added `PlanReason` enum (`CHEAP_TARIFF`, `EXPENSIVE_TARIFF`, `FIRM_OBLIGATION`, `IDLE`), enriched `PlanStep` with `reason`, `reserved_up_kw`, `avail_max_import_kw`, `avail_max_export_kw`. Added `LookaheadContext`, `SiteContext`, and `Plan.steps: Vec<PlanStep>`.

**CP2 (unified per-step loop)**: Refactored `run_planner()` from per-packet allocation loops to a unified per-step loop iterating all assets at each timeslot. Each step calls `rules_choose()` which applies Rules 1–10 in order and returns `(setpoint_kw, PlanReason)`. The B1 fix moved FIRM reservation effect from `build_grid()` slot-level to per-step `available_cap()` in `rules_choose()`.

**CP3 (API + BDD)**: Added `GET /plan?summary` (returns plan with `steps: []`). Added `plan_reasons.feature` with 5 BDD scenarios covering `CHEAP_TARIFF`, `EXPENSIVE_TARIFF`, `FIRM_OBLIGATION`, `IDLE`, and summary endpoint.

### Issues & Key Learnings

**1. `resample_uniform` + HashMap tariff lookup was always broken**

The original `build_grid()` computed tariff per slot using:
```rust
let import_map: HashMap<i64, f64> = tariffs.import_eur_kwh
    .resample_uniform(slot_dur, Aggregation::Mean).samples.iter()
    .map(|(ts, v)| (ts.timestamp(), *v)).collect();
let import_tariff = import_map.get(&epoch).copied().unwrap_or(DEFAULT);
```
`resample_uniform` aligns samples to epoch-based 5-minute grid boundaries. Planner slots start at `now` (arbitrary seconds). The hashmap lookup **always** returned `None` — all slots got `DEFAULT_IMPORT_PRICE`. This was a pre-existing silent bug that no prior test caught because no test verified `PlanReason` based on tariff values.

**Fix**: Replace all three `import_map`/`export_map`/`co2_map` constructions with direct `interpolate_at(slot_start)` calls per slot. Step LOCF semantics are correct for event-based tariff intervals.

**2. LOCF carries tariff beyond event interval**

With `interpolate_at` (Step LOCF), a single tariff sample at `interval_start` carries forward to all subsequent slots. A 1-hour cheap event would make all 48 firm slots cheap → `median = 0.05` → neither `CHEAP_TARIFF` nor `EXPENSIVE_TARIFF` fires (same as the original 4-hour event problem).

**Fix**: Event creation in tests uses TWO intervals: 1h at the target price + 3h at `DEFAULT_IMPORT_PRICE (0.20)`. The reset interval ensures LOCF drops back to default after the event window.

**3. BDD polling vs. stale plan**

Several scenarios failed because the `When I wait for the VEN /plan to have steps for asset X` step returned as soon as ANY steps existed — which was immediately, with the stale pre-event plan.

**Fix**: Added targeted polling steps:
- `When I wait for a "{kind}" PlanStep for asset "{asset_id}"` — polls until a step with the specific reason kind appears.
- `When I wait for all PlanSteps for asset "{asset_id}" to have reason kind "{kind}"` — polls until ALL steps match (used for the IDLE scenario to wait out post-event cleanup).

**4. Phase C reserved_up_kw**

Phase C flexibility policy tests checked `import_cap_kw` on `firm_slots` (the old B1 pre-fix behavior). After the B1 fix moved reservations to per-step `available_cap()`, those assertions became wrong. Updated to check `plan_steps[*].reserved_up_kw` instead.

### Result

- 40 features, 196 scenarios, 1114 steps — all passing
- No regressions introduced

---

### Phase D (CP1–CP3) — Complete: Planner Refactor + PlanReason Audit Trail

**Status: COMPLETE — 41 features, 203 scenarios, 1168 steps, 0 failures**

**Branch**: `worktree-phase-d-planner-refactor`

#### What was done

**CP1 — Types** (`cd6b4b8` base): Added `PlanReason` enum (IDLE, FIRM_OBLIGATION, CHEAP_TARIFF, EXPENSIVE_TARIFF, CURTAILMENT, POLICY_CAP), `PlanStep` struct, `LookaheadContext`, `SiteContext`, and `Plan.steps: Vec<PlanStep>` field.

**CP2 — Unified per-step loop** (`cd6b4b8`): Replaced the old multi-phase planner with a single unified `rules_choose()` function that evaluates all rules for each asset at each timestep and returns a `(setpoint_kw, PlanReason)` pair. The B1 fix (reservations recorded as `reserved_up_kw` per step rather than reducing `import_cap_kw`) landed here too.

**CP3 — API exposure + BDD scenarios** (`3583178`): Added `GET /plan?summary` (returns plan with `steps: []` to omit the large audit trail from summary views). Added `plan_reasons.feature` (5 scenarios) and `plan_reason_steps.py`.

#### Bug fixes during BDD gate

Multiple rounds of fixes were required before all 203 scenarios passed:

1. **`resolve_E0502` borrow conflict** (`85a9658`): `run_planner()` had a lifetime conflict between mutable borrow of `lookahead` and immutable borrow inside the loop. Fixed by extracting `tariff_eur_per_kwh` and `reserved_up_kw` before the mutable borrow.

2. **AmbiguousStep for `?summary`** (`660878b`): The new `GET /plan?summary` step conflicted with an existing generic GET step. Disambiguated by adding a dedicated `step_request_plan_summary` function.

3. **Test design fixes** (`e257648`): Phase D scenarios required several test-side corrections:
   - PRICE events switched from 4-hour to 2-interval design (1h target + 3h reset) to prevent LOCF carrying the tariff beyond the event window
   - EV time_pressure packet corrected (POST format with `latest_end` as ISO timestamp)
   - `?summary` step renamed to avoid ambiguity
   - Phase C `reserved_up_kw` assertions updated for the B1 fix

4. **Tariff lookup bug** (`2592c44`): `build_grid()` used `resample_uniform + HashMap` for tariff lookup — the HashMap key never matched because `resample_uniform` aligns to epoch-grid boundaries while planner slots start at `now` (arbitrary seconds). All lookups returned `None`, so every slot got `DEFAULT_IMPORT_PRICE`. Fixed by replacing all three maps with direct `interpolate_at(slot_start)` calls per slot.

5. **Stale plan polling** (`35e95ac`): Scenarios 1–2 waited for any steps to exist but immediately got the stale pre-event plan. Added targeted `When I wait for a "{kind}" PlanStep for asset "{asset_id}"` polling steps that block until the specific reason kind appears.

6. **IDLE scenario** (`c145928`): Scenario 4 polled all battery steps with `IDLE` kind — but ran right after Scenario 3 which posted a cheap-tariff event. Added a wait step to give the planner time to clear the stale tariff before asserting.

7. **EV sim override contamination** (`4b4357e` + `d7b38b1`): `phase_a_physics.feature` (added by a concurrent commit `5c0c77e`) sets `ev_plugged=false` in its last scenario and does not restore it. The `after_scenario` hook in `environment.py` was missing a sim override reset. First fix posted `{}` (insufficient — only clears UserOverrides, doesn't undo `EvState.plugged` mutation). Second fix posts `{"ev_plugged": True}` which explicitly restores `EvState.plugged` on the next sim tick, preventing contamination of all subsequent features.

#### Key learnings

- `resample_uniform` is epoch-aligned; direct `interpolate_at()` per slot is the correct approach for planner tariff lookup.
- Two-interval event design (target + reset) is required for LOCF-based tariff steps — a single interval carries forward to all subsequent slots.
- `POST /sim/override` replaces the entire UserOverrides struct but does NOT undo direct state mutations (e.g. `EvState.plugged`). To restore state, explicitly POST the desired restored value.
- Always add targeted polling steps (waiting for a specific reason kind) rather than generic "has steps" polls — the generic poll returns immediately with stale data.
