# Opportunistic Objective & Fleet Forecast Diffing — Master Plan

> **Status:** concept — not yet broken into openspec tasks.
> **Motivation:** turn OpenADR Lab into an experimentation tool, not only a fleet that runs one
> configured planning objective. Today's fleet power chart (`VTN/ui` `FleetPowerChart.tsx`) shows
> *recorded* grid power — it already proves planning shifts load in time. It cannot show the
> counterfactual: what the fleet would have done under today's common, non-planning HEMS
> behaviour ("opportunistic": use PV surplus as soon as and as much as it's available, no
> look-ahead), under the *same* live conditions a planning objective is currently facing.

## Two experiment modes

- **Mode A — record and compare** (the naive version): flip every VEN in the fleet to an
  opportunistic objective, let it run a real day, record it, compare that recording against a
  day recorded under a planning objective. Cheap to get once M2 below exists, but weak evidence:
  no two days share weather/base-load/tariff, so the comparison is confounded, and it costs a
  full day per data point.
- **Mode B — forecast diffing** (the actual goal): each VEN already produces a forward-looking
  `Plan` from its planner; a fleet-level aggregation of those (already named "Portfolio position"
  in `docs/plans/fleet-monitor/vision.md`) would let a config-flip script switch every VEN's
  objective and show the *forecast* curve change immediately — same instant, same weather/tariff,
  two different re-plans. No confound, no day-long wait.

Mode B is the target; Mode A falls out of the same work for free at M2 and is useful as an early
sanity check.

## Constraints this plan must satisfy

- **`one-concept-one-function`**: "what does PV surplus get allocated to, in what order, up to
  what target" must have exactly one implementation, reused both for real-time dispatch and for
  producing the forecast curve. The trap to avoid is encoding the same greedy rule a second time
  as a MILP objective.
- **`asset-competence-assurance`**: each asset answers its own opportunistic capability (how much
  surplus it can absorb toward its own target); the arbiter never computes or assumes an asset's
  answer for it.
- **`ui-transparency` / `no-half-built-features`**: the forecast producer must emit the same
  `Plan`/`PlanTimeSlot` shape every other objective does, so `/plan`, `/forecast`,
  `/history/plans`, envelopes and the fleet-monitor views keep working unmodified — this isn't a
  parallel, second-class data path.

## Inventory — what already exists and is reused

| Concept | Lives in | Reused how |
|---|---|---|
| One opportunistic lever (EV only) | `controller/arbiter/arbiter_levers.rs::apply_ev_lever_opportunistic` | Template for the battery/heater levers (M1); not copied, generalized |
| Ranked lever allocation (deviation correction) | `controller/arbiter.rs::rank_levers`/`apply_ranked_levers` | Reused as the merit-order allocator for opportunistic surplus too (M1), instead of a second bespoke allocation loop |
| No-plan dispatch branch | `controller/arbiter.rs::reconcile` (`let Some(slot) = tick.plan_slot else { … }`) | Becomes the steady-state `Opportunistic` dispatch path (M2), not just the startup-window transient it is today |
| Runtime objective switch + replan trigger | `PUT /plan/objective` (`routes/hems/misc.rs`) | Fleet-wide flip script is a loop over this existing route — no new VEN route needed (M4) |
| Forecast inputs (PV, base-load, tariff) | `controller/milp_planner/inputs.rs::build_milp_inputs` → `MilpInputs` | Read as-is by the forecast-by-simulation producer (M3); no new forecast plumbing |
| `Plan`/`PlanTimeSlot` output shape | `entities/plan.rs`, produced by `controller/milp_planner::run_planner` | The one shared representation both the solver and the new simulation producer emit |
| Fleet-level aggregation groundwork | `docs/plans/fleet-monitor/vision.md` "Portfolio position", `phase-0-foundation.md` (BFF `TimeSeries::resample_uniform`, per-VEN series alignment) | Reused for aggregating per-VEN forecasts (M4), not re-derived |

Known gaps this plan must close, not inherit:

