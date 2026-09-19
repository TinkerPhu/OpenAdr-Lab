# Fleet Monitor — Phase 0: Foundation Concept

Concept for reaching the first three fleet-monitor views from [vision.md](vision.md):
**§1 Signal timeline**, **§2 Fleet power chart** and **§5 Reaction tracing**, in the VTN UI.

Phase 0 builds only the **environment and services**: programs, report requests, VEN report
fixes, the MQTT telemetry side channel, BFF ingest/storage/stream, and seeding. It builds
**no views**. Phase 0 is done when every data series the three views need can be fetched from a
BFF endpoint and each one has at least a raw diagnostic surface (see "UI surface in phase 0"
below — required by `ui-transparency`).

Status: concept — not yet broken into tasks; the decisions taken so far are recorded in §9.

---

## 1. What the three views need

| View | Data needed | Freshness | Source |
|---|---|---|---|
| §1 Signal timeline | all events with intervals, payload types, targets, program, version (`modificationDateTime`) | ≤ 10 s after create/edit | VTN objects via BFF |
| §1 (resolved) | per VEN: which signal applies now/at t (as the VEN understood it) | seconds | MQTT `signals` (live); none in reports |
| §2 Fleet power | per-VEN net grid power, signed (import +, export −), time-aligned so it can be summed | live: ≤ 5 s; report: per report interval | MQTT `telemetry` (live) + `DEMAND` report (report) |
| §2 overlay | active import/export limits, dispatch setpoints per VEN and fleet | as §1 | VTN events (resolved by BFF) |
| §5 Reaction tracing | event created/modified at VTN; VEN saw it; VEN replanned because of it; power changed; report delivered | timestamps with ≤ 1 s resolution | VTN event timestamps, MQTT `trace`, telemetry, report `createdDateTime`/`modificationDateTime` |

---

## 2. Inventory — where these concepts live today

Required by the one-concept-one-function rule: every phase 0 work item reuses or consolidates
these, never adds a parallel copy.

| Concept | Lives in | Phase 0 use |
|---|---|---|
| Net site power from asset history | `controller/report_intervals.rs::build_net_site_power_ts` (obligation path) **and** `tasks/sim_tick/publish.rs` via `sim_snap.grid.net_power_w` (timer path) | two derivations — consolidate (F-9) |
| Measurement report assembly | `controller/reporter.rs::build_measurement_report` (timer) **and** `build_measurement_report_for_obligation` (obligation) | two builders — consolidate (F-9) |
| Report obligation extraction from `reportDescriptors` | `controller/openadr_interface.rs::extract_report_obligations` | fix descriptor semantics (F-4) |
| Report submission / upsert | `vtn.rs::upsert_report` (POST → 409 → find by name → PUT) | cache report id (F-6) |
| Uniform resampling on a wall-clock grid | `common/mod.rs::TimeSeries::resample_uniform` | BFF fleet aggregation — share, don't copy |
| "Does interval i run at t" | `controller/event_timing` + `entities/time_window::TimeWindow` | BFF signal resolution — share, don't copy |
| "Which limit applies at t" | `tightest_capacity_limit`, `tariff_at` | BFF overlay — share, don't copy |
| Controller decision log | `controller/trace.rs::ControllerEvent` + `/trace/events` | publish over MQTT; add correlation ids (§6.3) |
| Signals the VEN believes apply | `GET /signals` (VEN) | publish over MQTT |
| MQTT client + topic root convention | `weather.rs`, `measurement.rs` (rumqttc; `<root>` default `openadr-lab`) | reuse connection setup for publishing |
| Paginated VTN list fetch | VEN `vtn.rs::get_json_paginated`; BFF `recorder.rs::fetch_all_pages`; BFF list routes **don't paginate** | consolidate in BFF (F-1) |
| Report/event archive | BFF `recorder.rs` → Postgres schema `lab_recorder` | extend for telemetry |
| SSE push to a UI | VEN `services/notify.rs` (notification feed) | same pattern for BFF `/api/fleet/stream` |
| Report consumers | `experiments/kpi.py` (`USAGE` as energy per interval), `tests/features/steps/reporter_resampling_steps.py`, `sim_steps.py` | must move with any report-type change (F-2) |

