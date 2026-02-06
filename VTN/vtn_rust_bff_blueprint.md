# VTN Rust BFF Blueprint (React + MUI UI)

**Scope:** This document is a build blueprint for a **Rust BFF (Backend-for-Frontend)** that powers a **VTN Web UI** for an OpenADR 3.x deployment based on **OpenLEADR/openleadr-rs**. It is written to be usable by both humans and AI tooling.

**Target environment:**
- Single **Raspberry Pi 4** running:
  - VTN (openleadr-rs server)
  - PostgreSQL (VTN persistence)
  - 3–5 VEN containers (separate workstream)
  - This **BFF** container + a **React + MUI** UI
- Expected VEN telemetry cadence: **every 5 minutes** per VEN (aggregate ≈ 1/minute at 5 VENs).

---

## 1. Background and key sources

### OpenADR and resource model
OpenADR 3 defines a **REST API** with CRUD-style endpoints for resources like **programs, events, reports, VENs, and resources**. citeturn0search3turn0search6

### openleadr-rs project
The OpenLEADR/openleadr-rs repository provides:
- A Rust **VTN server** implementation
- A Rust **VEN client library**
- A Rust **wire/message model** crate
It states support for managing OpenADR resources through a RESTful interface and uses PostgreSQL in its standard setup. citeturn0search0turn0search9

### openleadr-client
`openleadr-client` is a Rust library for interacting with an OpenADR 3.x VTN, mainly as a thin wrapper around the VTN REST interface. citeturn0search1turn0search5

### openleadr-vtn OAuth configuration
The VTN supports an **internal OAuth provider** (and external) configured via environment variables such as `OAUTH_TYPE`, `OAUTH_KEY_TYPE`, etc. citeturn0search2

---

## 2. System decisions (from the conversation)

### 2.1 Database choice: PostgreSQL
**Decision:** Keep the VTN backed by **PostgreSQL**, not SQLite.

**Why:**
- Matches upstream happy path (migrations/fixtures/dev workflow) in openleadr-rs
- Better concurrency and operational predictability for a multi-container setup on a Pi

The VTN persists core OpenADR resources in Postgres as part of its implementation setup. citeturn0search0

### 2.2 BFF required (browser must not hold secrets)
**Decision:** UI talks to **BFF**, not directly to VTN.

**Why:**
- VTN access requires OAuth client credentials; browser cannot safely store a `client_secret`
- BFF centralizes auth, caching, normalization, and “compose event” logic

The VTN supports OAuth configuration, including INTERNAL OAuth, via env vars. citeturn0search2

### 2.3 Polling-first architecture
**Decision:** UI uses polling (via BFF) rather than push/webhooks initially.

**Why:**
- Many OpenADR stacks are CRUD-over-REST first; push mechanisms are optional.
- We design for eventual upgrades (SSE/WebSockets), but polling is simplest.

OpenADR 3’s REST model emphasizes HTTP operations on resources. citeturn0search3

### 2.4 Reports/telemetry: “prepare now, persist later”
**Decision:** BFF provides **stable endpoints** for report definitions and telemetry, but **does not require its own DB initially**.
- If VTN exposes sufficient history queries and persists report datapoints, BFF can pass through.
- If not, add a telemetry store later (Postgres table or Timescale).

OpenADR describes reports as first-class resources in the model; the specifics of datapoint retention vary by implementation. citeturn0search3turn0search6

---

## 3. Responsibilities of the Rust BFF

### 3.1 Core responsibilities (MVP)
1. **OAuth client-credentials** against the VTN and bearer-token injection
2. **Read APIs** for UI:
   - programs, VENs, resources, events
3. **Write APIs** for UI:
   - compose and create events (target multiple VENs/resources)
4. **Normalization**: convert VTN shapes into stable UI DTOs
5. **Caching**: TTL cache to reduce VTN load under polling
6. **Audit-friendly logging**: request IDs, upstream latency, sanitized errors

### 3.2 Optional/phase-2 responsibilities
- Telemetry persistence (time-series store)
- Role-based access control
- Push updates via SSE/WebSockets
- Advanced event lifecycle management (cancel/update, templates, schedules)

---

## 4. BFF API (UI contract)

All endpoints are under `/api`. The BFF should return consistent JSON errors:
```json
{ "error": { "code": "VTN_UNAVAILABLE", "message": "..." , "details": {...} } }
```

### 4.1 Health and meta
#### `GET /api/health`
Purpose: liveness + readiness summary. Includes VTN reachability and auth health.
Response:
```json
{
  "time": "2026-02-05T12:00:00Z",
  "bff": { "ok": true, "version": "0.1.0" },
  "vtn": { "reachable": true, "authOk": true, "baseUrl": "http://vtn:3000" }
}
```

#### `GET /api/config`
Purpose: expose **non-sensitive** runtime config and feature flags to the UI.
Response:
```json
{
  "polling": { "dashboardSeconds": 5, "eventsSeconds": 10 },
  "features": { "eventWriteEnabled": true, "telemetryMode": "passthrough" }
}
```

