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

*Last updated: 2026-02-08*