**D-06 trigger.** `docs/architecture/VTN_ARCHITECTURE.md` D-06 says `VEN/src/common/` becomes a
shared workspace crate "when a VTN controller is built". The BFF's fleet aggregation is that
moment: it needs `TimeSeries`, the interval-timing rule and the limit lookup. `common/mod.rs`
depends only on `chrono`, so extraction is mechanical; `event_timing` and `TimeWindow` must move
with it (check their imports first).

---

## 3. Report engine analysis

Analysed against the code and the live VTN on Node1 (20 VENs: ven-1..3 on Node1, ven-4..20 on
Node2; one active experiment event `exp-simple` requesting `USAGE` + `BASELINE` at
`frequency: 300`).

### How it works today

Two independent paths submit reports:

1. **Timer path** (`tasks/sim_tick/publish.rs::run_measurement_reports`, every
   `report_interval_s` = 60 s): for every *active* event **without** `reportDescriptors`, one
   single-interval report (net import or export W, `OPERATING_STATE`, EV SoC).
2. **Obligation path** (`tasks/obligation.rs`, 5 s check loop): for every event **with**
   `reportDescriptors`, one obligation per `(event, payloadType)`. When due, it resamples the
   last hour of asset history onto `frequency`-second buckets and upserts the whole window as
   one report named `ob-<ven>-<event>-<payloadType>`.

Live footprint: 40 report objects (20 VENs × 2 payload types), ~3 KB each, 12 intervals each;
a full `GET /reports` is ~120 KB.

### What is good and should stay

- Buckets are aligned to the wall-clock epoch grid (`floor_to_grid`), so buckets from different
  VENs line up and can be summed without re-alignment.
- Stable `reportName` per (VEN, event, payload type); 404 drops the obligation (GB-23).
- `OPERATING_STATE` is derived from sample freshness, not hard-coded.
- The recorder dedups on `(id, modificationDateTime)` and measures submission lag over newly
  appended intervals only (GB-36).

### Findings

