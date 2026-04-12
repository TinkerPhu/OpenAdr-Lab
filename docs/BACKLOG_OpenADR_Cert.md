# OpenADR 3.1 VEN Certification — Fulfilment Analysis

> **Date:** 2026-03-22
> **Scope:** VEN backend (`VEN/src/`) vs OpenADR 3.1.0 Definition & User Guide
> **Spec sources:** `docs/openadr_3_1_specs/2_OpenADR 3.1.0_Definition_20250801.md`, `docs/openadr_3_1_specs/3_OpenADR 3.1.0_User_Guide_20250801.md`, `docs/openadr_3_1_specs/0_READ ME_OpenADR 3 Information and Certification_v3.1.0.md`

---

## Legend

- **Full** — requirement fully implemented
- **Partial** — partly implemented or simplified
- **Missing** — not implemented
- **N/A** — not applicable to this lab/HEMS context

---

## 1. VEN Registration & Identity

| Requirement | Spec Level | Status | Notes |
|---|---|---|---|
| Configurable VTN URL | MUST | Full | `VTN_BASE_URL` env var |
| Configurable clientID / clientSecret | MUST | Full | `CLIENT_ID`, `CLIENT_SECRET` env vars |
| End-user reconfiguration support | MUST | Partial | Requires container restart (env-var based); no runtime UI for credential changes |
| mDNS discovery of local VTN | SHOULD | Missing | No mDNS implementation |

**Fulfilment: ~75%**

---

## 2. Communication & Protocol

| Requirement | Spec Level | Status | Notes |
|---|---|---|---|
| HTTPS / TLS 1.2+ for all communication | MUST | Missing | VTN client uses plain HTTP (`reqwest` with no TLS enforcement); Docker lab runs HTTP internally |
| TLS certificate verification (default on) | SHOULD | Missing | No TLS handling in `vtn.rs` |
| Configurable unverified-TLS setting | SHOULD | Missing | — |
| MQTT notification support | SHOULD | Missing | No MQTT client; purely polling-based |

**Fulfilment: 0%**

The entire transport is plain HTTP. For a production/certified VEN, HTTPS with TLS 1.2+ is mandatory. Acceptable for a lab environment but would be a certification blocker.

---

## 3. Authentication & Authorization

| Requirement | Spec Level | Status | Notes |
|---|---|---|---|
| OAuth 2.0 Client Credentials flow | MUST | Full | `vtn.rs` — POST `/auth/token` with client_id/secret, Bearer token on all requests |
| Token caching & auto-refresh | — | Full | RwLock cache with 60s refresh margin, 401 retry logic |
| Token endpoint discovery (`/auth/server`) | MUST | Missing | Hardcoded to `/auth/token`; does not query `/auth/server` for dynamic discovery |
| Non-authenticating VTN support (public tariffs) | SHOULD | Missing | Client always requires credentials; no unauthenticated mode |

**Fulfilment: ~75%**

---

## 4. Program Access

