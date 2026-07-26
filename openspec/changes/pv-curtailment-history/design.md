## Context

Three gaps, found in that order while scoping this change:

1. The Controller page's PV graph (`AssetTimelineChart.tsx` via `GET /timeline/pv`) has no visual
   distinction between normal, capped, and curtailed periods, past or future.
2. A first draft proposed storing a simulator-only "potential vs. actual" delta (`curtailed_kw`).
   Rejected: real inverters under a curtailment command report actual output and the commanded
   limit, never "what would have been produced" — that's a model, not a measurement, and
   persisting it as ground truth wouldn't generalize past the simulator.
3. Distinguishing "a limit is active" from "a limit is actually reducing output" requires knowing
   the inverter's own ceiling — and PV profiles today only carry `rated_kw` (installed DC panel
   peak). Real installations routinely run an inverter rated below panel peak (deliberate DC/AC
   oversizing) — a `rated_kw`-based limit check would misclassify normal hardware clipping as
   imposed curtailment.

## Goals / Non-Goals

**Goals:**
- Model the inverter's true AC output ceiling as distinct from installed panel DC peak.
- Distinguish, per tick: no curtailment / hardware-capped (informational) / imposed curtailment
  (actionable), and for imposed curtailment, planned vs. unplanned.
- Persist only measurable facts (the applied limit and its source) — no derived/modeled delta.
- Render all three states on the Controller page's PV timeline, reusing its existing shading
  mechanism.

**Non-Goals:**
- No plan-snapshot persistence or retrospective plan-vs-actual cross-reference — the planned/
  unplanned distinction is resolved live, at the moment `resolve_pv_export_limit_kw` runs, so
  there's nothing to reconstruct after the fact.
- No profile validation constraining `inverter_max_kw` relative to `rated_kw` — both over- and
  under-sizing the inverter relative to panel peak are legitimate real configurations. (The spec
  does require `inverter_max_kw > 0`, a basic sanity check, not a relative constraint.)
- No UI change to `PlanPowerStack.tsx` beyond what `pv-export-curtailment` already shipped.
- No change to `pv-export-curtailment`'s decision variable, tie-break, or the fact that
  `resolve_pv_export_limit_kw` returns the tighter of two sources — only its return type gains the
  source tag.

## Decisions

**1. Persist facts (limit value + source), never a derived potential-output delta.**
Rejected the original `curtailed_kw` design outright (see Context #2). `export_limit_kw` (what the
VEN commanded) and which source produced it (plan vs. capacity) are both things the VEN always
knows with certainty, in simulation or on real hardware. Whether that limit is currently *binding*
is derived at render time from already-stored facts (`power_kw` vs. `export_limit_kw` vs.
`inverter_max_kw`) — arithmetic on stored numbers, not a re-derivation of a physical model, so no
duplicated modeling logic between Rust and the UI.

**2. `inverter_max_kw` is a physical clamp, not just a display threshold.**
Modeling it only as a number to compare against in the UI would be inconsistent: the simulator
would still let PV "produce" more than the inverter could actually deliver. Instead, `step_inner`
(and the forecast/MILP-input functions — `forecast_kw_at`, `capability_trajectory`,
`build_milp_context`, which all currently compute `rated_kw × irradiance` unclamped) apply
`.min(inverter_max_kw)` to the DC-potential term before any commanded limit. Default
(`inverter_max_kw == rated_kw`) makes this a no-op for every profile that doesn't opt in — the DC
potential can never exceed `rated_kw` anyway (`irradiance ∈ [0,1]`), so clamping to a value equal
to `rated_kw` never binds.

Alternative considered: leave the planner's forecast unclamped and only apply the hardware ceiling
in the live simulator. Rejected — the planner would then "see" and plan around DC potential that
physically can never be delivered, producing wrong plans specifically in the oversized-panel case
this change exists to model correctly.