| # | Finding | Evidence | Impact on the fleet monitor |
|---|---|---|---|
| **F-1** | BFF list routes (`/api/reports`, `/api/events`, `/api/programs`, `/api/vens`) fetch a single page; openleadr-rs caps pages at 50. The recorder has its own pagination loop (`fetch_all_pages`), the routes don't. | `VTN/bff/src/routes/*.rs` call `get_json` once; `openleadr-vtn/src/api/report.rs` `limit ≤ 50` | **Silent truncation**: live count is 40; one more payload type or a second event with descriptors → 60 → 10 reports disappear from the UI. Also a duplicated concept. |
| **F-2** | Reports use `USAGE` for mean **power in W**. OpenADR 3.1 defines `USAGE` as *energy over an interval* and `DEMAND` as *real power* (with `readingType` MEAN/PEAK). No program declares `payloadDescriptors`, so units are implicit. | `reporter.rs` `value_kw * 1000.0` under `USAGE`; spec Definition §Report payload types | Fleet chart can't trust units from the report alone; a non-lab VTN would misread it. |
| **F-3** | Measured power is clamped to ≥ 0 (`value_kw.max(0.0)`) for `USAGE`/`PRICE`/`SIMPLE`; export shows as 0. | `reporter.rs` obligation fallback arm; ven-1's midday intervals read `0.0` | **Fleet sum is wrong** whenever a PV/battery site exports — exactly the interesting case. |
| **F-4** | `reportDescriptor.frequency` is interpreted as **seconds** (default 3600). The spec defines it as the *number of intervals between reports*, with interval length coming from the event's `intervalPeriod`. `startInterval`, `numIntervals`, `repeat` and `aggregate` are ignored. | `openadr_interface.rs::extract_report_obligations`; spec Definition §reportDescriptor | Report cadence is a lab convention, not spec-portable; `numIntervals` can't limit the window (see F-5). |
| **F-5** | Every submission re-sends the **full last hour** (12 buckets at 300 s) and replaces the report object. 11 of 12 intervals are repeats; the newest bucket is still in progress when sent. The recorder stores each full snapshot. | `tasks/obligation.rs` slices `Duration::seconds(3600)`; live reports have 12 intervals | VTN keeps only the last hour; archive holds ~12× redundant data; values of the newest bucket change afterwards. |
| **F-6** | Steady-state upsert costs 3 requests: POST → 409 → GET own reports (paginated) → PUT. | `vtn.rs::upsert_report`, `find_report_by_name` | ~120 requests per 5 min at 20 VENs; grows with report count. Cheap fix: remember the report id after first create. |
| **F-7** | The 5 s obligation loop locks the simulator and copies one hour of history for every asset **before** checking whether anything is due. | `tasks/obligation.rs` | Needless lock contention on every VEN, 12 times a minute. |
| **F-8** | Reports exist only while an event with `reportDescriptors` is active. With no such event, the VTN receives no telemetry. | obligation extraction runs only over events | An idle fleet is invisible in the report view — needs a standing monitoring program (§4). |
| **F-9** | Two report builders and two net-power derivations (timer vs obligation path). The timer path sends intervals without `intervalPeriod` (no timestamp), a `STORAGE_CHARGE_LEVEL` as a string, and `SIMPLE` reports with a constant `1.0`. | `reporter.rs`, `tasks/sim_tick/publish.rs` | Duplicated concept with diverging behaviour; timer-path reports can't be placed on a time axis. |
| **F-10** | End-to-end report latency is up to ~6 min: report frequency (300 s) + recorder/BFF poll (30 s) + BFF cache + UI refetch (10 s). openleadr-rs has no subscriptions/webhooks and no "modified since" filter, so the BFF must re-read full lists. | recorder `RECORDER_POLL_SECS` default 30; `VTN/ui/src/api/hooks.ts` refetch 10 s | Reports alone can't drive "watch it live" — this is why MQTT runs in parallel. |

F-1 and F-9 are duplicated concepts and go into `docs/reference/TECHNICAL_DEBTS.md` (R-84,
R-85). F-2..F-8 are fixed in phase 0 as work items, because the fleet monitor is their first
real consumer.

---

## 4. Programs required

One **standing monitoring program** plus the existing signal programs. Programs stay
event-agnostic; report requests ride on events, as in the spec.

### 4.1 `fleet-monitoring` (new)

- **Targets:** none (open to all VENs).
- **`payloadDescriptors`:** declares units for every report payload used below (`DEMAND` in
  `KW`… or `W` — see §9 D-2), `USAGE`/`BASELINE` in `KWH`, `STORAGE_CHARGE_LEVEL` in `PERCENT`.
- **Standing event `fleet-telemetry`:** one interval with a long `intervalPeriod` (e.g. 1 year,
  re-announced by the seed script), payload `SIMPLE` 0 (no DR action), carrying the
  `reportDescriptors` of §5. This fixes F-8 — the fleet is visible even when no DR event runs.

### 4.2 Signal programs for the event composer

The composer's templates (vision §VTN controller) need one program per signal family so targets
and descriptors stay clean:

| Program | Event payload types | Typical event |
|---|---|---|
| `fleet-tariff` | `PRICE`, `EXPORT_PRICE`, `GHG` | 24 h of 15-min price intervals |
| `fleet-capacity` | `IMPORT_CAPACITY_LIMIT`, `EXPORT_CAPACITY_LIMIT` (later: subscription/reservation) | "cap import at 5 kW for 1 h" |
| `fleet-dispatch` | `DISPATCH_SETPOINT`, `SIMPLE` | short setpoint / shed level |
| `fleet-alert` | `ALERT_GRID_EMERGENCY`, `ALERT_BLACK_START` | "emergency now, 30 min" |