| Requirement | Spec Level | Status | Notes |
|---|---|---|---|
| Read programs (`GET /programs`) | read_targets | Full | Polls at configurable interval, stores in app state |
| Read individual program (`GET /programs/{id}`) | read_targets | Missing | Only bulk fetch implemented |
| Target-filtered program queries | MUST | Missing | No target query parameter sent; relies on VTN-side VEN_NAME filtering (upstream PRs #372-#374) |
| No create/update/delete programs | — | Full | Correctly read-only |

**Fulfilment: ~60%**

---

## 5. Event Handling

| Requirement | Spec Level | Status | Notes |
|---|---|---|---|
| Read events (`GET /events`) | read_targets | Full | Polls with `?active=true`, change detection |
| Read individual event (`GET /events/{id}`) | read_targets | Missing | Only bulk fetch |
| Target-filtered event queries | MUST | Missing | Relies on server-side filtering |
| PRICE payload parsing | — | Full | PRICE, EXPORT_PRICE, GHG extracted into TariffSnapshots |
| SIMPLE payload handling | — | Partial | Referenced in reporter but no dedicated signal handling path |
| CHARGE_STATE_SETPOINT | — | Missing | Not parsed |
| DISPATCH_SETPOINT / DISPATCH_INSTRUCTION | — | Partial | `dispatch_setpoints` field exists in capacity model but no dedicated actor/signal path |
| CONTROL_SETPOINT / CONTROL_LEVEL_OFFSET | — | Missing | Not implemented |
| Alert payloads (GRID_EMERGENCY, BLACK_START, OUTAGE, FLEX_ALERT, FIRE, FREEZING, WIND) | — | Partial | `alert_type` field in capacity model, `Alert` PlanTrigger enum — but no dedicated alert handling/UI |
| CURVE / OLS payloads | — | Missing | Not implemented |
| Capacity subscription/reservation payloads | — | Partial | `import_subscription_kw` / `import_reservation_kw` tracked but not dynamically negotiated |
| Opt-in / opt-out signaling | — | Missing | No opt-in/opt-out mechanism at all |
| Looping event support (`P9999Y` duration) | — | Full | Cyclic repetition implemented in `openadr_interface.rs` |
| Variable-duration intervals | — | Full | ISO 8601 duration parsing (PT1H, PT15M, PT30S) |

**Fulfilment: ~50%**

The VEN handles PRICE-family payloads well (its primary use case as a HEMS), but most non-price payload types (direct control, alerts, curves) are either stub-level or missing. This limits certification to pricing-oriented profiles only.

---

## 6. Report Creation & Submission

| Requirement | Spec Level | Status | Notes |
|---|---|---|---|
| Create reports (`POST /reports`) | MUST | Full | Upsert semantics with 409 Conflict -> PUT fallback |
| Update reports (`PUT /reports/{id}`) | MUST | Full | Used in upsert flow |
| Delete reports (`DELETE /reports/{id}`) | MUST | Missing | No delete operation in `vtn.rs` |
| Read own reports (`GET /reports`) | read_ven_objects | Full | Polls with `?clientName={ven_name}` |
| TELEMETRY_USAGE measurement reports | — | Full | Per-event, includes net power + OPERATING_STATE + SoC |
| Status reports (event-driven) | — | Full | Triggered on PacketTransition |
| Report obligation tracking | — | Full | Extracts from `reportDescriptors`, tracks due times, marks fulfilled |
| Data quality metadata (accuracy, confidence) | — | Missing | Not included in report payloads |
| Historical / forecast / rolling reports | — | Partial | Only real-time measurement reports; no historical replay or forecast reports |
| `resourceName` in report resources | — | Full | `"{ven_name}-meter"` naming pattern |

**Fulfilment: ~70%**

---

## 7. Subscriptions & Notifications

| Requirement | Spec Level | Status | Notes |
|---|---|---|---|
| Create subscriptions (`POST /subscriptions`) | write_subscriptions | Missing | No subscription management at all |
| Read / update / delete subscriptions | — | Missing | — |
| Webhook callback endpoint (HTTPS) | MUST | Missing | No webhook server; VEN is purely poll-based |
| Echo challenge verification | MUST | Missing | — |
| MQTT subscription support | SHOULD | Missing | — |

**Fulfilment: 0%**

This is one of the largest functional gaps. The VEN operates entirely on polling. Webhook subscriptions are a key OpenADR 3.1 capability for low-latency event notification. MQTT is SHOULD-level but increasingly expected.

---

## 8. VEN & Resource Management

| Requirement | Spec Level | Status | Notes |
|---|---|---|---|
| Create VEN object (`POST /vens`) | write_vens | Missing | VEN entity assumed pre-provisioned |
| Read own VEN (`GET /vens`) | read_ven_objects | Missing | No VEN self-read in `vtn.rs` |
| Update own VEN (`PUT /vens/{id}`) | write_vens | Missing | — |
| Delete own VEN (`DELETE /vens/{id}`) | write_vens | Missing | — |
| Create resources (`POST /resources`) | write_vens | Missing | No resource management |
| Read / update / delete resources | — | Missing | — |
| Resource -> report association | — | Partial | Reports use `resourceName` but no formal resource objects created on VTN |

**Fulfilment: 0%**

The VEN never self-registers or manages its own VEN/resource objects on the VTN. It assumes out-of-band provisioning (done by the seed script). A certified VEN would be expected to at least read its own VEN object and manage resources.

---

## 9. Targeting & Enrollment

| Requirement | Spec Level | Status | Notes |
|---|---|---|---|
| Target-based access control | MUST | Partial | Handled server-side via upstream PRs #372-#374 (VEN_NAME filtering); VEN does not send target parameters in queries |
| Program enrollment (target matching) | — | Partial | Seed data assigns VEN_NAME targets to programs; VEN does not actively manage enrollment |
| Target hiding (privacy) | — | Full | Handled server-side by fix/event-ven-target-privacy (PR #374) |

**Fulfilment: ~50%**

---

## 10. Data Model & Payload

| Requirement | Spec Level | Status | Notes |
|---|---|---|---|
| Object metadata (objectID, createdDateTime, etc.) | — | Full | Passthrough from VTN responses (DTO normalization avoidance) |
| Event payload descriptors | — | Full | Parsed for PRICE/EXPORT_PRICE/GHG + report obligations |
| Report payload descriptors | — | Full | payloadType, readingType, units included in reports |
| Interval management (id, period, duration) | — | Full | Full ISO 8601 duration parsing, interval IDs |
| `randomizeStart` support | — | Missing | Not implemented |
| Start time sentinel "0001-01-01" (meaning "now") | — | Missing | No special sentinel handling |

**Fulfilment: ~75%**

---

## 11. Response Handling & Error Management

| Requirement | Spec Level | Status | Notes |
|---|---|---|---|
| HTTP status code handling (200, 400, 403, 404, 500) | — | Partial | 401/403 triggers token refresh; 409 handled for report upsert; generic error logging for others |
| Problem response object parsing (RFC 7807) | MUST | Missing | No `ProblemDetails` struct or structured error parsing |
| Pagination (`skip` / `limit`) | SHOULD | Missing | No pagination parameters in any VTN query |

**Fulfilment: ~30%**

If the VTN returns 1000+ events or programs, the VEN will only get the first page (VTN default limit). No automatic pagination loop exists.

---

## 12. Compression

| Requirement | Spec Level | Status | Notes |
|---|---|---|---|
| gzip `Accept-Encoding` support | OPTIONAL | Missing | Not set; `reqwest` may handle transparent decompression if enabled in features |

**Fulfilment: 0% (optional requirement)**

---

## 13. Security & Privacy

| Requirement | Spec Level | Status | Notes |
|---|---|---|---|
| `write_reports` scope usage | — | Full | Reports submitted with VEN credentials |
| `read_targets` scope compliance | — | Partial | Relies on server-side enforcement |
| Object privacy (clientID isolation) | — | Full | VTN-side enforcement via openleadr-rs |
| HTTPS for all communication | MUST | Missing | Plain HTTP in lab setup |

**Fulfilment: ~40%**

---

## 14. Certification Profiles

| Profile | Status | Notes |
|---|---|---|
| **Continuous Pricing (CP)** — Price Receiving, GHG, Emergency Alert VEN | Partial | PRICE/EXPORT_PRICE/GHG parsing done, reporting done. Missing: subscriptions, TLS, pagination, problem parsing, full payload type coverage |
| **Baseline Profile (BP)** — General Flexibility System | Missing | Would require: DISPATCH_SETPOINT handling, CONTROL payloads, opt-in/out, resource management |

**Overall certification readiness: ~35%**

---

## Summary Table

| # | Spec Topic | Fulfilment | Key Gaps |
|---|---|---|---|
| 1 | VEN Registration & Identity | **~75%** | No mDNS, no runtime reconfiguration UI |
| 2 | Communication Protocol | **0%** | No TLS, no MQTT — lab-only HTTP |
| 3 | Authentication | **~75%** | No `/auth/server` discovery, no unauthenticated mode |
| 4 | Program Access | **~60%** | No individual fetch, no client-side target filtering |
| 5 | Event Handling | **~50%** | Strong on PRICE; weak on direct-control payloads, alerts, opt-in/out |
| 6 | Report Submission | **~70%** | No delete, no data quality metadata, no forecast reports |
| 7 | Subscriptions & Notifications | **0%** | Entirely missing — pure polling model |
| 8 | VEN & Resource Management | **0%** | No self-registration, no resource CRUD |
| 9 | Targeting & Enrollment | **~50%** | Server-side only; VEN is passive |
| 10 | Data Model & Payload | **~75%** | No `randomizeStart`, no "now" sentinel |
| 11 | Error Handling | **~30%** | No problem object parsing, no pagination |
| 12 | Compression | **0%** | Not implemented (optional) |
| 13 | Security & Privacy | **~40%** | No TLS; scope enforcement delegated to VTN |
| 14 | Certification Readiness | **~35%** | CP profile partially met; BP profile not met |

---

## Top Missing Areas (ordered by certification impact)

### 1. Subscriptions / Webhooks
The entire push notification layer is absent. The VEN can only poll. This is the biggest functional gap for a real-world deployment where low-latency event response matters.

**What's needed:**
- `POST /subscriptions` creation with callback URL
- HTTPS webhook listener endpoint in the VEN
- Echo challenge verification handler
- Subscription lifecycle management (read, update, delete)
- Optionally: MQTT client for topic-based notifications

### 2. TLS / HTTPS
A MUST requirement. Currently plain HTTP everywhere.

**What's needed:**
- Enable `reqwest` TLS features (rustls or native-tls)
- TLS certificate validation (default on)
- Configurable setting to allow unverified connections
- HTTPS listener for webhook callbacks

### 3. VEN & Resource Self-Management
The VEN never creates or manages its own VEN/resource objects on the VTN. It assumes out-of-band provisioning.

**What's needed:**
- `POST /vens` for self-registration on first boot
- `GET /vens` to read own VEN object (verify provisioning)
- `POST /resources` to register assets as resources
- `PUT /resources/{id}` to update resource state
- `DELETE /resources/{id}` for decommissioning

### 4. Non-Price Payload Types
Only PRICE/EXPORT_PRICE/GHG/capacity-limit payloads are parsed. Most direct-control and alert payload types are missing.

**What's needed:**
- CHARGE_STATE_SETPOINT parser + EV integration
- DISPATCH_SETPOINT / DISPATCH_INSTRUCTION -> reactor signal path
- CONTROL_SETPOINT / CONTROL_LEVEL_OFFSET -> asset setpoint mapping
- Alert payload handler (GRID_EMERGENCY, BLACK_START, OUTAGE, FLEX_ALERT, FIRE, FREEZING, WIND) with appropriate VEN response actions
- CURVE / OLS payload parsing

### 5. Opt-in / Opt-out
No mechanism for the VEN to signal acceptance or rejection of events or programs.

**What's needed:**
- Opt-in/opt-out decision logic per event
- Communication of opt status back to VTN (likely via report or subscription mechanism)
- User-facing control for manual opt decisions

### 6. Pagination
No `skip`/`limit` handling on any VTN query. Will silently lose data if collections exceed VTN page size.

**What's needed:**
- Pagination loop in `vtn.rs` for all GET collection endpoints
- Configurable page size
- Accumulate results across pages until exhausted

### 7. Problem Response Parsing (RFC 7807)
VTN error responses are logged as generic HTTP failures without structured parsing.

**What's needed:**
- `ProblemDetails` struct (`type`, `title`, `status`, `detail`, `instance`)
- Parse 4xx/5xx response bodies as RFC 7807 problem objects
- Structured error logging with problem fields
- Appropriate retry/backoff behavior per error type

### 8. Token Endpoint Discovery
Hardcoded `/auth/token` instead of querying `/auth/server` first.

**What's needed:**
- On startup, `GET /auth/server` to discover token endpoint URL
- Fall back to `/auth/token` if discovery fails
- Cache discovered endpoint

---

## What's Done Well

The VEN excels as a **HEMS-oriented price-responsive VEN**:

- **OAuth authentication** — full client credentials flow with auto-refresh and 401 retry
- **PRICE/tariff parsing** — including looping events (P9999Y), variable intervals, ISO 8601 durations
- **MILP energy planning** — joint 24 h horizon optimisation, battery arbitrage, EV semi-continuous scheduling, heater tier selection, configurable cost/GHG/comfort objectives
- **Report submission** — upsert semantics, obligation tracking, measurement + status reports
- **Simulation layer** — 5 physics-based asset models (EV, Battery, PV, Heater, BaseLoad) with 1s tick
- **Controller pipeline** — OpenADR interface -> planner -> dispatcher -> monitor -> reporter (full lifecycle)
- **Observability** — decision trace log, per-asset history buffers, Prometheus metrics, timeline API
- **User requests** — multi-tier deadlines, completion policies, energy packet lifecycle

For a lab/research environment focused on pricing use cases and HEMS optimization, the implementation is comprehensive and well-architected. The gaps are primarily around protocol-level requirements (TLS, subscriptions, self-registration) and non-price signal types that would be needed for formal OpenADR 3.1 certification.
