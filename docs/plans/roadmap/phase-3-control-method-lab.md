# Phase 3 — Control-Method Lab

> **Goal:** every VTN control knob (priority events, alerts, SIMPLE shed, capacity
> reservations, direct setpoints) is honoured per spec, the VEN reports forecast +
> flexibility back, and a scripted experiment harness compares the methods on KPIs.
> This is the phase that delivers the "next big goal" (SG-1 + SG-2 demonstrated).
> **Items:** BL-04, UC:SIMPLE, UC:§8.10, BL-05, BL-10, BL-15 + UC:§8.8,
> UC:device-status, BL-06 + BL-24, NEW A-3 (KPI harness).
> **Prerequisites:** Phases 1–2 (history stores + fleet). BL-02 (priority) from Phase 0.
> **Exit demonstration:** one generated experiment report (markdown + charts)
> comparing scenario days S-1…S-6 across a 10-VEN fleet.
> **Total effort:** ~4–5 weeks. Two tracks can run in parallel: Track A (VEN signal
> handling, WP3.1–3.4) and Track B (reporting out, WP3.5–3.7); WP3.8 last.

## Track A — inbound signals

### WP3.1 — BL-04: ALERT_GRID_EMERGENCY / ALERT_BLACK_START (M)

1. BDD first: `alerts.feature` — send ALERT_GRID_EMERGENCY for a 30-min window,
   assert grid import drops to ≈ 0 within one poll cycle and recovers after.
2. Parse ALERT payload types in `openadr_interface` → emit `PlanTrigger::Alert`
   (enum variant already exists) with the alert window.
3. Planner: alert window slots become highest-priority FIRM with a hard
   import ≤ ε constraint (battery may still cover local load; export allowed for
   BLACK_START per its semantics — check User Guide §8.1 wording before coding).
4. Unit tests at planner level with the mock solver context: alert slots present →
   constraint rows emitted; overlapping user deadline → deadline yields (alert wins).

### WP3.2 — SIMPLE levels 0–3 (S–M)

1. Mapping decision (record in the openspec design doc): level 0 = normal, 1 = mild
   (cap import at X % of contractual limit), 2 = moderate (defer all FLEXIBLE +
   OPPORTUNISTIC), 3 = severe (as WP3.1 shed). Percentages profile-configurable
   (`simple_levels:` map) so experiments can vary them.
2. Parse `SIMPLE` payload → tariff-independent constraint input to the planner,
   reusing the WP3.1 constraint path for level 3.
3. Unit test per level; one BDD scenario stepping 0→2→0.

### WP3.3 — UC:§8.10: capacity reservations constrain the solver (M)

1. `OadrCapacityState` already carries `import_subscription_kw` /
   `import_reservation_kw` (parsed, shown at `GET /capacity` — currently ignored by
   planning). Feed them into the MILP's contractual-limit constraint inputs alongside
   `*_CAPACITY_LIMIT`.
2. Add export-side subscription/reservation parsing (currently unhandled).
3. Unit tests: reservation tighter than contractual limit → binds; looser → inactive.
   BDD: publish IMPORT_CAPACITY_RESERVATION 3 kW over peak hour → fleet VEN's plan
   respects 3 kW in that window.

### WP3.4 — BL-06 + BL-24: direct setpoints (M–L)