The existing `Summer Peak DR`, `EV Managed Charging`, `HVAC Optimization`, `exp-*` and
`manual-uc*` programs stay untouched — experiments and manual use cases keep using them.
Seeding is idempotent by `programName` (extend `scripts/seed_vtn.py`, which already is for
programs; it is additive for events, so the standing event needs a name-based lookup first).

---

## 5. Reports required

Carried by the standing `fleet-telemetry` event. All values signed where the physics is signed.

| payloadType | readingType | Content | Unit | Interval | Needed by |
|---|---|---|---|---|---|
| `DEMAND` | `MEAN` | net site grid power, **signed** (import +, export −) | kW (D-2) | 60 s | §2 report series |
| `OPERATING_STATE` | — | freshness-derived state (companion payload, as today) | — | with each | §2 health |
| `USAGE_FORECAST` | `FORECAST` | planned net power per plan slot (`historical: false`) | kW | on replan / 15 min | later views (§4 flexibility, portfolio) — not required for 1/2/5 |

`DEMAND` in kW is the one conversion point: the VEN's own field stays `net_power_w` (W)
everywhere inside, MQTT telemetry publishes that field unchanged, and kW appears only at the
OpenADR boundary, declared in the program's `payloadDescriptors`.

The per-event report requests experiments already use (`USAGE` + `BASELINE` on `exp-*`) keep
working **unchanged**, including `USAGE`-as-mean-power-in-W (D-4): `experiments/kpi.py` and the
reporter BDD steps are not migrated here. That leaves `USAGE` with a non-spec meaning for those
events — recorded in `docs/BACKLOG_OpenADR_Cert.md` §6 as its own later change, and the reason
the fleet series is `DEMAND` rather than `USAGE`.

**One report object per VEN (D-3).** Each (VEN, event, payloadType) keeps its stable
`reportName` and report id; new intervals are appended by `PUT` on that id. Consequences to
build in: cache the id after the first create (F-6, removes the 409 + lookup), trim the
intervals array to a bounded window (24 h) so the object cannot grow without limit, and keep the
recorder's appended-interval lag measurement (GB-36), which already assumes exactly this shape.

**Descriptor semantics (fixes F-4, F-5).** Follow the spec: the report interval length comes
from the event's interval grid; `frequency` counts intervals between submissions; `numIntervals`
bounds how many intervals one report carries. A standing event has a single long interval, so
its report grid can't come from the event: the monitoring event's report intervals carry their
own `intervalPeriod` (allowed on report intervals) at the table's interval length. Either way,
each submission carries only the intervals **closed since the last submission** (no in-progress
bucket, no re-sent history). The VTN accumulates the series either by appending intervals via
`PUT` on the stable report id, or by one report per window — §9 D-3 decides which. How the
standing event expresses the 60 s / 15 min grid in its descriptor is part of the F-4 work.

Where exact spec semantics and the lab's needs differ, the lab convention is written down in
`docs/architecture/INTERFACES.md`, not left implicit.

---

## 6. MQTT side channel (live)

### 6.1 Broker

*(Revised 2026-09-19 — this section originally proposed a second listener on the house broker.)*

**`lab-mqtt`**, the lab's own broker, built during the 3.1 migration: a container in the VTN
stack on `openadr-net`, listening on **1884 everywhere** (container, docker network and host), so
the port alone says which broker is meant — `:1883` is always the house broker, `:1884` always
this one. `allow_anonymous false`, with the password file generated at start-up from environment
variables so no credential is committed.

A separate broker rather than a second listener: the house broker's password file and
`allow_anonymous` are config the home automation depends on, so the lab could not tighten auth
there without risking it (R-54). Here the lab owns the policy outright, which is what the 3.1
notifiers and any later certification profile need.

