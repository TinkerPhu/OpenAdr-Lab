# OpenADR 3.1 VEN Certification — Fulfilment Analysis

> **Audited:** 2026-10-11, every row checked against the source on `main` (`670750a7`). The
> stack runs OpenADR 3.1 (the `openleadr-rs` fork tracks upstream's 3.1 line, see
> `docs/reference/FORK_PATCHES.md`).
> **Scope:** VEN backend (`VEN/src/`, `lab-core/src/`) vs OpenADR 3.1.0 Definition & User Guide
> **Spec sources:** `docs/openadr_3_1_specs/2_OpenADR 3.1.0_Definition_20250801.md`, `docs/openadr_3_1_specs/3_OpenADR 3.1.0_User_Guide_20250801.md`, `docs/openadr_3_1_specs/0_READ ME_OpenADR 3 Information and Certification_v3.1.0.md`
> **Lab position:** certification is not a goal today. The open transport work is planned, not
> scheduled, in `docs/plans/roadmap/phase-6-fidelity-and-cert.md`; what the lab deliberately does
> stricter than or differently from the spec is in `docs/reference/WIRE_PROFILE.md`.

---

## Legend

- **Full** — requirement fully implemented
- **Partial** — partly implemented or simplified
- **Missing** — not implemented

**Fulfilment** of a section is the mean over its rows with Full = 1, Partial = 0.5, Missing = 0,
rounded to 5 %. It counts rows, not their weight: one missing MUST blocks certification whatever
the percentage says.

---

## 1. VEN Registration & Identity

| Requirement | Spec Level | Status | Notes |
|---|---|---|---|
| Configurable VTN URL | MUST | Full | `VTN_BASE_URL` env var (`config.rs`) |
| Configurable clientID / clientSecret | MUST | Full | `CLIENT_ID`, `CLIENT_SECRET` env vars |
| End-user reconfiguration support | MUST | Partial | Requires a container restart (env vars); no runtime UI for credential changes |
| mDNS discovery of local VTN | SHOULD | Missing | No mDNS implementation |

**Fulfilment: ~65%**

---

## 2. Communication & Protocol

| Requirement | Spec Level | Status | Notes |
|---|---|---|---|
| HTTPS / TLS 1.2+ for all communication | MUST | Missing | The lab runs plain HTTP between VEN and VTN. `reqwest` is built with `rustls-tls`, so an `https://` `VTN_BASE_URL` would connect, but no deployment or test uses one |
| TLS certificate verification (default on) | SHOULD | Partial | On by library default whenever `https://` is used; never exercised |
| Configurable unverified-TLS setting | SHOULD | Missing | No setting; a self-signed VTN certificate could not be accepted |
| MQTT notification support | SHOULD | Missing | The VEN has MQTT clients (`rumqttc`) for the lab's own feeds only: weather, measurement, fleet telemetry. It does not read `GET /notifiers` or subscribe to OpenADR notifications |

**Fulfilment: ~15%**

Certification blocker. Planned as WP6.5 (TLS at a reverse proxy in front of the VTN).

---

## 3. Authentication & Authorization

| Requirement | Spec Level | Status | Notes |
|---|---|---|---|
| OAuth 2.0 Client Credentials flow | MUST | Full | `vtn.rs` — POST `/auth/token`, Bearer token on all requests |
| Token caching & auto-refresh | — | Full | Cached with a refresh margin; a 401/403 refetches the token and retries once |
| Token endpoint discovery (`/auth/server`) | MUST | Missing | Hardcoded to `/auth/token` |
| Non-authenticating VTN support (public tariffs) | SHOULD | Missing | The client always requires credentials |

**Fulfilment: ~50%**

Beyond the spec: the VEN reads the scopes out of its own token and reports a missing one
(`read_targets`, `read_ven_objects`, `write_reports_ven`) through its wire-conformance health
(`controller/token_scopes.rs`, `state/wire_health.rs`).

---

## 4. Program Access

| Requirement | Spec Level | Status | Notes |
|---|---|---|---|
| Read programs (`GET /programs`) | read_targets | Full | Polled at a per-profile interval, paginated |
| Read individual program (`GET /programs/{id}`) | read_targets | Missing | Only the bulk fetch |
| Target-filtered program queries | MUST | Partial | The VEN sends no `targets` query parameter. The 3.1 VTN filters and redacts by the caller's own targets on every request, so the VEN sees only what addresses it |
| No create/update/delete programs | — | Full | Correctly read-only |

