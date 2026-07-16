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
| **SG-1 Fleet** | Run a fleet of independent, *diverse* VEN agents against one VTN | **Built.** `fleet.sh up N` (bulk registration, personas, health checks); resource budget on the Pi4 caps practical size at N=3 base + fleet VENs |
| **SG-2 Control-method lab** | Observe and compare VTN control methods (tariffs, limits, alerts, SIMPLE, dispatch) | **Built, not yet exercised.** All control paths implemented and BDD-covered; the experiment harness exists — the first full S-1…S-6 comparison run is still pending (see §3.1) |
| **SG-3 Report evaluation** | Judge the *usefulness* of VEN reports from the VTN side | **Directional only.** VTN recorder archives reports incl. `report_lag_s`; rigorous M&V-grade evaluation needs BASELINE reports (WP5.4, §3.2) |
| **SG-4 Forecast from history** | VEN learns heuristics from its own past data | **Half done.** History store + learned weekday/weekend heuristics ship and feed the planner; external weather/CO₂ feeds (WP5.3) and the held-out-week validation demo remain |
| **SG-5 Client comfort** | The resident's experience is first-class | **Mostly done.** Request modes, comfort-curve overrides, notifications, History UI ship — with one substantive gap: comfort curves never reach the MILP constraints (BL-34) |

SG-1–SG-3 are the **VTN-side benefit** axis; SG-4–SG-5 the **client comfort** axis.

---

## 2. Where the open items live

- `docs/BACKLOG.md` — feature gaps: BL-09, BL-11, BL-13, BL-17, BL-18, BL-21…BL-27,
  BL-29, BL-34, BL-35; general items GB-04, GB-05, GB-07, GB-09, GB-11.
- `docs/BACKLOG_OpenADR_Cert.md` — certification/transport line items (Cluster H).
- `docs/reference/TECHNICAL_DEBTS.md` — the debt register (R-18…R-40).
- `docs/plans/refactoring_backlog.md` — R-08 (AssetConfig enum→trait).
- `docs/plans/roadmap/` — the per-phase implementation plans (phases 0–4 executed;
  phase 5 partially; phase 6 not started).

---

## 3. Remaining work, priority order

### 3.1 The experiment windows (highest value, zero new code)

The whole SG-1/SG-2 arc converges on demonstrations that have not run yet — they are
scheduled-time items (scenarios run in real time; the full set is ~3 h on the Pi4):

1. **S-1…S-6 control-method comparison** (Phase 3 exit): one report comparing flat
   tariff vs. dynamic tariff vs. capacity limit vs. emergency alert vs. direct
   dispatch vs. combined, on the same fleet and day. Harness: `experiments/`
   (`run_experiment.py` → `kpi.py` → `report.py`); only the 3-minute smoke scenario
   has been exercised end-to-end.
2. **Persona re-run** (Phase 4 exit): the same scenarios with eco-optimizer /
   comfort-first / commuter personas (`fleet.sh --personas`) — expected to show
   measurably different fleet response; KPI segmentation support exists.

Scenario matrix and KPI definitions: §4 below.

### 3.2 Phase 5 remainder — Forecast & Baseline (SG-3/SG-4 rigor)

| WP | Item | Content |
|----|------|---------|
| WP5.3 | BL-17 `ExternalDataSource` | Weather/irradiation/CO₂-forecast ingestion (Open-Meteo sketched), cached with `ExternalDataFetchStatus`, feeding `ForecastSource::WeatherModel` forecasts — PV forecasting from physics + weather instead of history alone |
| WP5.4 | BASELINE + capability forecast | `BASELINE` report = heuristic forecast computed *as if no event* (M&V counterfactual); parse `reportDescriptor.historical`; `LOAD_SHED_DELTA_AVAILABLE`/`GENERATION_DELTA_AVAILABLE` payloads; data-quality metadata. **This is what upgrades SG-3 from directional to rigorous** |
| exit | Validation demo | Heuristic forecast beats last-known extrapolation on a held-out week; baseline reports quantify one event's impact |

Plan: `docs/plans/roadmap/phase-5-forecast-and-baseline.md` (WP5.1/WP5.2 are done).

### 3.3 Comfort remainder (SG-5)

| Item | Content |
|------|---------|
| BL-34 | Translate comfort curves (default or user-override) into MILP tier constraints — today the resolved curve is dropped before the solver, so it influences nothing |
| BL-35 | Notification producers for tier fallback / deadline-at-risk / packet abandoned (blocked on BL-09's tier machinery) |
| BL-27 / BL-18 | Control-mode metadata for UI sliders; instantaneous per-asset flexibility widget (scope decision first) |

### 3.4 Phase 6 — Planner fidelity & certification track

| Item | Content |
|------|---------|
| BL-11 | Time-weighted tariff averaging per slot (slot straddling a tariff boundary) |
| BL-09 | Penalty-threshold check (peak-demand penalties) — also unblocks BL-35 |
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
prediction error + coverage + timeliness. Directional until WP5.4's baselines;
M&V-grade after.

---

## 5. Explicitly de-prioritised

- **OpenADR 3.0→3.1 migration** — distant goal; the spec copies in
  `docs/openadr_3_1_specs/` are 3.1, the implementation targets 3.0-era openleadr-rs.
- **Opt-in/opt-out signalling** — becomes interesting only when fleet *participation
  choice* is an experiment variable.
- **Capacity negotiation (`OadrCapacityRequest`)** — no driving experiment yet (S-7
  placeholder above).
- **Fleet scale N=10** — the Pi4 resource budget caps practical fleet size; larger
  fleets need a second host or lighter VEN builds.