Addressing: the VTN and the Node1 VENs reach it by service name (`lab-mqtt:1884`); the Node2
fleet uses the published port over the LAN. The **inbound** feeds are untouched and still come
from the house broker — `openadr-lab/measurement/<site>/*` and `openadr-lab/weather/<site>/*` are
derived from real hardware on Node1, and each VEN feed is independently addressable
(`PV_MEASUREMENT_MQTT_HOST`, `BASE_LOAD_MEASUREMENT_MQTT_HOST`, `WEATHER_MQTT_HOST`), so no bridge
is needed. The split is by direction of travel: the house broker carries what the site
**measures**, `lab-mqtt` carries what the lab **generates**.

Still to do here: the per-VEN ACL, so a VEN may publish only below its own `venName`. `lab-mqtt`
authenticates every client but does not yet restrict topics.

### 6.1a Why not 3.1's native subscriptions?

*(Decided 2026-09-19, the re-decision the 3.1 migration deferred.)*

3.1 ships `/subscriptions` with three notifier transports (`/notifiers/{ws,mqtt,push-mqtt}`), and
`paho-mqtt` is a hard dependency of the VTN — so "the lab polls because OpenADR gives it nothing
better" is no longer true, and the question had to be asked again.

The answer is **both, for different jobs**, because they carry different things:

- **Subscriptions carry OpenADR objects.** A notification says a program, event, report, VEN or
  resource was created, updated or deleted. That is exactly what the BFF polls the VTN for today,
  so subscriptions could replace *that* polling and cut the "≤ 10 s after create/edit" latency in
  §1 to near zero.
- **They cannot carry VEN internals.** Net grid power, per-asset SoC, the adopted plan, a
  controller trace entry — none of these are OpenADR objects, and no amount of subscribing makes
  the VTN aware of them. The side channel in §6.2 exists precisely because the interesting fleet
  data never reaches the VTN.

So the side channel stays as specified, and native subscriptions become a **later, separate
improvement to the BFF's VTN-object freshness** — not a replacement for it. Two things to settle
before adopting them: `experimental-websockets` is currently disabled in our build because
upstream notes object privacy is unimplemented for it, and the VEN scope set omits
`write_subscriptions_ven` until a subscription actually exists to write.

### 6.2 Topics and payloads

Root `openadr-lab/fleet/<venName>/…`. Payloads reuse the JSON shape of the VEN route that
already serves the same data (DTO passthrough — no new vocabulary).

| Topic | Retained | QoS | Cadence | Body = shape of |
|---|---|---|---|---|
| `status` | yes | 1 | on connect; LWT `{"state":"offline"}` | `/health` subset + `venName`, version |
| `telemetry` | yes | 0 | every 5 s (configurable) | `ts`, `grid.net_power_w` (signed), per asset `power_kw`/`soc` — as `/sim` snapshot |
| `signals` | yes | 1 | on change | `GET /signals` |
| `trace` | no | 1 | on every `ControllerEvent` | one `/trace/events` entry |
| `plan` | yes | 1 | on adoption | `GET /plan` (later views; cheap to add now) |

Publishing is one outbound port in the VEN (`TelemetryPort`, no-op when
`FLEET_MQTT_HOST` is unset — same two-gate pattern as `real_measurement_mqtt.md`), fed from the
existing tick snapshot and trace log. It must not add a second derivation of net power: the
telemetry value and the `DEMAND` report both come from the same site-power function (F-9).

### 6.3 Correlation for reaction tracing

Today `ControllerEvent::OpenAdrArrived` carries `event_name` only — and the VTN does not enforce
unique event names. Needed additions:

- `OpenAdrArrived`: `eventID`, `modificationDateTime` (identifies the event **version** the VEN
  saw), VEN-side `ts`.
