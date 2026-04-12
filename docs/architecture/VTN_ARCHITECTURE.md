# VTN Architecture

**Authoritative reference for VTN, BFF, OpenADR message sequences, provisioning, and deployment.**
Domain vocabulary is in [docs/REQUIREMENTS.md](../REQUIREMENTS.md).
VEN architecture is in [docs/architecture/VEN_ARCHITECTURE.md](VEN_ARCHITECTURE.md).

---

## 1. Component Overview

```
┌────────────────────────────────────────────────────────────────────┐
│                         Pi4-Server (Docker)                        │
│                                                                    │
│  ┌──────────────────────────────────────────────────────────────┐  │
│  │                       VTN Stack                              │  │
│  │                                                              │  │
│  │  ┌─────────────────┐       ┌──────────────────────────────┐  │  │
│  │  │  openleadr-rs   │       │  PostgreSQL 16               │  │  │
│  │  │  (VTN Server)   │◄─────►│  (vtn-db-1, port 8201)      │  │  │
│  │  │  port 8200      │       └──────────────────────────────┘  │  │
│  │  └────────┬────────┘                                         │  │
│  │           │  OpenADR 3 REST                                  │  │
│  │  ┌────────▼────────┐                                         │  │
│  │  │  VTN BFF        │  ← dual-credential Rust/Axum proxy      │  │
│  │  │  port 8220      │                                         │  │
│  │  └────────┬────────┘                                         │  │
│  │           │  HTTP                                            │  │
│  │  ┌────────▼────────┐                                         │  │
│  │  │  VTN UI         │  ← React + nginx                        │  │
│  │  │  port 8221      │                                         │  │
│  │  └─────────────────┘                                         │  │
│  └──────────────────────────────────────────────────────────────┘  │
│                                                                    │
│  ┌──────────────────────────────────────────────────────────────┐  │
│  │  VEN Instances (per VEN)                                     │  │
│  │  ven-ven-1-1 : 8211  |  ven-ven-2-1 : 8212  |  ven-ven-3-1 : 8213  │
│  │  ven-ui-1    : 8214                                          │  │
│  └──────────────────────────────────────────────────────────────┘  │
│                                                                    │
│  Docker network: vtn_openadr-net (external, shared by VEN compose) │
└────────────────────────────────────────────────────────────────────┘
```

---

## 2. VTN Server (openleadr-rs)

