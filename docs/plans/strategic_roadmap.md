# Strategic Roadmap

> **Refreshed:** 2026-07-16 (original 2026-07-06; execution record of the completed
> phases lives in `docs/history/project_journal.md` and `docs/plans/roadmap/`).
> **Purpose:** the priority-ordered view of what remains, aligned with the two
> strategic focuses — **client comfort** and **VTN-side benefit** — on the path to a
> **fleet of independent VEN agents** whose responses to VTN control methods and whose
> reports can be observed and evaluated.

---

## 1. Strategic Goals — status

| ID | Goal | Status |
|----|------|--------|
| **SG-1 Fleet** | Run a fleet of independent, *diverse* VEN agents against one VTN | **Built.** `fleet.sh up N` (bulk registration, personas, health checks); resource budget on Node1 caps practical size at N=3 base + fleet VENs |
| **SG-2 Control-method lab** | Observe and compare VTN control methods (tariffs, limits, alerts, SIMPLE, dispatch) | **Built and exercised.** Two full S-1…S-6 comparison runs done 2026-08-09/10 (non-persona baseline + persona re-run) — see `docs/history/project_journal.md`. Capacity limits/alerts measurably shift load; a price signal alone did not, in this run |
| **SG-3 Report evaluation** | Judge the *usefulness* of VEN reports from the VTN side | **M&V-grade.** VTN recorder archives reports incl. `report_lag_s` (crash-fixed 2026-08-10); `BASELINE` reports ship (WP5.4, 2026-08-11) — `experiments/kpi.py`'s `event_impact_kwh` quantifies an event's actual impact from archived BASELINE vs. USAGE pairs |
| **SG-4 Forecast from history** | VEN learns heuristics from its own past data | **Mostly done.** History store + learned weekday/weekend heuristics ship and feed the planner; a live weather feed (MQTT, `docs/architecture/weather_forecast.md`) now drives a physics-based PV forecast too. Remaining: the held-out-week validation demo and an external grid-CO₂ feed (BL-17) |
| **SG-5 Client comfort** | The resident's experience is first-class | **Done.** Request modes, comfort-curve overrides, notifications, History UI ship; comfort curves shape MILP reward terms for EV/heater sessions (BL-34, shipped 2026-07-31; R-18 EV extra-reward coupling fixed 2026-08-11 — `docs/architecture/ven_milp_planner.md` §10) |

SG-1–SG-3 are the **VTN-side benefit** axis; SG-4–SG-5 the **client comfort** axis.

---

## 2. Where the open items live

- `docs/BACKLOG.md` — feature gaps: BL-11, BL-13, BL-17, BL-18, BL-21…BL-27,
  BL-29, BL-34, BL-35; general items GB-04, GB-05, GB-07, GB-09, GB-11.
- `docs/BACKLOG_OpenADR_Cert.md` — certification/transport line items (Cluster H).
- `docs/reference/TECHNICAL_DEBTS.md` — the debt register (R-18…R-40).
- `docs/plans/refactoring_backlog.md` — detailed diagnostics for open register items (currently empty).
- `docs/plans/roadmap/` — the per-phase implementation plans (phases 0–4 executed;
  phase 5 partially; phase 6 not started).

---

## 3. Remaining work, priority order

### 3.1 The experiment windows — done (2026-08-09/10)

Both the S-1…S-6 control-method comparison and the persona re-run have now run against the
live Node1 fleet; full results and the debugging saga (a dead VTN recorder found and fixed
mid-arc) are in `docs/history/project_journal.md`. Remaining open questions from that work:
GB-19-class findings (see BACKLOG.md) rather than un-run demonstrations.

Scenario matrix and KPI definitions: §4 below.

### 3.2 Phase 5 remainder — Forecast & Baseline (SG-3/SG-4 rigor)