**3. `export_limit_kw` and the source tag move from `PvInverter` (config) to `PvState` (per-tick state).**
`state_values()` is called both live (`self` = current config) and historically
(`cfg.state_values(&p.state)` for each point in the ~1h `AssetHistoryBuffer`, per
`to_timeline_snapshot()`). Today `export_limit_kw` is read from `self`, so a *historical* point
reconstructed later reports the *current* limit, not what was active at that past tick — already
wrong before this change, just never surfaced because nothing rendered it. Storing both fields on
`PvState` (set once per tick in `step_inner`/the tick-override path) and reading `state.*` instead
of `self.*` in `state_values()` fixes this and is required for the past-window chart to be
accurate at all.

**4. Planned vs. unplanned is tagged live, not reconstructed.**
`resolve_pv_export_limit_kw` (`controller/dispatcher.rs`) already computes both the plan-derived
limit and the live-capacity-derived limit and picks the tighter one (`(Some(a), Some(b)) =>
Some(a.max(b))`). Returning which side won *at that moment* — the plan's own target, or a live
capacity source it didn't originally account for — gives exactly "was this planned" without ever
persisting or later comparing past `Plan` objects. This directly supersedes the retrospective
`plan_snapshots`-cross-reference design from the previous draft: same conceptual answer, computed
once, synchronously, where the information already exists.

Alternative considered (previous draft): persist every adopted plan and retrospectively compare a
past curtailed tick's plan-in-effect against actual behavior. Rejected as more machinery for the
same answer, and it introduced a second observability question (accuracy of “which plan was in
effect”) the live-tag approach doesn't have.

**5. Only two new history columns; `inverter_max_kw` is not sampled per tick.**
`inverter_max_kw` is a static profile parameter — it doesn't change at runtime, so storing it in
every `tick_samples` row would be pure duplication. It's exposed live via `/sim`'s `state_values()`
(same as `rated_kw` today) and used by the UI as a constant reference for the whole chart window;
only `export_limit_kw` (varies tick to tick) and the source tag need per-tick persistence.

**6. Three chart states, not two.**
"Hardware-capped" (output pinned at `inverter_max_kw` with no tighter commanded limit involved) is
informationally distinct from "imposed curtailment" (a commanded limit below `inverter_max_kw`
that's actually binding) — the former is an inherent hardware fact nothing can change tick to
tick, the latter is the actionable, alertable case. Shading both the same way would bury the
alertable signal in noise from ordinary DC/AC clipping on oversized systems.

**7. Window aggregation prioritizes capacity > plan > none, not "most recent" or a mean.**
`tick_samples` downsamples 1s ticks into a 1-minute mean — appropriate for continuous quantities
like `soc_pct`, meaningless for a categorical source tag. A "most recent value in the window" rule
would silently drop a live VTN/capacity event that fires and clears within the same minute as an
otherwise plan-sourced (or unlimited) window — exactly the brief, alertable case this feature
exists to surface. Instead, the window's persisted source is the highest-priority source observed
at any point in the window (capacity > plan > none), and the persisted limit value is the tightest
value observed for that category. This guarantees a brief unplanned event is never masked by
surrounding plan-sourced or unlimited samples.

Alternative considered: last-value-wins (simplest to implement, matches how some other per-asset
fields behave). Rejected — it would make the unplanned signal's visibility depend on exact timing
within the sample window, which defeats the purpose of recording it at all.

## Risks / Trade-offs

- [`state_values()`'s behavior for historical points changes for `export_limit_kw`] → This is a
  bugfix, not a new risk: today's behavior (showing the current limit on past points) is already
  wrong; no code depends on that inaccuracy.
- [Moving fields onto `PvState` means every `PvState`/`PvInverter` literal-construction test site
  needs updating] → Same category of change as `pv-export-curtailment`'s `temp_safety_max_c`
  addition; found exhaustively via `grep -rn "PvState {" "PvInverter {"`, same audit approach.
- [Clamping DC potential in the forecast/MILP-input functions touches the planner's PV input] →
  Bounded by the default (`inverter_max_kw == rated_kw`, a no-op); only changes behavior for
  profiles that explicitly configure a lower inverter capability, which is exactly the case this
  change is meant to model correctly.

## Open Questions

None blocking.