- Only EV has an opportunistic lever; battery and heater have none.
- `reconcile`'s no-plan branch currently skips the limit-enforcement pass entirely
  (`limit: None` unconditionally) — acceptable for a few-second startup window, not for a
  deliberate steady-state mode that must still obey grid-envelope and OpenADR event limits.
- No merit-order policy exists across opportunistic loads (battery vs. EV vs. heater) — this is a
  modeling decision, not implied by any code today.

## Milestones

### M0 — Groundwork (no user-visible behaviour change)

- Decide the merit-order policy across opportunistic loads, and each asset's opportunistic
  target (battery → full, heater → comfort-band ceiling, EV → existing `soc_target`). Declared as
  a policy table (`declare-dont-branch`), so a new asset kind is a new row.
- Fix the limit-pass gap: the limit-enforcement pass must run regardless of whether a plan target
  exists — one pass, unconditioned on which branch dispatched.
- Add `PlannerObjective::Opportunistic` as a marker (no behaviour yet).

### M1 — Per-asset opportunistic levers (real-time, single VEN)

*Depends on M0.*

- Battery and heater opportunistic levers, same shape as the EV template, each asset answering
  its own capability.
- Generalize surplus-claiming into the existing ranked-lever machinery (shared with deviation
  correction) instead of a second allocation loop.
- Unit tests per lever, plus a combined test proving merit order holds when surplus is scarce.

### M2 — Opportunistic as a real dispatch mode, single VEN, end-to-end

*Depends on M1.*

- Under `Opportunistic`, the (now limit-pass-fixed) no-plan branch becomes the steady-state path:
  always run the opportunistic levers + limit pass, never deviation correction.
- BDD scenario: PV surplus present → assets draw in merit order, capped at limits; no surplus →
  idle.
- Mode A (flip fleet, record a day, compare) already works end-to-end here, confounds and all —
  useful as an early sanity check.

### M3 — Opportunistic forecast producer (the mode-B piece)

*Depends on M1; reads the same forecast inputs `build_milp_inputs` already assembles.*

- Fork `run_planner` on objective: existing objectives keep solving via HiGHS; `Opportunistic`
  forward-simulates the M1 levers tick-by-tick over the same horizon and forecast inputs,
  packaging the result as the same `Plan`/`PlanTimeSlot` shape.
- Guard test: real-time dispatch (M2) and forecast simulation (M3) must behave identically
  tick-for-tick given the same synthetic inputs — otherwise they silently drift into two
  implementations of the same concept.
- `/plan`, `/forecast`, `/history/plans`, envelopes keep working unmodified.

### M4 — Fleet-level plumbing

*Can be scaffolded in parallel with M1–M3 (VTN/BFF side); needs M3's per-VEN output to be useful
end-to-end.*

- BFF aggregation of each VEN's plan/forecast into the "Portfolio position" view already named in
  `docs/plans/fleet-monitor/vision.md`, reusing `TimeSeries::resample_uniform` for grid alignment
  the same way the fleet power chart already does for telemetry.
- Fleet-wide flip script: a loop over `PUT /plan/objective` across the fleet's VENs — no new
  VEN-side surface needed.

### M5 — Observability / UI

*Depends on M4; required by `ui-transparency`, not deferred polish.*

- Fleet power chart gains a second overlaid series: forecast vs. recorded/actual, so an
  objective flip is visible as a curve change without a day's wait.
- Per-VEN diagnostics: active objective, and which lever currently leads the opportunistic
  allocation — extends the existing arbiter-decision trace pattern.

## Dependency sketch

```
M0 (policy + limit-pass fix + enum)
 └─▶ M1 (per-asset levers, shared ranking)
      ├─▶ M2 (real-time opportunistic dispatch) ─┐
      └─▶ M3 (forecast-by-simulation)            ├─▶ M4 (fleet aggregation + flip script) ─▶ M5 (UI)
                                                   ┘
```

## Open question

Should `Opportunistic` be a real `PlannerObjective` (this plan's assumption — it reuses
`PUT /plan/objective` and the `Plan` plumbing end-to-end for free, and is what makes the
fleet-wide flip script trivial), or a per-asset "charging strategy" toggle instead (only the EV
opportunistic, rest still planned)? Decide before M0 locks in.
