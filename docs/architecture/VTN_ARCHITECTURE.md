# VTN Architecture

**Authoritative reference for VTN, BFF, OpenADR message sequences, provisioning, and deployment.**
Domain vocabulary is in [docs/REQUIREMENTS.md](../REQUIREMENTS.md).
VEN architecture is in [docs/architecture/VEN_ARCHITECTURE.md](VEN_ARCHITECTURE.md).

---

## 1. Component Overview

```
┌────────────────────────────────────────────────────────────────────┐
│                         Node1 (Docker)                        │
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
- Fixture user: **`bl-client`** only (`VTN/fixtures/01_bl_client.sql`). Nothing can call
  `/users` without a token, so exactly one bootstrap credential lives in SQL; every VEN user
  is created through the API, so the VTN hashes each secret and no password hash is committed.
- `POST /auth/token` and the whole `/users` tree exist only when the VTN is built with the
  `internal-oauth` feature, which is **not** an upstream default — see `vtn.Dockerfile`.

**Field names (pass-through, no DTO normalisation):**
`programName`, `programID`, `createdDateTime`, `venName`, `eventName` — upstream spec names used at all layers.

---

## 3. BFF — Single Business Credential

The VTN UI never holds OAuth secrets. The BFF (Backend For Frontend) holds one credential and
proxies all API calls.

```
Browser  →  VTN BFF (port 8220)  →  VTN API (port 8200)
```

### One credential, not two

OpenADR 3.1 replaced the role model with **scopes carried on the user object**, so a single
client can hold everything the BFF needs. The 3.0 split (`any-business` for operator work,
`ven-manager` for VEN administration, because no single role could do both) is gone.

`bl-client` holds:

```
read_all  write_programs  write_events  write_vens_bl  write_reports_bl  write_users
```

**Scopes are written in full; the aliases are never used.** 3.1 splits three of them by actor —
`write_vens_bl` / `write_vens_ven`, and the same for reports and subscriptions — and the bare
`write_vens`, `write_reports` and `write_subscriptions` are aliases for the **VEN** variant.
The distinction is not privilege level but code path: with `write_vens_bl` the VTN takes
`clientID` from the request body, while with `write_vens_ven` it overwrites it with the caller's
own token subject. A BFF granted the alias would therefore create every VEN object owned by
itself and collide on `ven_client_id_unique` at the second one.

`read_all` and `read_targets` are mutually exclusive branches in every read handler: a `read_all`
holder bypasses target filtering entirely, which is correct for the BFF and wrong for a VEN.

### Token management

The BFF holds one `VtnClient` with its own OAuth token, refreshed on 401. The UI communicates
with the BFF using session-scoped API keys (not OAuth credentials).

### Reports

VENs submit their own reports with `write_reports_ven`; the BFF holds `write_reports_bl`, which
is what lets it read and delete reports across the fleet. A report carries `eventID` as its only
object link — 3.1 removed `programID` — and `clientID`, which the VTN fills in from the token, so
a report always identifies the VEN that submitted it.

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
  → builds report payload from sim snapshot (asset power, SoC) via reporter.rs
  → VEN: POST /reports  →  VTN stores
  → OadrReportObligation marked Fulfilled
```

---

## 5. VEN Provisioning Sequence

Three steps, one credential (`bl-client`). OpenADR 3.1 carries a user's scopes on the user
object and sets them in the creation call, so the 3.0 fourth step — attaching a VEN role after
the credential already existed — is gone.

```
Step 1 — Create the user WITH its scopes
  POST /users
  body: { "reference": "ven-1-user", "description": "VEN ven-1",
          "scope": ["read_targets", "read_ven_objects", "write_reports_ven"] }
  → returns { "id": "<user-uuid>" }

Step 2 — Add the OAuth credential to that already-scoped user
  POST /users/{user-uuid}
  body: { "client_id": "ven-1", "client_secret": "ven-1" }

Step 3 — Create the VEN object
  POST /vens
  body: { "objectType": "BL_VEN_REQUEST",     ← mandatory discriminator
          "venName": "ven-1",
          "clientID": "ven-1",
          "targets": ["ven-1"] }              ← without this it sees nothing targeted
  → returns { "id": "<ven-uuid>" }
```

**`objectType` is mandatory.** 3.1 models `VenRequest` as a tagged enum
(`BL_VEN_REQUEST` / `VEN_VEN_REQUEST`); a body without the tag is rejected with
*"missing field `objectType`"*. It also selects behaviour: with `write_vens_bl` the VTN takes
`clientID` from the body, whereas the `_ven` variant derives it from the caller's token.

**The ordering hazard is gone.** Under 3.0 the role was attached only after the credential
existed, so a VEN polling for its token could mint one carrying no role and cache it for the
token's whole lifetime — authorized to nothing, polling successfully, seeing an empty world
(GB-49). A 3.1 user has its scopes from the moment it exists, so that window cannot occur and
credentials no longer have to be created last.

**VEN identity and visibility:**
- `clientID` — identifies *which VEN object* a caller is. Unique (`ven_client_id_unique`).
- `targets` — decides *what that VEN can see*. **This is the address, not the clientID.**
  The VTN intersects an object's `targets` with the union of the VEN's own `targets` and its
  resources'; a VEN provisioned with an empty list sees only untargeted objects, however
  precisely a program names its clientID (spec 3.1.1 Definition.md, "VEN created object
  privacy"). The lab gives each VEN its own `venName` as a target, which is a convention, not a
  protocol rule.
- `venName` — human-readable name; also the target string by the convention above.
- `ven_id` — stable UUID assigned at `POST /vens`.

**Target hiding** is native in 3.1: a VEN sees only its own id in an object's `targets`, never a
sibling's, while a `read_all` holder sees the full list. An empty `targets` means visible to
every VEN.

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
- Host access: use `Node1:<host-port>`

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
Node1: cd /srv/docker/openadr_lab && git pull
  → if VEN source changed: docker compose build ven-1 (or all)
  → docker compose up -d
```

First clone requires `git clone --recursive` (submodule).
Existing clones after pull: `git submodule update --init`.

### Docker Build Times (Node1 ARM64)

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

### D-01: BFF Single Business Credential

**Decision:** the BFF holds one OAuth credential, `bl-client`, with its scopes named in full.
**Rationale:** 3.1 carries scopes on the user object, so one client can hold both operator and
VEN-administration rights; the 3.0 dual-credential pattern existed only because RBAC roles could
not be combined. The scopes are spelled out rather than abbreviated because `write_vens`,
`write_reports` and `write_subscriptions` are aliases for the *VEN* variants, which select a
different code path (`clientID` taken from the token instead of the body) rather than a lower
privilege level.

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

The planning algorithms themselves are **not** shared — VEN does single-site greedy
scheduling; a VTN controller would do fleet-level dispatch optimisation across N VENs.

**Decision:** Shared abstractions are introduced as `VEN/src/common/` — a plain Rust module,
not a separate crate. When a VTN controller is built, `common/` is extracted into a shared
workspace crate at that point. No API changes are required at extraction time because the
module boundary is already clean.

See `VEN_ARCHITECTURE.md §1` (target source layout) and `docs/BACKLOG.md RF-05`.