**Fulfilment: ~65%**

---

## 5. Event Handling

| Requirement | Spec Level | Status | Notes |
|---|---|---|---|
| Read events (`GET /events`) | read_targets | Full | Polls `?active=true`, paginated, change detection |
| Read individual event (`GET /events/{id}`) | read_targets | Missing | Only the bulk fetch |
| Target-filtered event queries | MUST | Partial | As for programs: server-side by the caller's targets, no query parameter |
| PRICE / EXPORT_PRICE / GHG | — | Full | Parsed into tariff and CO₂ series the planner uses |
| IMPORT / EXPORT_CAPACITY_LIMIT | — | Full | Time-aware: the planner caps each slot with the limit in force then (`entities::capacity::tightest_capacity_limit`); the arbiter enforces it at execution |
| SIMPLE | — | Full | Levels 0–3 clamp the per-slot import cap |
| CHARGE_STATE_SETPOINT | — | Partial | Received and deliberately **not applied**: a grid operator's SoC command is not the driver's plan. Each event produces one Info notification (`WIRE_PROFILE.md`, "State-of-charge setpoints are received, not applied") |
| DISPATCH_SETPOINT / DISPATCH_INSTRUCTION | — | Partial | DISPATCH_SETPOINT steers net site power during its window (an alert takes precedence). DISPATCH_INSTRUCTION is not handled |
| CONTROL_SETPOINT / CONTROL_LEVEL_OFFSET | — | Missing | Not handled |
| Alert payloads (GRID_EMERGENCY, BLACK_START, OUTAGE, FLEX_ALERT, FIRE, FREEZING, WIND) | — | Partial | GRID_EMERGENCY and BLACK_START clamp planned import to 0 over their window; the other five are not handled |
| CURVE / OLS payloads | — | Missing | Not handled |
| Capacity subscription / reservation | — | Partial | Import and export subscription/reservation bind the solver as a contracted allowance, and the VEN reports the capacity it wants (`IMPORT_RESERVATION_CAPACITY`, `EXPORT_RESERVATION_CAPACITY`). Not time-aware per interval (GB-48 C) and no fee (GB-56) |
| Opt-in / opt-out signalling | — | Missing | No mechanism |
| Looping events (`P9999Y` duration) | — | Full | `lab_core::event_timing` |
| Variable-duration intervals | — | Full | ISO 8601 durations; one interval-timing rule for every parser (`lab_core::event_timing`) |

**Fulfilment: ~55%**

Every event value is read through its declared unit (`lab_core::wire_contract::PayloadReader`):
the event's `payloadDescriptors`, else the program's, else the profile default, which is then
shown as a wire assumption. A payload declaring a different unit is refused, not converted.
A payload type the profile does not list is passed through and nothing acts on it.

---

## 6. Report Creation & Submission

| Requirement | Spec Level | Status | Notes |
|---|---|---|---|
| Create reports (`POST /reports`) | MUST | Full | Upsert: on 409 Conflict the report is found by name and updated with PUT |
| Update reports (`PUT /reports/{id}`) | MUST | Full | Used by the upsert and by the VEN's own `PUT /reports/{id}` route |
| Delete reports (`DELETE /reports/{id}`) | MUST | Missing | No delete operation in `vtn.rs` |
| Read own reports (`GET /reports`) | read_ven_objects | Full | Polls `?clientName={ven_name}` |
| Measurement reports | — | Full | Per active event; site power or energy plus `OPERATING_STATE` and storage state of charge |
| Status reports (event-driven) | — | Missing | Plan cycles are logged (`/trace/events`) and streamed (`/plan/events`), but no report is built from them |
| Report obligation tracking | — | Full | Extracted from `reportDescriptors`; each due obligation is re-armed to its next due time and retired when its event leaves the active set |
| Report payload unit semantics | — | Full | `USAGE`, `USAGE_FORECAST`, `BASELINE`, `DELTA_USAGE` are energy over the interval in `KWH`; `DEMAND` is signed real power in `KW`. Every report payload carries its descriptor; a payload cannot be built without one (`controller/report_payload.rs`) |
| `reportDescriptor` fields honoured | — | Partial | `payloadType`, `readingType`, `historical`, `reportIntervals`, `frequency` (as "intervals between reports") and `numIntervals` are read (`openadr_interface.rs::report_timing`). `startInterval`, `repeat` and `aggregate` are ignored |
| `BASELINE` reports | — | Full | The heuristic forecast as if no event ran, so an event's impact can be measured against `USAGE` |
| Data quality metadata | — | Partial | `DATA_QUALITY` tags provenance (`HEURISTIC`) on baseline intervals. No accuracy or confidence figure |
| Historical / forecast / rolling reports | — | Partial | `historical: false` serves plan-slot forecasts; `historical: true` serves measured history. Rolling reports are not built |
| `resourceName` in report resources | — | Full | `"{ven_name}-meter"` |

