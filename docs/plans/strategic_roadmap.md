# Strategic Roadmap — Ordered Backlog by User Story

> **Date:** 2026-07-06
> **Sources:** `docs/BACKLOG.md` (BL-02…BL-29, GB-01…GB-10, dependency audit),
> `wiki/use-cases/openadr-spec-use-cases.md` (spec-implied gaps, §5–§8),
> `docs/BACKLOG_OpenADR_Cert.md` (certification detail behind those gaps),
> `wiki/overview/vision-and-roadmap.md` (long-term directions).
> **Purpose:** Group all open items into topics, derive user stories, and produce a
> priority-ordered backlog aligned with two strategic focuses: **client comfort** and
> **VTN-side benefit**, on the path to the next big goal — a **fleet of independent VEN
> agents** whose responses to VTN control methods (tariffs, limits, events) and whose
> reports can be observed and evaluated. A second vision — **heuristic forecasts from
> past data** — requires a usage/history store in both VEN and VTN, addressed here as
> the Data Foundation.

---

## 1. Strategic Goals

| ID | Goal | What "done" looks like |
|----|------|------------------------|
| **SG-1 Fleet** | Run a fleet of independent, *diverse* VEN agents against one VTN | `docker compose` (or script) brings up N VENs with distinct profiles/personas; they run for days without manual care; naming/IDs are uniform |
| **SG-2 Control-method lab** | Observe and compare the influence of VTN control methods — tariffs, capacity limits, events (alerts, setpoints, priorities) | Repeatable experiment scenarios drive the fleet; per-method KPIs (cost, peak, comfort violations, compliance latency) are computed from logged data |
| **SG-3 Report evaluation** | Judge the *usefulness* of VEN reports from the VTN side | VTN archives all incoming reports; an operator view answers "could a utility have dispatched better with these reports?" — coverage, timeliness, accuracy vs. actual metering |
| **SG-4 Forecast from history** | VEN (and VTN) learn heuristics from their own past data | Persistent usage DB in each VEN; `AssetHeuristics`/`AssetForecast` populated from it; planner measurably better than flat/last-known extrapolation on uncontrollable loads |
| **SG-5 Client comfort** | The simulated resident's experience is first-class | Requests carry urgency/cost intent, comfort curves are user-overridable, the UI shows past + planned behaviour, notifications explain what the HEMS did and why |

SG-1–SG-3 are the **VTN-side benefit** axis; SG-4–SG-5 are the **client comfort** axis.
They meet in the middle: a diverse fleet is only interesting if each VEN models a
*resident with preferences* (SG-5 features double as fleet-diversity drivers), and
forecasting (SG-4) is what makes reports worth evaluating (SG-3).

---

## 2. Topic Clusters — every open item, grouped

Legend: `BL-xx`/`GB-xx` = `docs/BACKLOG.md` · `UC:x` = gap row from
`wiki/use-cases/openadr-spec-use-cases.md` / `docs/BACKLOG_OpenADR_Cert.md`.

### Cluster A — Data Foundation (history, persistence, ledger)
The prerequisite for SG-2, SG-3, SG-4 and half of SG-5. **Nothing here exists yet.**

| Item | Description |
|------|-------------|
| **NEW A-1** | VEN usage database (SQLite) — persist downsampled tick history, plan snapshots, events received, reports sent (design in §6) |
| **NEW A-2** | VTN-side report/event archive (recorder on existing Postgres, design in §6) |
| BL-“log for past” (unnumbered, `BACKLOG.md` line 129) | Show past behaviour in VEN UI — becomes a thin view over A-1 |
| BL-16 | `AssetLedger` — per-asset billing-period cost/CO₂ rollup with period reset; persistence lands naturally on A-1 |
| **NEW A-3** | KPI extraction jobs — per-VEN daily cost, peak import kW, deadline misses, comfort violations, event-compliance latency (feeds SG-2/SG-3 dashboards) |

### Cluster B — Forecasting & Heuristics (SG-4)