---

### 4.2 Programs
#### `GET /api/programs`
Purpose: list programs for navigation and event composition.

#### `GET /api/programs/:id`
Purpose: program details page (including summary counts and recent events if available).

**DTO guidelines**
- Always include: `id`, `name` (if present), `createdAt`, `updatedAt` (if present).
- If the VTN doesn’t have `name`, show `id` as primary label.

---

### 4.3 VENs
#### `GET /api/vens`
Purpose: list VENs, show status/lastSeen, enable target selection for events.

#### `GET /api/vens/:id`
Purpose: VEN detail, including resources and relevant events.

**DTO guidelines**
- Include computed fields when possible:
  - `resourceCount`
  - `lastSeen` (best effort; may come from last report/event ack once available)
  - `status` (derived: OK/STALE/UNKNOWN)

---

### 4.4 Resources
#### `GET /api/resources`
Purpose: list resources across VENs, for drill-down and targeting.

#### `GET /api/resources/:id`
Purpose: resource detail, show its VEN, latest telemetry, and event applicability.

---

### 4.5 Events
#### `GET /api/events?status=&programId=&venId=&from=&to=&limit=`
Purpose: events list with filtering and polling.

Recommended `status` values in UI DTO:
- `UPCOMING`, `ACTIVE`, `COMPLETED`, `CANCELLED`, `UNKNOWN`

BFF should compute `status` based on event timing fields if VTN doesn’t provide a stable status.

#### `GET /api/events/:id`
Purpose: event detail including:
- full event payload
- resolved targets
- (future) telemetry overlays for verification

---

## 5. Event composition and controls

### 5.1 Why the BFF composes events
The UI wants simple control semantics (targets + timing + payload). The VTN schema may be more verbose. OpenADR 3 uses REST resources; the BFF acts as a schema/UX adapter. citeturn0search3

### 5.2 Draft input model (UI → BFF)
#### `POST /api/events/preview`
Purpose: validate and show exactly what will be sent to the VTN.
Request:
```json
{
  "programId": "prog-1",
  "targets": { "venIds": ["ven-1","ven-2"], "resourceIds": [] },
  "timing": { "start": "now", "durationMinutes": 30 },
  "payload": { "type": "LOAD_REDUCTION", "valueKw": 10 }
}
```
Response:
```json
{
  "valid": true,
  "warnings": [],
  "vtnRequest": { "...": "exact JSON to POST to VTN" }
}
```

#### `POST /api/events`
Purpose: create an event on the VTN (send to multiple VENs/resources).
- Same request body as preview.
- BFF performs server-side validation and then calls VTN.
Response: created event DTO.

#### `POST /api/events/:id/cancel` (optional)
Purpose: cancel an event if supported by VTN.

### 5.3 Validation rules (MVP)
- `programId` must be provided
- at least one target (`venIds` or `resourceIds`)
- duration bounds (e.g., 1–1440 minutes)
- start time not in the past (allow small drift)
- payload whitelist (start small; expand later)
- feature flag `EVENT_WRITE_ENABLED=true` must be set to allow writes

---

## 6. Reports and telemetry (future-ready)

OpenADR 3 treats reports as resources; implementations vary on how datapoints are stored and queried. citeturn0search3turn0search6

### 6.1 Report definitions
#### `GET /api/reports?programId=&venId=`
Purpose: list report definitions the VTN knows about (metadata/capabilities).

#### `GET /api/reports/:id`
Purpose: report definition details.

### 6.2 Telemetry snapshots and time-series
#### `GET /api/telemetry/latest?venId=&resourceId=`
Purpose: show latest values on dashboard and VEN/resource detail pages.

#### `GET /api/telemetry/range?venId=&resourceId=&metric=&from=&to=`
Purpose: charting.

### 6.3 Telemetry modes
Configured via `TELEMETRY_MODE`:
- `passthrough`: call VTN for telemetry when available; otherwise return `NOT_AVAILABLE`
- `store`: persist incoming samples (from VEN submissions or VTN polling) into Postgres tables

**Note:** Whether the VTN persists report datapoints long-term must be verified against the running openleadr-vtn behavior. This is a known open item (see section 11).

---

## 7. OAuth and security model

### 7.1 VTN OAuth (internal)
The VTN supports OAuth configuration through env vars such as:
- `OAUTH_TYPE` = `INTERNAL` or `EXTERNAL`
- `OAUTH_KEY_TYPE` (`HMAC` default) and related secrets/keys citeturn0search2

### 7.2 BFF OAuth to VTN
**BFF obtains bearer tokens** via **client credentials** and attaches:
`Authorization: Bearer <token>`

Token handling requirements:
- cache token in memory
- refresh before expiry (use `expires_in` with a safety margin)
- on 401: refresh once and retry the request once

