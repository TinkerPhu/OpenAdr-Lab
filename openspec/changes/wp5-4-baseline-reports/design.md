## Context

The VTN recorder archives reports but has no counterfactual to compare against — WP5.4's
last piece. Two report-generation paths already exist in `VEN/src/controller/reporter.rs`:
a timer-driven single-interval path (`build_measurement_report`, for events without
`reportDescriptors`) and an obligation-driven multi-interval path
(`build_measurement_report_for_obligation`, dispatched on `obligation.payload_type: String`
— already an open string, e.g. `"USAGE_FORECAST"`, `"IMPORT_CAPACITY_RESERVATION"`). Adding
`"BASELINE"` as a new payload type is additive to that same match statement — no new
plumbing needed to get a VTN's `reportDescriptor` requesting `payloadType: "BASELINE"`
routed to a new builder function.

The heuristic forecast (`AssetHeuristics::sample_kw(slot_t)`, WP5.2) already predicts
per-asset power independent of any event — it has no event-awareness at all, so it is
*already* the counterfactual by construction: there is no extra "subtract the event's
effect" step, the heuristic simply never knew about the event.

`ForecastSource` (`entities/design_vocabulary.rs`) already ranks forecast provenance
(`WeatherModel` > `Heuristic` > `LastKnown`/`None`) — this is the natural, already-built
proxy for "quality," not a new statistical model.

## Goals / Non-Goals

**Goals:**
- Submit a `BASELINE` report (heuristic counterfactual) alongside `USAGE` during an active
  event, archived by the (now-healthy) VTN recorder.
- `kpi.py` computes `event_impact_kwh` from archived BASELINE/USAGE pairs.
- Report payloads carry a coarse quality tag reusing `ForecastSource`.

**Non-Goals:**
- A new statistical confidence model (sample-count/variance) for `AssetHeuristics` — the
  original phase-5 plan's "confidence (sample count/variance)" language was aspirational,
  not built; `AssetHeuristics` today has no such fields. Building real statistical
  confidence is a separate, larger follow-up (would need to track per-bucket sample counts
  in `services::heuristics::learn_asset_heuristics`) — out of scope here.
- Reworking `LOAD_SHED_DELTA_AVAILABLE`/`GENERATION_DELTA_AVAILABLE` payload-type naming —
  the existing `IMPORT_CAPACITY_RESERVATION`/`EXPORT_CAPACITY_RESERVATION` reporting already
  covers this ground under different (also OpenADR-3-valid) names; renaming is a separate
  spec-compliance decision, not blocking BASELINE work.
- Historical-report replay and `reportDescriptor.historical` parsing — already implemented
  (R-15), untouched by this change.

## Decisions

**BASELINE = heuristic forecast, unconditionally.** Rather than computing "what the plan
would have been without this event" (which would require re-solving the MILP with the event
constraint removed — expensive, and re-introduces the exact planning-time cost this feature
is meant to avoid at report-submission time), BASELINE reuses the *already event-blind*
heuristic forecast (`AssetHeuristics::sample_kw`) that WP5.2 built for a different purpose
(the planner's own uncontrollable-load input). It never saw the event, so it needs no
adjustment to serve as the counterfactual. Alternative considered: a "no-event MILP re-solve"
baseline — rejected as unnecessary complexity for a first cut; can be revisited if the
heuristic proves too coarse (see Risks below, and the phase-5 plan's own noted risk about
simulated households being too regular).

**Reuse the obligation-driven reporter path, not the timer-driven one.** `BASELINE` only
makes sense when the VTN has explicitly asked for it via a `reportDescriptor`
(`payloadType: "BASELINE"`) — there's no sensible "auto" BASELINE the way timer-driven USAGE
reports fire for any active event. This keeps BASELINE symmetric with how
`USAGE_FORECAST`/`IMPORT_CAPACITY_RESERVATION` already work.

**Quality tag = `ForecastSource` string, not a new field type.** `BASELINE` (and, riding
along, forecast-type payloads generally) get an additional `QUALITY` (or similar)
`OadrReportPayload` entry carrying the `ForecastSource` variant name the underlying number
came from (e.g. `"HEURISTIC"`). Cheap, reuses an existing well-tested enum, honest about what
it actually represents (provenance tier, not a computed confidence interval).

**`event_impact_kwh` lives in `kpi.py`, not the VEN.** The VEN's job stops at submitting
BASELINE+USAGE; the subtraction and interpretation belongs to the evaluator (the recorder
already has both series once BASELINE reports land), matching how `energy_shifted_kwh`
already works there (baseline-run comparison, `kpi.py`'s existing `--baseline` flag) —
`event_impact_kwh` is the intra-run twin of that inter-run computation.

## Risks / Trade-offs

- **[Risk] Simulated households may be too regular** → the heuristic-baseline counterfactual
  could look artificially accurate against equally-regular simulated actuals, overstating
  M&V confidence. **Mitigation**: note explicitly in the WP5.4 exit write-up; consider
  stochastic base-load noise in the simulator as a follow-up (record as a new BACKLOG item
  if adopted, not built here).
- **[Risk] Heuristic quality varies a lot by asset maturity** (a freshly-provisioned VEN's
  heuristic is near-flat/uninformative) → BASELINE reports from a young VEN will be a poor
  counterfactual. **Mitigation**: the `ForecastSource`/quality tag surfaces this
  (`Heuristic` still shown even when thin) rather than hiding it; real confidence scoring
  is the explicit non-goal follow-up above.
- **[Trade-off] No MILP re-solve baseline** means BASELINE can't capture "the plan would
  have shifted this load anyway for cost reasons, event or not" — it's a pure
  uncontrollable-load counterfactual, not a full economic counterfactual. Accepted for this
  change's scope; flagged in Non-Goals.

## Migration Plan

Additive only — no schema migration, no breaking change to existing report types. Deploy
alongside the next VEN release; VTNs that never request `payloadType: "BASELINE"` see no
behavior change at all. Rollback = revert the VEN binary; no data cleanup needed (recorder
rows are additive and self-describing by `report_type`).

## Open Questions

- Should quality tags ride on *all* forecast-type payloads (`USAGE_FORECAST`, capacity
  reservations) in this change, or BASELINE only for a first cut? Leaning BASELINE-only
  initially (smaller diff, proves the pattern) with the others as an easy follow-up once
  proven — confirm during task breakdown.