- `PlanCycle`: the triggering `eventID`(s) when the trigger is `RateChange`/`CapacityChange`/
  `Alert`.
- `OpenAdrExpired`: `eventID`.

Chain assembled by the BFF: VTN `createdDateTime`/`modificationDateTime` → VEN `OpenAdrArrived`
(seen) → `PlanCycle` (replanned) → first telemetry sample whose power deviates beyond a
threshold (reacted) → first report containing the interval (reported). Node1 and Node2 clocks
are NTP-synced from the LAN; the BFF records each VEN's clock offset (`status` publish time vs.
receive time) so sub-minute latencies can be judged against it.

---

## 7. BFF services

| Service | Responsibility |
|---|---|
| **Paginated VTN client** | one `get_all_pages` used by the list routes **and** the recorder (F-1, R-84) |
| **Fleet MQTT ingest** | subscribe `openadr-lab/fleet/+/#`; update in-memory latest state per VEN; persist telemetry and trace rows |
| **Recorder (existing)** | unchanged role; stores reports/events; after F-5, stores appended intervals rather than full snapshots |
| **Fleet store** | Existing Postgres, schema `lab_recorder` (D-7): `fleet_telemetry(ven_name, ts, net_power_w, payload_json)`, `fleet_trace(ven_name, ts, type, event_id, payload_json)`; batched inserts; retention job (raw 7 days, 1-min rollup 90 days) |
| **Fleet query API** | `GET /api/fleet/power?from&to&step&source=live\|report` — per-VEN series and fleet sum, aligned with the shared `resample_uniform`; `GET /api/fleet/signals?from&to` — events resolved to per-VEN bands with the shared interval-timing rule; `GET /api/fleet/reactions?eventID=` — the §6.3 chain |
| **Fleet stream** | `GET /api/fleet/stream` (SSE): telemetry, status, trace and VTN-object changes as they arrive |

Deliberately **not** in the BFF: flexibility or capability computation (asset competence) and
signal interpretation beyond "which interval runs at t" (that is the shared rule).

Rejected alternative: a separate time-series database (InfluxDB is reachable on Node1's
`influxdb_network`). It would add a second store and a second query language for data the BFF
already writes to Postgres; the volume (20 VENs × 5 s ≈ 350 k rows/day) is well within
Postgres range with a retention job.

### UI surface in phase 0

No fleet views yet, but no feed may be invisible: the VTN UI Dashboard's health card gains
"Fleet MQTT: connected, N/20 VENs live, last message Xs ago", and the Reports page shows the
`DEMAND`/`USAGE` series of `fleet-telemetry` with their source badge. The VEN UI Diagnostics
shows its own fleet-publisher status (connected / last publish).

---

## 8. Environment setup (before any view work)