**Implementation:** Rust, [openleadr-rs](https://github.com/OpenLEADR/openleadr-rs) — git submodule
at `openleadr-rs/`, tracking fork `TinkerPhu/openleadr-rs` (upstream: `OpenLEADR/openleadr-rs`).

**Responsibilities:**
- OAuth2 authorization server (`POST /auth/token`)
- OpenADR 3 API provider (programs, events, reports, VENs, resources)
- Program management (targets, enrollment)
- Event lifecycle (create, update, delete)
- Report ingestion and storage

**Database:** PostgreSQL 16 (`vtn-db-1`). Auto-migrates on first boot (15 tables via SQLx).
Persistent Docker volume required.

**Authentication:**
- Token endpoint: `POST /auth/token` (NOT `/oauth/token`)
- Token TTL: 2,592,000 s (30 days)
- Fixture users: `any-business`, `ven-manager`, `user-manager`, `business-1`, `ven-1`

**Field names (pass-through, no DTO normalisation):**
`programName`, `programID`, `createdDateTime`, `venName`, `eventName` — upstream spec names used at all layers.

---

## 3. BFF — Dual-Credential Pattern

The VTN UI never holds OAuth secrets. The BFF (Backend For Frontend) holds two credential sets
and proxies all API calls.

```
Browser  →  VTN BFF (port 8220)  →  VTN API (port 8200)
```

### Why two credentials?

The VTN RBAC enforces role separation:

| Role | Credential | Can access |
|---|---|---|
| `any-business` | `business-client` | `GET/POST/PUT/DELETE /programs`, `/events`, `/reports` |
| `ven-manager` | `ven-client` | `GET/POST/PUT/DELETE /vens` |

A single credential cannot do both. The BFF uses `any-business` for operator operations
and `ven-manager` for VEN administration. The UI gets a unified API surface without knowing
about the split.

### Token management

The BFF holds two `VtnClient` instances, each with its own OAuth token. Tokens are refreshed
on 401. The UI communicates with the BFF using session-scoped API keys (not OAuth credentials).

### Report constraint

`POST /reports` requires the VEN role. Only VENs (not the BFF's `any-business` credential)
can create reports. The BFF proxies report submissions from the VEN's own API calls.

---

## 4. OpenADR Message Sequences

### 4.1 VEN Startup

```
VEN boots
  → load config (PROFILE_PATH, VTN_URL, client_id, client_secret)
  → POST /auth/token  →  access_token (30 days)
  → GET /programs     →  discover enrolled programs
  → GET /vens         →  confirm venName / venID
  → start polling loop (30 s interval)
```

### 4.2 Event Distribution

```
Operator creates event (VTN UI)
  → BFF proxies: POST /events  →  VTN stores, assigns eventID
  → (30 s later) VEN polls: GET /events
  → VEN evaluates intervals against local targets
  → VEN translates event type → internal signal (OadrEventSnapshot / CapacityState / alert)
  → PlanTrigger.RATE_CHANGE or CAPACITY_CHANGE emitted
  → Planner replans
  → (next report cycle) VEN: POST /reports
```

### 4.3 Event Update

```
Operator modifies event (VTN UI)
  → BFF proxies: PUT /events/{id}  →  VTN increments version
  → (30 s later) VEN polls: GET /events
  → VEN detects version change on known eventID
  → Re-processes event → PlanTrigger emitted
```

### 4.4 Event Cancellation

```
Operator cancels event (VTN UI)
  → BFF proxies: DELETE /events/{id}  →  VTN removes event
  → (30 s later) VEN polls: GET /events
  → VEN detects eventID absent from response
  → VEN rolls back DR response for that event
  → PlanTrigger.RATE_CHANGE or CAPACITY_CHANGE emitted
```

OpenADR 3 has **no cancel status field**. Cancellation is always a DELETE.

### 4.5 Token Lifecycle

```
POST /auth/token  →  access_token (TTL 30 days)
  → VEN stores token
  → On 401 Unauthorized  →  re-POST /auth/token
  → On VTN down  →  exponential backoff: 1 min → 2 min → 4 min → 8 min → max 15 min
```

### 4.6 Report Submission

```
OpenADR Interface reads OadrReportObligation (DueAt)
  → builds report payload from AssetState / PastEnergySum
  → VEN: POST /reports  →  VTN stores
  → OadrReportObligation marked Fulfilled
```

---

## 5. VEN Provisioning Sequence

VENs are provisioned via the VTN admin API. Four steps, three different roles:

```
Step 1 — Create user account (user-manager role)
  POST /users
  body: { "reference": "ven-1", "description": "VEN 1", "roles": [] }
  → returns { "id": "<user-uuid>" }

Step 2 — Add OAuth credential to user (user-manager role)
  POST /users/{user-uuid}/credentials
  body: { "client_id": "ven-1", "client_secret": "ven-1" }

Step 3 — Create VEN entity (ven-manager role)
  POST /vens
  body: { "venName": "ven-1" }
  → returns { "id": "<ven-uuid>" }

Step 4 — Assign VEN role to user (user-manager role)
  PUT /users/{user-uuid}
  body: {                             ← FULL body required (not a patch)
    "reference": "ven-1",
    "description": "VEN 1",
    "roles": [{ "role": "VEN", "id": "<ven-uuid>" }]
  }
```

**Important:** Step 4 is a full-replace PUT. The `roles` array must include all roles,
not just the new one. The VTN does not support PATCH on users.

**VEN identity model:**
- `ven_id` — stable UUID assigned at `POST /vens`
- OAuth `client_id` / `client_secret` — used for token acquisition
- `venName` — human-readable name, used in event `targets` filtering

**Target filtering:** Programs and events with `targets: [{ type: "VEN_NAME", values: ["ven-1"] }]`
are visible only to the named VEN(s). Programs/events with `targets: null` are open to all VENs.

---

## 6. Deployment Topology

### Host Port Mapping

| Container | Host Port | Role | Credentials |
|---|---|---|---|
| `vtn-vtn-1` | 8200 | openleadr-rs VTN | — |
| `vtn-db-1` | 8201 | PostgreSQL 16 | — |
| `ven-ven-1-1` | 8211 | VEN (ven-1) | ven-1 / ven-1 |
| `ven-ven-2-1` | 8212 | VEN (ven-2) | ven-2 / ven-2 |
| `ven-ven-3-1` | 8213 | VEN (ven-3) | ven-3 / ven-3 |
| `ven-ui-1` | 8214 | React VEN Web UI | — |
| `vtn-bff-1` | 8220 | Rust Axum BFF | dual-credential |
| `vtn-ui-1` | 8221 | React VTN UI (nginx) | — |

### Docker Network

- VTN uses Docker network `vtn_openadr-net` (named from compose project `vtn`)
- VEN compose references it as `external: true` — containers join the same network
- Container-to-container: use Docker DNS names (`vtn`, `ven-1`, etc.)
- Host access: use `Pi4-Server:<host-port>`

### Compose Projects

```
/srv/docker/openadr_lab/
  VTN/              → compose project name: vtn
    docker-compose.yml
  VEN/              → compose project name: ven
    docker-compose.yml
  tests/
    docker-compose.test.yml
    docker-compose.openleadr-test.yml
  openleadr-rs/     → git submodule
```

### Deploy Flow

```
local: git push
Pi4-Server: cd /srv/docker/openadr_lab && git pull
  → if VEN source changed: docker compose build ven-1 (or all)
  → docker compose up -d
```

First clone requires `git clone --recursive` (submodule).
Existing clones after pull: `git submodule update --init`.

### Docker Build Times (Pi4 ARM64)

| Image | First build | Cached rebuild |
|---|---|---|
| VTN (openleadr-rs + SQLx) | ~25 min | ~1–2 min |
| VEN (Rust) | ~11 min | ~1–2 min |
| VEN UI (npm + vite) | ~33 s | ~10 s |

---

## 7. Seeded Data

Seed script: `scripts/seed_vtn.py`

| Program | Enrolled VENs | Purpose |
|---|---|---|
| Summer Peak DR | ven-1, ven-2 | Residential peak demand reduction |
| EV Managed Charging | ven-2, ven-3 | Controlled EV charging during peak |
| HVAC Optimization | (open — all VENs) | Climate control DR |

**Note:** The VTN does not enforce unique event names. Re-running the seed script creates
duplicate events. The seed script is idempotent for programs but additive for events.

---

## 8. Design Decisions

### D-01: BFF Dual-Credential Pattern

**Decision:** BFF holds two OAuth credentials (`any-business` + `ven-manager`).
**Rationale:** VTN RBAC separates operator operations from VEN management. A single credential
cannot access both `/programs`+`/events` and `/vens`. The BFF provides a unified surface to
the browser without exposing secrets.

### D-02: VTN as Git Submodule

**Decision:** `openleadr-rs` is a git submodule pointing to the fork `TinkerPhu/openleadr-rs`.
**Rationale:** Allows local patches (e.g. PRs #372, #373, #374) to be applied and tested
before upstream merge without forking the main repo.

### D-03: Polling, Not Webhooks

**Decision:** VENs poll `GET /events` every 30 s; no webhooks.
**Rationale:** Lab deployment. Webhooks require VEN to be publicly addressable or on the same
network with known hostname. Polling is simpler and sufficient for 30 s event latency.

### D-04: PostgreSQL Persistence

**Decision:** VTN stores all entities in PostgreSQL 16.
**Rationale:** openleadr-rs requires PostgreSQL (SQLx migrations). Persistent Docker volume
ensures state survives container restarts. DB is not exposed to VENs — only VTN reads it.

### D-05: No DTO Normalisation

**Decision:** Field names pass through all layers unchanged: `programName`, `venName`, `eventName`, etc.
**Rationale:** One vocabulary across backend, BFF, and UI reduces boilerplate and debugging
friction. Any translation layer is a future source of bugs and cognitive overhead.

### D-06: VTN Controller Symmetry — shared abstractions staged in VEN/src/common/

**Observation:** A future VTN operator/aggregator controller (fleet flexibility aggregation,
M&V, event creation optimisation) would need the same foundational abstractions as the VEN
HEMS controller:
- `TimeSeries<T>` with typed interpolation (Step for tariffs, Linear for power)
- Interval arithmetic: overlap, union-of-breakpoints, time-weighted average
- `FlexibilityEnvelope` (VEN produces it; VTN aggregates it across fleet)
- Baseline model (VEN reports it; VTN uses it for M&V)

The planning algorithms themselves are **not** shared — VEN does single-site MILP scheduling (joint optimisation over a 24 h horizon); a VTN controller would do fleet-level dispatch optimisation across N VENs.

**Decision:** Shared abstractions are introduced as `VEN/src/common/` — a plain Rust module,
not a separate crate. When a VTN controller is built, `common/` is extracted into a shared
workspace crate at that point. No API changes are required at extraction time because the
module boundary is already clean.

See `VEN_ARCHITECTURE.md §1` (target source layout) and `docs/BACKLOG.md RF-05`.