| WP | Item | Content |
|----|------|---------|
| WP5.3 | BL-17 weather/PV done; CO₂ remains | PV forecasting from physics + a live MQTT weather feed ships (`docs/architecture/weather_forecast.md`, tagged `ForecastSource::WeatherModel`) — an MQTT push from SRF Meteo rather than the originally-sketched `ExternalDataSource` poll loop. Grid-CO₂-intensity forecast ingestion is the one part of BL-17 still open (no free-tier provider evaluated) |
| WP5.4 | BASELINE reports — **done** (2026-08-11) | `BASELINE` report = heuristic forecast computed *as if no event* (M&V counterfactual), shipped via `openspec/changes/wp5-4-baseline-reports/` (now archived — see `docs/history/project_journal.md`'s WP5.4 entry). `reportDescriptor.historical` parsing and capacity-reservation reporting were already implemented before this work (found stale in the original plan). Not done: literal `LOAD_SHED_DELTA_AVAILABLE`/`GENERATION_DELTA_AVAILABLE` payload-type names (the existing `IMPORT_RESERVATION_CAPACITY`/`EXPORT_RESERVATION_CAPACITY` — corrected from the wrong word order fixed under GB-21 — cover similar ground under different names); a real statistical confidence model for data-quality metadata (deliberate non-goal, `DATA_QUALITY` currently tags provenance only) |
| exit | Validation demo | ~~Heuristic forecast beats last-known extrapolation on a held-out week~~ not attempted; **baseline reports quantify one event's impact** — done, verified live against Node1 production `ven-1` |

### 3.3 Comfort remainder (SG-5)

SG-5 is now done — R-18 (the last open item, EV `e_ev_extra` reward not driving real charging)
was fixed 2026-08-11. Two lower-priority, unblocked-independent items remain:

| Item | Content |
|------|---------|
| BL-35 | Notification producers for tier fallback / deadline-at-risk / packet abandoned (blocked on Stage-5 tier/SIMPLE-level-fallback machinery — not unblocked by BL-09, which shipped a lightweight per-solve constraint with no persisted tier state) |
| BL-27 / BL-18 | Control-mode metadata for UI sliders; instantaneous per-asset flexibility widget (scope decision first) |

### 3.4 Phase 6 — Planner fidelity & certification track

| Item | Content |
|------|---------|
| BL-11 | Time-weighted tariff averaging per slot (slot straddling a tariff boundary) |
| BL-13 | Early firm-up heuristic under flat rates |
| Cluster H | Transport modernisation: TLS 1.2+ (cert MUST), webhooks/subscriptions, optional MQTT, `/auth/server` discovery, mDNS, randomizeStart, gzip — tracked in `docs/BACKLOG_OpenADR_Cert.md` |

The standing decision holds: **lab-learning first** — transport work doesn't change
fleet dynamics at 30 s poll resolution; revisit when latency experiments or
certification become goals. Dependency audits are clean as of 2026-07-16
(BACKLOG.md §Dependency Vulnerabilities); re-run before any internet-exposed
deployment.

### 3.5 Continuous — hygiene & decision-shaped debt

BL-21/22/23/26/29 (vocabulary cleanup, wire-or-delete decisions), GB-04/05/07/09/11,
and the R-register — fold in opportunistically when touching the same files, per the
refactoring rule in `.claude/CLAUDE.md`.

---

## 4. Experiment scenarios & KPIs (for §3.1)

Scripted VTN drivers, each isolating one control method against the same fleet and
same simulated day (scenario YAMLs in `experiments/`):

| Scenario | VTN behaviour | Question answered |
|----------|---------------|-------------------|
| S-1 flat tariff | constant price, no events | baseline fleet behaviour |
| S-2 dynamic tariff | day-ahead PRICE curve | how much load shifts on price alone? |
| S-3 capacity limit | `IMPORT_CAPACITY_LIMIT` window at peak | do limits beat prices for peak shaving? |
| S-4 emergency | ALERT_GRID_EMERGENCY mid-day | shed depth + speed across fleet |
| S-5 direct dispatch | DISPATCH_SETPOINT to a subset | precision vs. side effects on comfort |
| S-6 combined | tariff + limit + one event | interaction / arbitration |

A future **S-7 capacity negotiation** (VEN *requests* capacity via
`OadrCapacityRequest`, BL-24 tail) gets added when a driving experiment exists.

**KPIs per scenario, per VEN and fleet-aggregate** (from the VEN history stores +
VTN recorder): total cost (€/day), peak import (kW), load factor, energy shifted
(kWh vs. S-1), comfort violations (deadline misses, temperature-band exits, unmet
SoC), event-compliance latency, report timeliness (`report_lag_s`), and report
accuracy (forecast vs. later actuals — the SG-3 usefulness metric).

**Report-usefulness evaluation (SG-3), concretely:** compare the operator's
*predicted* fleet response (USAGE_FORECAST / flexibility-envelope reports in the
recorder) against *actual* metered response (VEN history ground truth). Usefulness =
prediction error + coverage + timeliness. M&V-grade now that WP5.4's BASELINE reports ship
(2026-08-11) — not yet exercised on a fresh experiment run with a BASELINE reportDescriptor
configured in the scenario matrix itself (the WP5.4 exit demo used a standalone manual event,
not `experiments/run_experiment.py`'s scenario YAMLs).

---

## 5. Explicitly de-prioritised

- **OpenADR 3.0→3.1 migration** — distant goal; the spec copies in
  `docs/openadr_3_1_specs/` are 3.1, the implementation targets 3.0-era openleadr-rs.
- **Opt-in/opt-out signalling** — becomes interesting only when fleet *participation
  choice* is an experiment variable.
- **Capacity negotiation (`OadrCapacityRequest`)** — no driving experiment yet (S-7
  placeholder above).
- **Fleet scale N=10** — the Node1 resource budget caps practical fleet size; larger
  fleets need a second host or lighter VEN builds.