1. **Broker** — Node1 Mosquitto: add the authenticated fleet listener, password entries per VEN
   and for the BFF, ACL file; restart only the `mosquitto` container (confirm with the user
   first — it is a production container outside this project's compose files).
2. **Compose** — `FLEET_MQTT_HOST/PORT/USER/PASSWORD` env for ven-1..3 (`VEN/docker-compose.yml`),
   ven-4..20 (`VEN/scale_out/node2/docker-compose.yml`), BFF (`VTN/docker-compose.yml`); secrets
   via `.env`, not committed.
3. **Test stack** — `tests/docker-compose.test.yml` already runs an ephemeral Mosquitto; add the
   fleet listener config there so BDD covers the authenticated path.
4. **Seeding** — `fleet-monitoring` + signal programs + the standing `fleet-telemetry` event, via
   the extended seed script; idempotent.
5. **Shared crate** — extract `VEN/src/common` (+ `event_timing`, `TimeWindow`) into a workspace
   crate used by VEN and BFF (D-06).
6. **Locks** — every Node1/Node2 docker build or test run under `scripts/docker_host_lock.sh`;
   WSL cargo under `scripts/wsl_lock.sh`; E2E preferably on Node2.

---

## 9. Decisions

| # | Question | Decision |
|---|---|---|
| D-1 | Telemetry cadence | 5 s default, configurable per VEN via profile/env |
| D-2 | `DEMAND` unit | **kW**, declared in `payloadDescriptors`; W stays the internal field unit (§5) |
| D-3 | Report accumulation | **one report object per VEN** per (event, payloadType): stable id, intervals appended by `PUT`, trimmed to a 24 h window (§5) |
| D-4 | Migrate `USAGE` to energy in this phase | **No** — `exp-*` reports and `experiments/kpi.py` stay as they are; the unit fix is its own change (`docs/BACKLOG_OpenADR_Cert.md` §6) |
| D-5 | Keep the timer report path | **No** — deleted once the standing monitoring event exists (resolves R-85 / F-9) |
| D-6 | Retention | raw telemetry 7 days, 1-min rollup 90 days |
| D-7 | Where telemetry is stored | **The existing Postgres instance** (`vtn-db-1`), schema `lab_recorder` — see below |

### D-7: Postgres, not a second database

The VTN's own store is Postgres 16 (`vtn-db-1`, currently 1.4 GB; Node1 has ~569 GB free), and
the BFF already writes reports/events/VEN snapshots into the `lab_recorder` schema of that same
instance with `sqlx`. Fleet telemetry joins it as two more tables in that schema.

- **No new service, no new dependency.** A second store (SQLite file or an InfluxDB instance)
  adds a deployment unit, a second backup story and a second query language for data one
  service already writes.
- **Correct writer model.** The BFF's ingest task and the recorder write concurrently; SQLite's
  single-writer locking is the wrong fit, and a SQLite file inside a container needs its own
  volume and retention tooling.
- **Retention is SQL.** The 7 d / 90 d policy (D-6) is a scheduled `DELETE` plus a rollup
  `INSERT … SELECT`; monthly partitions can be added later if deletes get expensive.
- **Volume fits.** 20 VENs × one sample per 5 s ≈ 350 k rows/day; with the payload JSON roughly
  50–100 MB/day raw, so ~0.5 GB at 7 days' retention, and the 1-min rollup is ~1/12 of that.
- **Isolation.** Separate schema, never openleadr-rs's own tables (unchanged recorder rule).
  Writes are batched (one multi-row insert per second, not one per message) so telemetry ingest
  does not compete with the live VTN for IO on the Pi.
- **Escape hatch.** If IO on Node1 ever becomes the constraint, the `lab_recorder` schema can
  move to its own Postgres container without touching the query API, since nothing joins across
  the two schemas.

Rejected: InfluxDB (reachable on Node1's `influxdb_network`) — better suited to this data shape,
but it would split the monitor's storage across two systems for a volume Postgres handles
comfortably.

---

## 10. Sequence

Each step is test-first and leaves the stack deployable.

1. **R-84** BFF pagination consolidation (unblocks correct report/event lists).
2. **Report fixes** F-3 (signed values), F-6 (cached report id), F-7 (no history copy when
   nothing is due), F-9/D-5 (delete the timer path). `USAGE` semantics stay put (D-4).
3. **Descriptor semantics and accumulation** F-4, F-5, D-3 (append + 24 h trim).
4. **Shared crate** extraction (D-06).
5. **Programs + seeding** (§4) and the `fleet-telemetry` report requests (§5).
6. **Broker + compose environment** (§8.1–8.3).
7. **VEN telemetry publisher** (§6.2) + trace correlation fields (§6.3), with VEN UI diagnostic.
8. **BFF ingest, store, query API, stream** (§7), with the phase 0 UI surface.
9. **BDD**: a scenario that creates an import-limit event and asserts that `/api/fleet/power`
   (live and report) and `/api/fleet/reactions` show every targeted VEN's reaction.

Views §1, §2, §5 follow as phase 1 on top of these endpoints.