**Fulfilment: ~75%**

3.1 makes `eventID` a report's only object link; every report from this lab carries one and
there is no program-level reporting.

---

## 7. Subscriptions & Notifications

| Requirement | Spec Level | Status | Notes |
|---|---|---|---|
| Create subscriptions (`POST /subscriptions`) | write_subscriptions | Missing | No subscription management |
| Read / update / delete subscriptions | — | Missing | — |
| Webhook callback endpoint (HTTPS) | MUST | Missing | No callback route; the VEN only polls |
| Echo challenge verification | MUST | Missing | — |
| MQTT subscription support | SHOULD | Missing | See §2 |

**Fulfilment: 0%**

Certification blocker. The 3.1 VTN offers the subscription API; the VEN side is planned as WP6.6.
The VTN's own MQTT notifier binding does not conform yet either
(`docs/plans/mqtt_notifier_conformance.md`).

---

## 8. VEN & Resource Management

| Requirement | Spec Level | Status | Notes |
|---|---|---|---|
| Create VEN object (`POST /vens`) | write_vens | Missing | Provisioned by `scripts/seed_vtn.py` |
| Read own VEN (`GET /vens`) | read_ven_objects | Missing | No VEN self-read in `vtn.rs` |
| Update own VEN (`PUT /vens/{id}`) | write_vens | Missing | — |
| Delete own VEN (`DELETE /vens/{id}`) | write_vens | Missing | — |
| Create resources (`POST /resources`) | write_vens | Missing | No resource management |
| Read / update / delete resources | — | Missing | — |
| Resource → report association | — | Partial | Reports name a `resourceName`, but no resource object exists on the VTN |

**Fulfilment: ~5%**