### 7.3 “Minimum security” for the UI
Recommended baseline:
- BFF and VTN on the same Docker network; only BFF exposed to LAN
- Optional API key for UI calls (`BFF_API_KEY`) to prevent casual access
- No secrets in browser
- TLS optional for LAN-only lab; required if exposed beyond trusted network

---

## 8. Configuration

### 8.1 BFF environment variables
Required:
- `BFF_LISTEN_ADDR` (default `0.0.0.0:8090`)
- `VTN_BASE_URL` (e.g., `http://vtn:3000`)
- `VTN_OAUTH_TOKEN_URL` (default `${VTN_BASE_URL}/oauth/token` — verify in your VTN)
- `VTN_CLIENT_ID`
- `VTN_CLIENT_SECRET`

Recommended:
- `EVENT_WRITE_ENABLED` (`true|false`)
- `CACHE_TTL_SECONDS_PROGRAMS` (default 30)
- `CACHE_TTL_SECONDS_VENS` (default 10)
- `CACHE_TTL_SECONDS_EVENTS` (default 5)
- `TELEMETRY_MODE` (`passthrough|store`)
- `BFF_API_KEY` (optional)
- `RUST_LOG` (`info` default)

If telemetry store enabled (`TELEMETRY_MODE=store`):
- `TELEMETRY_DB_URL` (can be same Postgres instance; separate schema recommended)

### 8.2 VTN environment variables (reference)
From `openleadr-vtn` docs, OAuth env vars include: citeturn0search2
- `OAUTH_TYPE` (`INTERNAL` | `EXTERNAL`)
- `OAUTH_KEY_TYPE` (`HMAC` | `RSA` | `EC` | `ED`)
- `OAUTH_BASE64_SECRET` (required for HMAC; >= 256-bit)
- `OAUTH_PEM` (required for non-HMAC key types)

---

## 9. Caching and polling behavior

### 9.1 Why caching is needed
The UI polls frequently (dashboard 5s, events 10s). Without caching, each UI client causes repeated upstream calls.

### 9.2 Suggested TTLs
- programs: 30–60s
- VENs/resources: 10–30s
- events list: 5–10s (depending on desired “liveness”)

### 9.3 Cache invalidation
- After event creation/cancel: invalidate events lists and relevant event detail cache

---

## 10. Implementation plan (Rust)

### 10.1 Technology stack
- **axum** for HTTP routing
- **reqwest** for VTN calls
- **tokio** runtime
- **serde** for JSON models
- **tower-http** for CORS, tracing, request IDs
- Optional cache:
  - simple TTL map or `moka` cache

### 10.2 Project structure
```
bff/
  Cargo.toml
  src/
    main.rs
    config.rs
    error.rs
    auth/
      mod.rs
      oauth_client.rs
    vtn/
      mod.rs
      client.rs
      adapter.rs      # VTN endpoint mapping
      models.rs       # raw VTN models (serde)
    dto/
      mod.rs
      ui_models.rs    # stable UI DTOs
      mappers.rs
    routes/
      mod.rs
      health.rs
      programs.rs
      vens.rs
      resources.rs
      events.rs
      reports.rs      # stubbed
      telemetry.rs    # stubbed
    cache/
      mod.rs
```

### 10.3 Adapter layer (important)
Create `VtnAdapterOpenleadrRs` that owns:
- endpoint paths
- query parameters
- response shape parsing quirks

This protects the UI from upstream changes.

---

## 11. Known open items / missing information

These must be resolved during implementation:

1. **Exact VTN endpoint paths**
   - Token URL and resource URLs may differ from defaults shown in OpenADR slides
   - The BFF should make token URL configurable and have integration tests against your deployed VTN. citeturn0search3

2. **Exact VTN event schema**
   - The BFF’s `EventDraft` must be mapped into the VTN’s required JSON schema.
   - Implement `POST /api/events/preview` early to validate and iterate.

3. **Report datapoint persistence behavior**
   - Verify whether openleadr-vtn persists report datapoints and how they can be queried.
   - Decide whether BFF needs telemetry persistence (`TELEMETRY_MODE=store`).

4. **UI authentication**
   - Minimal: `BFF_API_KEY`
   - Later: real auth (OIDC) if needed.

---

## 12. Acceptance criteria (MVP)

1. UI can load:
   - dashboard counts
   - programs list/detail
   - VENs list/detail
   - events list/detail
2. Event Composer:
   - preview shows VTN request JSON
   - create event sends to multiple VEN targets and returns created event
3. No secrets appear in browser traffic or UI code
4. Polling works smoothly with caching (VTN not overloaded)
5. All endpoints return consistent JSON errors

---

## 13. Appendix: Rationale for Rust BFF on Raspberry Pi

**Why Rust (axum) for the BFF:**
- Fits the existing Rust-based ecosystem (openleadr-rs, openleadr-client)
- Predictable runtime footprint on Raspberry Pi
- Strong typing and controlled error handling for security-sensitive OAuth plumbing

`openleadr-client` exists specifically to wrap VTN REST APIs into Rust calls when needed. citeturn0search1

---

*End of blueprint.*