1. `DISPATCH_SETPOINT`: parse → store in `OadrEventCache.dispatch_setpoints`
   (BL-24's cache, field exists in `entities/capacity.rs`) → dispatcher override mode:
   while an active dispatch window exists, dispatcher applies the commanded site
   setpoint directly, bypassing the plan (planner keeps running; on window end,
   normal plan resumes). Emit a trace event for observability.
2. `CHARGE_STATE_SETPOINT`: parse → create/update an `EvSession` targeting the given
   SoC via the existing `user_request` machinery.
3. BDD per BL-06's verify clause: sim setpoint matches within one poll cycle;
   `EvSession` created with correct target SoC. Unit tests for override
   precedence: dispatch override > alert shed? **Decision needed** — suggest alert
   wins (safety over instruction); record in design doc.

## Track B — outbound reporting

### WP3.5 — BL-05: obligation-triggered report submission (S–M)

Wire `due_obligations(now)` (checked but currently only marked fulfilled) to
`build_measurement_reports_for_active_events()` + `upsert_report()` before marking
fulfilled. BDD: event with short-interval `reportDescriptor` → report lands at
`due_at`, not at the next timer tick. (Obligations already re-arm correctly per the
wiki audit — only the *submission* wiring is missing.)

### WP3.6 — BL-10 + BL-15 + UC:§8.8 + UC:device-status: forecast & flexibility out (L)

1. BL-15: after each plan cycle, build `AssetForecast` (documented shape in
   `entities/design_vocabulary.rs` §3.6) from the planner's `planned_state_by_asset`;
   tag `ForecastSource::Optimization`. Route `GET /forecast`. Unit test: forecast
   power matches planner state for same asset/horizon.
2. UC:§8.8: build `USAGE_FORECAST` report payload from `AssetForecast` (site
   aggregate), submit on each new plan when an event requests it (else on the report
   timer as unsolicited telemetry — check what the VTN accepts; fall back to
   descriptor-driven only).
3. BL-10: when a plan yields non-empty `FlexibilityEnvelope`s, build
   `IMPORT_CAPACITY_RESERVATION` / `EXPORT_CAPACITY_RESERVATION` report payloads and
   submit. BDD: FLEXIBLE packets exist → VTN receives envelope report with matching
   power/energy values.
4. UC:device-status: replace hardcoded `"ACTIVE"` `OPERATING_STATE` in `reporter.rs`
   with real per-asset state (derive from asset availability/fault state; the
   `DeviceResponsiveness` vocabulary is the reference). Small, do alongside.

### WP3.7 — Recorder KPI columns (S)

Extend the Phase-1 recorder to also archive the new report types and compute
`report_lag_s` (due_at → received_at) per report — the SG-3 timeliness metric.

## WP3.8 — NEW A-3: experiment harness + KPI jobs (L)

1. New top-level `experiments/` directory (Python, consistent with E2E tooling):
   - `scenarios/s1_flat.yaml … s6_combined.yaml` — declarative: tariff series,
     capacity events, alerts, dispatch actions with relative timestamps.
   - `run_experiment.py` — resets state (GB-06), brings fleet up (`fleet.sh`),
     drives the VTN API per scenario (sim-time aware), waits out the scenario
     window, then snapshots both stores.
   - `kpi.py` — SQL against VEN SQLite files + `lab_recorder`:
     cost €/day, peak import kW, load factor, energy shifted vs. S-1 baseline kWh,
     comfort violations (deadline misses, temp-band exits, unmet SoC), compliance
     latency (signal → measurable response), report timeliness + forecast accuracy
     (USAGE_FORECAST vs. later actuals — the SG-3 usefulness metric).
   - `report.py` — renders one markdown report per run into
     `experiments/results/<date>-<scenario>/` (charts via matplotlib PNGs).
2. Scenario clock: experiments run against sim time — verify the simulator's
   time-acceleration hooks are drivable per-VEN from outside (spike first; if only
   wall-clock is supported, scenarios run in real time and S-1…S-6 become 6 short
   windows in one day rather than 6 full days).
3. Determinism: fleet seed + scenario file fully determine a run; two runs with the
   same seed produce KPIs within noise tolerance (assert in a harness self-test).
4. First real run = the phase exit demonstration; commit the generated report as the
   reference example.

## Order, decisions, risks

```
Track A: WP3.1 → WP3.2 → WP3.3 → WP3.4     (planner constraint paths build on each other)
Track B: WP3.5 → WP3.6 → WP3.7             (parallel to Track A)
WP3.8 after both tracks.
```

Open decisions to settle at openspec-proposal time: alert-vs-dispatch precedence
(WP3.4), SIMPLE level percentages (WP3.2), unsolicited USAGE_FORECAST vs.
descriptor-driven only (WP3.6), sim-time drivability (WP3.8 spike).

Risks: (a) planner constraint interactions (alert + reservation + user deadline) can
go infeasible — `PlanInfeasible` from Phase 2 WP2.3 plus a documented relaxation
order (drop OPPORTUNISTIC first, comfort next, never alert) is the guard; test the
triple-overlap case explicitly; (b) experiment wall-time if sim acceleration isn't
externally drivable — the spike in WP3.8 step 2 de-risks the estimate.

Bookkeeping: mark BL-04/05/06/10/15/24(cache part) resolved; cert rows §5/§6/§8.x
update; register A-3 as BL-33; journal + `/wiki-sync` ([[milp-planner]],
[[dispatcher]], [[openadr-interface]], new experiments page); resolve the wiki
OPEN QUESTION (arbitration-first confirmed by execution).