A decision, not an obstacle (GB-51, closed 2026-10-10): the seed script provisions every VEN,
its user, its scopes and its credential before the VEN starts. Under 3.1 the VTN gates these
routes by scope (`write_vens` for a VEN's own object), so self-registration is possible from the
VEN side; the lab does not grant that scope and has no rule for which side would win.

---

## 9. Targeting & Enrollment

| Requirement | Spec Level | Status | Notes |
|---|---|---|---|
| Target-based access control | MUST | Partial | Enforced by the VTN against the VEN's targets. The lab provisions those to equal the `venName`, a lab convention, not a protocol rule (`WIRE_PROFILE.md`, "Targets address VENs by name") |
| Program enrollment (target matching) | — | Partial | Seed data assigns the targets; the VEN does not manage its own enrollment |
| Target hiding (privacy) | — | Full | Native in the 3.1 VTN: an object shows a VEN only the targets it asked with |

**Fulfilment: ~65%**

---

## 10. Data Model & Payload

| Requirement | Spec Level | Status | Notes |
|---|---|---|---|
| Object metadata (objectID, createdDateTime, etc.) | — | Full | Passed through from VTN responses under the spec's own names |
| Event payload descriptors | — | Full | Read for every value the VEN acts on (see §5) |
| Report payload descriptors | — | Full | Always emitted (see §6) |
| Interval management (id, period, duration) | — | Full | `lab_core::event_timing` |
| `randomizeStart` support | — | Full | Per-VEN stable offset within the declared window |
| Start time sentinel "0001-01-01" (meaning "now") | — | Full | `lab_core::event_timing` |

**Fulfilment: 100%**

One deviation on the other side of the wire: the stack requires `intervalPeriod.start`, which the
3.1 schema leaves optional. The VTN rejects such an event before any VEN sees it (R-87).

---

## 11. Response Handling & Error Management

| Requirement | Spec Level | Status | Notes |
|---|---|---|---|
| HTTP status code handling (200, 400, 403, 404, 500) | — | Partial | 401/403 refetch the token; 409 drives the report upsert; any other non-2xx becomes `DomainError::VtnRejected` carrying the status, with no behaviour of its own per code |
| Problem response object parsing (RFC 7807) | MUST | Full | `vtn.rs::http_error` parses `type`/`title`/`status`/`detail`/`instance`, falling back to the raw body |
| Pagination (`skip` / `limit`) | SHOULD | Full | `get_json_paginated` pages every collection GET |

**Fulfilment: ~85%**

---

## 12. Compression

| Requirement | Spec Level | Status | Notes |
|---|---|---|---|
| gzip `Accept-Encoding` support | OPTIONAL | Missing | The `reqwest` `gzip` feature is not enabled |

**Fulfilment: 0% (optional requirement)**

---

## 13. Security & Privacy

| Requirement | Spec Level | Status | Notes |
|---|---|---|---|
| `write_reports` scope usage | — | Full | Reports are submitted with the VEN's own credential (`write_reports_ven`) |
| `read_targets` scope compliance | — | Partial | Enforced by the VTN; the VEN checks only that it was granted the scope |
| Object privacy (clientID isolation) | — | Full | Enforced by the VTN |
| HTTPS for all communication | MUST | Missing | See §2 |

**Fulfilment: ~65%**

---

## 14. Certification Profiles

| Profile | Status | Notes |
|---|---|---|
| **Continuous Pricing (CP)** — Price Receiving, GHG, Emergency Alert VEN | Partial | Prices, GHG, the two grid alerts, reporting, pagination and problem parsing are done. Missing: TLS, `/auth/server`, subscriptions |
| **Baseline Profile (BP)** — General Flexibility System | Partial | DISPATCH_SETPOINT, capacity limits and baseline reports are done. Missing: CONTROL payloads, opt-in/out, resource management, and everything CP misses |

**Overall: the sections average about 50 %, and four MUSTs are open** (TLS, `/auth/server`
discovery, the webhook callback with echo challenge, `DELETE /reports/{id}`). The VEN is not
certifiable until those are closed.

---

## Summary Table

| # | Spec Topic | Fulfilment | Key Gaps |
|---|---|---|---|
| 1 | VEN Registration & Identity | **~65%** | No mDNS, no runtime reconfiguration |
| 2 | Communication Protocol | **~15%** | Plain HTTP in the lab, no OpenADR MQTT notifications |
| 3 | Authentication | **~50%** | No `/auth/server` discovery, no unauthenticated mode |
| 4 | Program Access | **~65%** | No individual fetch, no `targets` query parameter |
| 5 | Event Handling | **~55%** | CONTROL and CURVE/OLS payloads, five alert types, opt-in/out, DISPATCH_INSTRUCTION |
| 6 | Report Submission | **~75%** | No delete, no status reports, three descriptor fields ignored |
| 7 | Subscriptions & Notifications | **0%** | Entirely missing — pure polling |
| 8 | VEN & Resource Management | **~5%** | Provisioned by the seed script, by decision |
| 9 | Targeting & Enrollment | **~65%** | Server-side only; the VEN is passive |
| 10 | Data Model & Payload | **100%** | — |
| 11 | Error Handling | **~85%** | No behaviour per status code beyond 401/403/409 |
| 12 | Compression | **0%** | Not implemented (optional) |
| 13 | Security & Privacy | **~65%** | No TLS |

---

## Open gaps, ordered by certification impact

1. **TLS / HTTPS** (MUST, §2). An unverified-TLS setting for the lab's self-signed certificate,
   and a deployment that actually uses `https://`. Planned: WP6.5.
2. **Subscriptions / webhooks** (MUST, §7). `POST /subscriptions`, a callback route, the echo
   challenge, subscription lifecycle, polling kept as fallback. Planned: WP6.6.
3. **Token endpoint discovery** (MUST, §3). `GET /auth/server`, fall back to `/auth/token`,
   cache the result. Planned: WP6.5.
4. **`DELETE /reports/{id}`** (MUST, §6). Small; not planned anywhere yet.
5. **VEN and resource self-management** (§8). Closed by decision for the lab (GB-51).
6. **Payload families not handled** (§5): CONTROL_SETPOINT / CONTROL_LEVEL_OFFSET,
   DISPATCH_INSTRUCTION, CURVE / OLS, and the alerts OUTAGE, FLEX_ALERT, FIRE, FREEZING, WIND.
7. **Opt-in / opt-out** (§5). De-prioritised until fleet participation choice is an experiment
   variable (`docs/plans/strategic_roadmap.md` §5).
8. **Status reports, rolling reports, `startInterval` / `repeat` / `aggregate`** (§6).
9. **mDNS, gzip, individual `GET` by id, a non-authenticating mode** (§1, §12, §4, §5, §3): the
   SHOULD and OPTIONAL remainder.
