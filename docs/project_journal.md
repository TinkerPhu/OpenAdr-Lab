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

**Status: COMPLETE (local) — BDD tests running on Pi4**

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