| Item | Description |
|------|-------------|
| BL-15 | `AssetForecast` — expose planner's per-slot forecast in the documented shape; source for `USAGE_FORECAST` report |
| BL-14 | `AssetHeuristics` — learn daytime/weekday/seasonal profiles from A-1 history for uncontrollable loads |
| BL-17 | `ExternalDataSource` — weather/irradiation/CO₂ feed ingestion for `ForecastSource::WeatherModel` |
| BL-08 | `SITE_RESIDUAL` — compute unmodeled site load; the main *consumer* of heuristics (it's exactly the load with no physical model) |
| UC:baseline (§7.5) | `BASELINE` computation for M&V — needs history (A-1) to compute "what would have happened" |

### Cluster C — Reporting Depth (SG-3, VTN-side visibility)

| Item | Description |
|------|-------------|
| BL-05 | Obligation-triggered report submission (reports at `due_at`, not only on timer) |
| BL-10 | `FlexibilityEnvelope` → `IMPORT/EXPORT_CAPACITY_RESERVATION` reports to VTN |
| UC:§8.8 | Operational forecast reporting — `USAGE_FORECAST` report from BL-15 |
| UC:§8.7 | Capability forecast reporting — parse `reportDescriptor.historical`, add `LOAD_SHED_DELTA_AVAILABLE`/`GENERATION_DELTA_AVAILABLE` payloads |
| UC:device-status | `OPERATING_STATE` is hardcoded `"ACTIVE"` — derive from real asset state (`DeviceResponsiveness` vocabulary) |
| UC:§7.5 | Rolling / ad-hoc / `report-only` report management |
| UC:quality | Data-quality metadata (accuracy/confidence) in report payloads |

### Cluster D — Event Arbitration & VTN Control Methods (SG-2)
The features that make the *control-method comparison* meaningful — each row is one
"control knob" the VTN can turn.

| Item | Description | Control method |
|------|-------------|----------------|
| BL-02 / UC:§7.1 | Event priority ordering before merge | overlapping events |
| BL-04 / UC:§8.1–8.2 | `ALERT_GRID_EMERGENCY` / `ALERT_BLACK_START` → planner shed | emergency alerts |
| UC:SIMPLE | Map `SIMPLE` levels 0–3 to shed behaviour | classic load shed |
| BL-06 / UC:§8.5, §8.12 | `DISPATCH_SETPOINT` (bypass planner) + `CHARGE_STATE_SETPOINT` (EV session) | direct control |
| UC:§8.10 | Make parsed `IMPORT_CAPACITY_SUBSCRIPTION`/`_RESERVATION` actually constrain the solver; handle export side | capacity management |
| BL-24 | `OadrEventCache.dispatch_setpoints` storage (lands with BL-06); `OadrCapacityRequest` (VEN *requests* capacity — future negotiation) | capacity negotiation |
| UC:opt-in/out | Opt-in/opt-out signalling per event | participation choice |
| UC:§8.4 | Inverter management — curtailment/power-factor setpoints on PV | generation control |

### Cluster E — Client Comfort & Resident UX (SG-5)

| Item | Description |
|------|-------------|
| BL-28 | `UserRequestMode` (ASAP, BY_DEADLINE, MAX_COST, OPPORTUNISTIC, …) — how the resident *expresses* a request; also the main fleet-diversity driver |
| BL-19 | `DefaultValueCurve` — user-overridable comfort/bid curves per asset |
| BL-20 | User notification feed (Info/Warn/Alert) — tier fallback, budget warning, deadline risk, grid emergency |
| BL-27 | `PowerAdjustability`/`PowerRange` — control-mode metadata so UI sliders snap to real device steps |
| BL-18 | `AssetFlexibility` — instantaneous per-asset "how much can I flex now" (needs the scope decision recorded in BL-18 first) |
| BL-16 (again) | Ledger — the resident's "what did each device cost me this month" view |

### Cluster F — Planner Fidelity (quality of decisions, serves both axes)

| Item | Description |
|------|-------------|
| BL-11 | Time-weighted tariff averaging per slot |
| BL-07 | `StaleRatePolicy` dispatch when tariff horizon runs out |
| BL-13 | Early firm-up heuristic under flat rates |
| BL-09 | Phase 6 penalty-threshold check (peak-demand penalties) |
| BL-12 | EV minimum charge rate + response delay in simulator |
| UC:§7.4 | Variable-duration interval edges blur inside plan-grid resolution |

### Cluster G — Fleet Operations & Robustness (SG-1)

| Item | Description |
|------|-------------|
| GB-02, GB-03 | Unify VEN naming; UUID VEN IDs |
| GB-07 | Setup script to bring up all containers → extend to *N-VEN fleet generator* |
| GB-09 | Poll interval configurable per profile |
| GB-06 | DB-reset script for re-seeding |
| BL-03 | Exponential backoff + jitter on VTN failure (essential once N VENs hammer one VTN) |
| BL-25 | `VtnUnreachable` / `PlanInfeasible` error variants at real boundaries (fleet debugging) |
| UC:pagination | `skip`/`limit` pagination in `vtn.rs` — silent data loss once fleet-scale event/report volume exceeds one page |
| UC:RFC7807 | Problem-response parsing — structured errors for fleet debugging |
| UC:§6.7 | VEN/resource self-registration (`POST /vens`, `POST /resources`) — removes per-VEN manual seeding, big fleet enabler |

### Cluster H — Transport Modernisation (certification track, deferred)

| Item | Description |
|------|-------------|
| UC:webhooks | Subscription objects + webhook receiver (cuts 30 s poll latency) |
| UC:MQTT | Optional MQTT listener |
| UC:TLS | HTTPS/TLS 1.2+ (certification MUST) |
| UC:auth-server | `/auth/server` token-endpoint discovery |
| UC:mDNS, randomizeStart, "now" sentinel, gzip, runtime reconfig | Remaining cert line items |

### Cluster I — Hygiene & Debt (background queue)

| Item | Description |
|------|-------------|
| BL-21, BL-26, BL-29 | Design-vocabulary cleanup (duplicate `ThermalModelParams`, `AssetState` collision, narrow enums fold-in) |
| BL-22 | Battery correction overlay — wire behind flag or re-confirm abandoned |
| BL-23 | `HvacService` — route through or delete shell |
| GB-01, GB-04, GB-05, GB-08, GB-10 | Docker orphans, DB index, VTN UI event filter, VEN UI tests, compiler warnings |
| Dependency vulns | `cargo audit` / `npm audit` findings (reqwest TLS stack, esbuild) — batch before any internet-exposed deployment; pairs naturally with Cluster H TLS work |

---

## 3. User Stories — priority order

Personas: **Resident** (simulated household user), **Operator** (VTN-side
utility/aggregator role, played by the lab owner), **Researcher** (the lab owner
studying fleet dynamics), **VEN** (the agent itself, for autonomy stories).

### P0 — Enablers (nothing downstream works without these)

**US-01 · Researcher — "I can replay history."**
*As a researcher, I want every VEN to persistently record what it measured, planned,
received and reported, so that any later question ('why did VEN-7 import 5 kW at 17:00?')
is answerable after the fact.*
→ **NEW A-1** (VEN SQLite), BL-“log for past” UI, BL-16 ledger persistence.
Enables: SG-2, SG-3, SG-4, half of SG-5. **This is the single highest-leverage item.**

**US-02 · Operator — "I can see what the fleet told me."**
*As a VTN operator, I want all reports and event acknowledgements archived with
timestamps, so I can later evaluate whether the reports would have been useful for
dispatch decisions.*
→ **NEW A-2** (VTN recorder on Postgres), GB-04 (index), GB-05 (UI filter).

**US-03 · Researcher — "I can launch a fleet."**
*As a researcher, I want to bring up N independent VENs with distinct profiles in one
command, so fleet experiments are cheap to set up and tear down.*
→ GB-02, GB-03, GB-07 (fleet generator), GB-09, GB-06, UC:§6.7 self-registration.

**US-04 · VEN — "I don't stampede the VTN."**
*As a VEN in a fleet, I want backoff + jitter on failures and paginated queries, so N
agents polling one VTN stay stable.*
→ BL-03, UC:pagination, UC:RFC7807, BL-25.

### P1 — The control-method laboratory (next big goal)

**US-05 · Researcher — "I can compare control methods."**
*As a researcher, I want to run scripted scenarios (tariff series vs. capacity limits
vs. events) against the same fleet and compare KPIs, so I can characterise each VTN
control method's influence.*
→ **NEW A-3** KPI jobs + experiment harness (§7), consuming A-1/A-2.

**US-06 · Operator — "My signals are honoured correctly."**
*As a VTN operator, when I send overlapping or prioritised events, an emergency alert,
or a capacity reservation, I want each VEN to arbitrate and respond per spec, so the
control methods I'm comparing actually differ in effect.*
→ BL-02 (priority), BL-04 (alerts), UC:SIMPLE levels, UC:§8.10 (reservations constrain
solver). This is the "event arbitration" cluster the wiki's OPEN QUESTION asks about —
this roadmap resolves it in favour of **lab-learning value over certification pressure**.

**US-07 · Operator — "I can see the fleet's flexibility."**
*As a VTN operator, I want each VEN to report its flexibility envelope and usage
forecast, so I can judge available DR capacity before sending signals.*
→ BL-10, BL-15, UC:§8.8 `USAGE_FORECAST`, BL-05 (reports arrive when due), UC:device-status.

**US-08 · Operator — "I can take direct control when needed."**
*As a VTN operator, I want to send `DISPATCH_SETPOINT` / `CHARGE_STATE_SETPOINT` and
see the VEN comply within one poll cycle, so direct load control is a comparable
control method alongside tariffs.*
→ BL-06, BL-24 (event cache), UC:§8.12.

### P2 — Client comfort (differentiates the fleet, serves the resident)

**US-09 · Resident — "The HEMS understands my intent."**
*As a resident, I want to say 'charge ASAP regardless of cost' or 'whenever it's
practically free', so the plan reflects my actual urgency, not a one-size deadline.*
→ BL-28 `UserRequestMode`. Doubles as the **fleet persona mechanism**: assign each VEN
a persona (eco-optimizer, comfort-first, opportunist) by defaulting request modes and
comfort curves differently.

**US-10 · Resident — "I can tune my comfort."**
*As a resident, I want to override an asset's default comfort curve with my own, so the
optimizer trades my comfort against cost the way* I *value it.*
→ BL-19, BL-27 (UI control steps), BL-18 (live flex widget — pending its scope decision).

**US-11 · Resident — "I'm told what happened and what it cost."**
*As a resident, I want a notification feed (deadline at risk, budget exceeded, grid
emergency curtailed my charging) and a per-device monthly cost view, so the HEMS is
trustworthy rather than opaque.*
→ BL-20 notifications, BL-16 ledger UI, BL-“log for past” (history view).

**US-12 · Resident — "Bad connectivity doesn't break my home."**
*As a resident, when the VTN is unreachable or the tariff horizon runs out, I want the
HEMS to degrade gracefully per policy, so comfort survives outages.*
→ BL-07 `StaleRatePolicy`, (BL-03 already in P0).

### P3 — Forecasting from history (second vision)

**US-13 · VEN — "I learn my household's patterns."**
*As a VEN, I want to learn daytime/weekday/seasonal profiles of my uncontrollable loads
from my own history, so my plan grid rests on realistic baselines instead of flat
extrapolation.*
→ BL-14 (needs weeks of A-1 data — hence P3 by dependency, not by value), BL-08
`SITE_RESIDUAL` (what the heuristic forecasts), BL-15 (the shape it's delivered in).

**US-14 · VEN — "I use external forecasts."**
*As a VEN, I want weather/irradiation/CO₂ feeds, so PV and thermal forecasts come from
physics + weather rather than history alone.*
→ BL-17.

**US-15 · Operator — "Baselines make reports meaningful."**
*As a VTN operator, I want baseline (`BASELINE`) and capability-forecast reports, so I
can measure event impact (M&V) instead of just observing consumption.*
→ UC:baseline §7.5, UC:§8.7, UC:quality metadata. The **report-usefulness evaluation
(SG-3) becomes rigorous only here** — before baselines, "usefulness" is qualitative.

### P4 — Planner fidelity & simulation realism

**US-16 · Resident — "My bill is computed right."**
*As a resident, I want slot costs to reflect tariff boundaries inside a slot and peak
penalties, so plans optimize my real bill.*
→ BL-11, BL-09, BL-13, UC:§7.4.

**US-17 · Researcher — "The simulation behaves like hardware."**
*As a researcher, I want EV minimum charge floors and response delays modeled, so fleet
results transfer to real devices.*
→ BL-12.

### P5 — Certification track (valuable, not urgent for the lab)

**US-18 · Operator — "Sub-second signal delivery, secure transport."**
*As a VTN operator, I want webhook/MQTT push and TLS, so the lab could graduate toward
certification and low-latency DR.*
→ Cluster H entire, plus dependency-vuln batch (Cluster I) since the TLS work touches
the same reqwest stack.

### Continuous — Hygiene

**US-19 · Maintainer — "Debt doesn't silently accumulate."**
→ Cluster I; fold items in opportunistically when touching the same files (per the
refactoring rule in `.claude/CLAUDE.md`), except the vuln batch which rides with US-18.

---

## 4. Feature Priority List (derived)

Ordered execution queue. Effort from `BACKLOG.md` where stated; NEW items estimated.

| # | Feature / Item | Story | Effort | Notes |
|---|----------------|-------|--------|-------|
| 1 | **NEW A-1: VEN SQLite history store** | US-01 | L | Design §6.1 — everything downstream feeds on this |
| 2 | BL-16 `AssetLedger` on top of A-1 | US-01/11 | M–L | Period rollover + persistence |
| 3 | History view in VEN UI (BL-“log for past”) | US-01/11 | M | Read-only over A-1 |
| 4 | **NEW A-2: VTN recorder** (Postgres) | US-02 | M | Sidecar, no fork changes — §6.2 |
| 5 | GB-02 + GB-03 VEN naming/UUID | US-03 | S | Do before fleet generator bakes in old names |
| 6 | GB-07→fleet generator + GB-09 + GB-06 | US-03 | M | Profile templating for N VENs |
| 7 | BL-03 backoff + jitter | US-04 | M | Before scaling fleet size |
| 8 | UC:pagination in `vtn.rs` | US-04 | S–M | Fleet-scale data volume |
| 9 | **NEW A-3: KPI jobs + experiment harness** | US-05 | L | §7 — first fleet experiment runs here |
| 10 | BL-02 event priority | US-06 | S | 1–2 h, high spec value |
| 11 | BL-04 alert shed | US-06 | M | |
| 12 | UC:SIMPLE level mapping | US-06 | S–M | |
| 13 | UC:§8.10 reservations constrain solver | US-06 | M | Data already parsed |
| 14 | BL-05 obligation-triggered reports | US-07 | S–M | |
| 15 | BL-10 envelope → capacity-reservation reports | US-07 | M | |
| 16 | BL-15 `AssetForecast` + `USAGE_FORECAST` report | US-07 | M | Plumbing of existing planner output |
| 17 | UC:device-status real `OPERATING_STATE` | US-07 | S | |
| 18 | BL-06 dispatch/charge-state setpoints (+BL-24 cache) | US-08 | M | |
| 19 | BL-28 `UserRequestMode` (+ persona profiles) | US-09 | M–L | Fleet diversity driver |
| 20 | BL-19 comfort-curve override | US-10 | S–M | |
| 21 | BL-20 notification feed | US-11 | M | |
| 22 | BL-07 `StaleRatePolicy` | US-12 | M | |
| 23 | BL-08 `SITE_RESIDUAL` | US-13 | M | Prerequisite for meaningful heuristics |
| 24 | BL-14 `AssetHeuristics` from history | US-13 | L | Needs weeks of A-1 data — start collection early! |
| 25 | BL-17 `ExternalDataSource` | US-14 | L | Provider choice first |
| 26 | UC:baseline + UC:§8.7 capability forecast | US-15 | L | Rigorous SG-3 evaluation |
| 27 | BL-11 time-weighted tariffs | US-16 | S–M | |
| 28 | BL-13 early firm-up | US-16 | S | |
| 29 | BL-09 penalty threshold | US-16 | L | |
| 30 | BL-12 EV floor + delay | US-17 | S | |
| 31 | BL-27 / BL-18 UI control metadata / live flex | US-10 | M | After scope decisions |
| 32+ | Cluster H (webhooks, TLS, MQTT…) + vuln batch | US-18 | XL | Certification track |
| bg | Cluster I hygiene | US-19 | S each | Opportunistic |

Items #10–13 (~1 week) unlock the *entire* control-method comparison and are
individually small — **the best value density in the queue**. Consider pulling #10
(BL-02) forward even before the data foundation, since it's 1–2 h.

---

## 5. Roadmap Phases

> **Implementation plans:** one plan file per phase lives in
> [`docs/plans/roadmap/`](roadmap/README.md) — work packages, test-first steps,
> decision points and exit demonstrations for each phase below.

```
Phase 0  Quick wins            BL-02 · GB-02/03 · BL-12 · warnings (GB-10)
Phase 1  Data Foundation       A-1 SQLite · BL-16 ledger · history UI · A-2 recorder
         └─ start collecting history NOW — Phase 4 heuristics need weeks of it
Phase 2  Fleet Enablement      fleet generator · BL-03 · pagination · self-registration
Phase 3  Control-Method Lab    BL-04 · SIMPLE · §8.10 constraints · BL-05/10/15 reports
         └─ A-3 KPI harness → FIRST FLEET EXPERIMENT (SG-1 + SG-2 demonstrated)
Phase 4  Comfort & Personas    BL-28 modes · BL-19 curves · BL-20 notifications · BL-07
         └─ personas re-run the Phase-3 experiments with a *diverse* fleet
Phase 5  Forecast & Baseline   BL-08 · BL-14 · BL-17 · BASELINE/§8.7 → rigorous SG-3
Phase 6  Fidelity & Cert       BL-11/09/13 · Cluster H + vuln batch (as needed)
```

Each phase ends with a runnable demonstration, not just merged code:
- **P1:** "show me yesterday" works in the VEN UI after a container restart.
- **P2:** `./fleet.sh up 10` yields 10 healthy VENs, VTN load stable.
- **P3:** one experiment report comparing tariff-only vs. limit-only vs. event-only days.
- **P4:** the same experiment with 3 personas shows measurably different fleet response.
- **P5:** heuristic forecast beats last-known extrapolation on a held-out week; baseline
  reports quantify one event's impact.

---

## 6. Data Foundation — design suggestions

### 6.1 VEN usage database (NEW A-1)

**Store: SQLite via `rusqlite` with the `bundled` feature** (compiles its own C sqlite;
no system dependency — works under WSL and in the Pi4 ARM containers). Alternatives
considered: `sqlx-sqlite` (async, but pulls a larger dependency tree and the write path
is a background task anyway — blocking writes on a dedicated thread are fine); plain
CSV/Parquet append (cheap but kills ad-hoc queries that SG-2/SG-3 analysis needs).
Pin per the dependency policy; licence MIT — acceptable.

**Architecture:** a `HistoryPort` trait in the application layer (services), SQLite
adapter in infra — same hexagonal pattern as `SolverPort`/`VtnPort`. Injectable clock
per the determinism rule. Mock in `services/test_support/` for unit tests.

**Write path:** subscribe to the existing 1 s monitor tick; **downsample to 1-minute
means** before insert (1 s × 5 assets × 90 days ≈ 39 M rows raw — too much; 1-min ≈
650 k rows ≈ tens of MB — fine). Batch inserts once per minute from a dedicated task
(`tasks/` file ≤ 200 lines per the size rule).

**Suggested schema (unit suffixes per naming rule):**

```sql
tick_samples    (ts, asset_id, power_kw, soc_pct, temperature_c)      -- 1-min means
grid_samples    (ts, import_kw, export_kw, tariff_eur_per_kwh,
                 export_tariff_eur_per_kwh, co2_g_per_kwh)
plan_snapshots  (created_at, horizon_start, horizon_end, plan_json)   -- one per plan cycle
events_received (received_at, event_id, event_type, payload_json)
reports_sent    (sent_at, report_type, event_id, payload_json)
ledger_periods  (asset_id, period_start, period_end,
                 energy_kwh, cost_eur, co2_kg)                        -- BL-16 lands here
notifications   (created_at, severity, message, asset_id)             -- BL-20 lands here
```

**Retention:** ring-delete beyond a configurable window (default 90 days) in the same
minute-batch task. **Location:** one file per VEN, e.g. `/data/history.sqlite`, mounted
as a docker volume so it survives container rebuilds.

**Consumers:** history UI (BL-“log for past”), BL-16 ledger, BL-14 heuristics
aggregation, A-3 KPI extraction, BASELINE computation (US-15).

### 6.2 VTN-side archive (NEW A-2)

The VTN (openleadr-rs fork) **already runs Postgres** — do *not* introduce SQLite
there. Two options:

1. **Sidecar recorder (recommended):** a small service (or a module in the existing
   BFF) that polls the VTN API with an `any-business` credential and appends
   reports/events/VEN states into its own Postgres schema (`lab_recorder.*`, same
   Postgres instance, separate schema). *No fork changes* — keeps the upstream-PR
   surface clean, and the recorder sees exactly what a BL client would see, which is
   itself the right vantage point for judging report usefulness (SG-3).
2. Fork-side table triggers/hooks — rejected: widens the fork diff, and upstream PRs
   are supposed to stay minimal.

Recorder schema mirrors §6.1's `events_received`/`reports_sent` shape plus
`ven_snapshots (ts, ven_name, last_seen, report_lag_s)` for fleet-health KPIs.

### 6.3 Shared vocabulary

Keep OpenADR field names end-to-end in both stores (DTO-passthrough rule) — archived
`payload_json` is the raw wire object, with typed extracted columns only for the fields
KPI queries filter on.

---

## 7. Fleet & Experiment Harness — suggestions (SG-1/SG-2)

**Fleet generation:** extend GB-07's setup script into `fleet.sh up N` — templates a
VEN profile per instance (UUID id per GB-03, distinct asset mix, persona defaults per
BL-28/BL-19 once available), generates a compose override file, seeds the VTN
(idempotent, via GB-06 reset), and health-checks. Stagger poll offsets (GB-09) so N
VENs don't align their 30 s polls.

**Personas (Phase 4):** e.g. *eco-optimizer* (OPPORTUNISTIC modes, aggressive comfort
curves), *comfort-first* (ASAP modes, flat curves), *absent commuter* (EV deadline
07:00, low daytime base load). Personas are just profile presets — no new code beyond
BL-28/BL-19.

**Experiment scenarios (A-3):** scripted VTN drivers, each isolating one control method
against the same fleet and same simulated weather/day:

| Scenario | VTN behaviour | Question answered |
|----------|---------------|-------------------|
| S-1 flat tariff | constant price, no events | baseline fleet behaviour |
| S-2 dynamic tariff | day-ahead PRICE curve | how much load shifts on price alone? |
| S-3 capacity limit | `IMPORT_CAPACITY_LIMIT` window at peak | do limits beat prices for peak shaving? |
| S-4 emergency | ALERT_GRID_EMERGENCY mid-day | shed depth + speed across fleet |
| S-5 direct dispatch | DISPATCH_SETPOINT to a subset | precision vs. side effects on comfort |
| S-6 combined | tariff + limit + one event | interaction / arbitration (BL-02) |

**KPIs per scenario, per VEN and fleet-aggregate** (computed from A-1 + A-2):
total cost (€/day), peak import (kW), load factor, energy shifted (kWh vs. S-1),
comfort violations (deadline misses, temperature-band exits, unmet SoC), event
compliance latency (signal receipt → measurable response), report timeliness
(due_at → received_at) and report accuracy (forecast vs. later actuals — the SG-3
usefulness metric).

**Report-usefulness evaluation (SG-3), concretely:** for each scenario, compare the
operator's *predicted* fleet response (from `USAGE_FORECAST`/flexibility-envelope
reports, A-2) against *actual* metered response (A-1 ground truth). Usefulness =
prediction error + coverage + timeliness. Before Phase 5's baselines this is
directional; after baselines it becomes M&V-grade.

---

## 8. Deferred / explicitly de-prioritised

- **Cluster H transport work** — the wiki's OPEN QUESTION ("certification pressure vs.
  lab-learning value") is answered here: **lab-learning first**. Webhooks/TLS/MQTT don't
  change fleet dynamics at 30 s poll resolution; revisit when latency experiments or
  certification become goals. Exception: the dependency-vuln batch should ride along
  whenever the reqwest stack is next touched.
- **BL-22, BL-23, BL-26, BL-21, BL-29** — decision-shaped debt, not features; keep in
  Cluster I and resolve opportunistically.
- **`OadrProgramConfig` / `OadrCapacityRequest` (BL-24 tail)** — capacity *negotiation*
  (VEN requesting capacity) is a compelling Phase-3+ extension of §8.10, but has no
  driving experiment yet; add a scenario S-7 when it does.
- **OpenADR 3.0→3.1 migration, mDNS, opt-in/out** — track in the cert backlog; opt-in/out
  becomes interesting only when fleet *participation choice* is an experiment variable.

---

## 9. Follow-ups for `docs/BACKLOG.md`

The note at the top of `BACKLOG.md` asks for a re-sort pass — this document *is* that
pass, at story level. Suggested bookkeeping (not done yet, to avoid churn on the
current branch):

1. Add a `Roadmap phase` column/tag to each BL/GB item referencing this file.
2. Number the unnumbered "Add log for past" line as **BL-30** and give it the standard
   Problem/Fix/Verify structure (it's feature #3 in §4).
3. Register **A-1/A-2/A-3** as new backlog items (suggest BL-31 VEN history store,
   BL-32 VTN recorder, BL-33 KPI harness) so they exist in the canonical list.
4. Update `wiki/overview/vision-and-roadmap.md` via `/wiki-sync` after this file merges,
   and resolve the OPEN QUESTION in `wiki/use-cases/openadr-spec-use-cases.md` (answered
   in §8 above).
