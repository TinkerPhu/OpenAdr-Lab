# Design: Capacity-Forecast / Site-Headroom Consistency & Unification

Status: **analysis complete, no implementation started**. This document is a
snapshot of a design discussion, preserved so the reasoning isn't lost before
any of it is acted on. Nothing in `VEN/src/controller/capacity_forecast.rs`
or `VEN/src/controller/envelope_forecast.rs` has been changed as a result of
this discussion yet.

## Why this document exists

A UI request ("move the Capacity Forecast curve pair onto Controller/History,
add a kWh axis, darken the Site Headroom band") led to re-examining what the
Capacity Forecast curve actually represents. That inspection surfaced several
real inconsistencies in how PV (and by the same reasoning, the heater) are
modeled, and a proposal to unify `capacity_forecast.rs` and
`envelope_forecast.rs` around one shared engine. The original UI request is
**parked** pending resolution of what's below — changing the chart's contrast
or axis before its underlying numbers are trustworthy would just make a wrong
curve easier to read.

## Current state: two independently-implemented curves

| | `capacity_forecast.rs` | `envelope_forecast.rs` |
|---|---|---|
| Question answered | "If we abandoned the plan now and committed to a sustained extreme (max import / max export), how does achievable power decay over elapsed time as reservoirs deplete?" | "How much further could each asset move from wherever the plan already put it, at this one instant?" |
| Quantity type | **Absolute** achievable power (kW), from a zero baseline | **Relative** delta from `planned_kw` |
| State source | Live `SimSnapshot` (raw current SoC/temp) for battery/EV/heater; `AssetForecastFrame` for PV only | `AssetForecastFrame`, built by re-simulating every asset forward along the plan's own scheduled trajectory |
| Time axis | `elapsed_s` from a `start` instant (today always `now`) | Plan slot timestamps (`ts`), one independent point-in-time counterfactual per slot |
| Battery/EV/heater formula | `reservoir_events`: constant power until an energy budget exhausts, then 0 | `up = planned_kw - cap_max_export_kw`, `down = cap_max_import_kw - planned_kw` |
| Used by | `CapacityForecastChart` (Diagnostics page) | `SiteHeadroomChart` (Controller, History) |

These two curves are **not guaranteed to agree** even at the one instant
where they overlap conceptually (`capacity_forecast` at `elapsed_s = 0` vs.
`envelope_forecast`'s first slot) — they're computing genuinely different
kinds of numbers (absolute vs. relative), and only coincide by accident when
`planned_kw = 0` for every asset at that instant.

## Findings

### 1. PV's Import-direction term double-credits curtailment (root-caused, fix agreed)

`pv_events`' Import branch added `(-planned_kw).max(0.0)` — PV's
currently-planned/forecast output — as a **positive** contribution to
achievable import, framed as "curtailment headroom." This is wrong: every
other flexible asset in this module commits to its own physical extreme for
the requested direction (battery charges at max, heater heats at max); PV's
extreme for "maximize import" is **curtailed to zero**. Once curtailed, PV
isn't subtracting anything (nothing is flowing) and isn't adding anything
either (curtailing a generator doesn't create new consumption capacity) — it
should contribute **nothing**, i.e. be entirely absent from the Import event
list, not a signed term of any kind.

**Consequence of the bug**: achievable-import numbers are currently
understated whenever PV is generating, because the code was subtracting
concept (a `-planned_kw` "netting" framing was floated and rejected during
this discussion) or adding an already-implicitly-available amount, rather
than simply excluding PV.

**Agreed fix**: `pv_events` returns no events at all for
`CommitmentDirection::Import`. PV keeps contributing to Export exactly as
today (`+cap_max_export_kw`, the weather ceiling — this side was always
correct).

**Also fix**: the module's own header comment ("PV's ceiling is driven by
the weather forecast, not by anything the plan decided") should be corrected
— `planned_kw` (used until this fix) was plan-derived, not weather-derived;
once the Import branch is removed the claim becomes trivially true for what
remains (`cap_max_export_kw` only), but the comment should say so plainly
rather than asserting something that was only half true.

### 2. Heater's Export-direction term likely has the identical bug (not yet fixed)

`heater_events`' Export branch adds the heater's **current draw**
(`asset.power_kw`) as a positive contribution to achievable export, on the
same "current consumption is curtailable, credit it" reasoning that was just
rejected for PV. By the same argument: the heater's extreme for "maximize
export" is **turned off**, and turning off a consumer doesn't manufacture new
exportable generation beyond what battery/PV can already deliver — it should
contribute **nothing**, not `+asset.power_kw`.

This has **not been walked through with the same rigor as PV yet** — no
worked numeric example, no test review, no confirmation this is actually
wrong the way PV was. It is flagged here as the most likely next concrete
fix, not as a settled conclusion.

### 3. `envelope_forecast`'s PV special case is a harmless no-op (found, not urgent)

`compute_headroom_forecast`'s PV branch (lines ~38–50) hand-writes
`down_kw += (-planned_kw).max(0.0)`, commented as "bounded by PV's own
output, not by `cap_max_import_kw` (always 0)". Since `cap_max_import_kw`
*is* always `0.0` for PV, this is arithmetically identical to the generic
formula (`cap_max_import_kw - planned_kw`) it was special-cased to avoid.
No behavior bug — just dead special-casing that could be deleted for
clarity whenever someone is next in this function.

### 4. EV `soc_target` bound — checked, no bug found

Verified `EvCharger::capability_inner` (`VEN/src/assets/ev.rs`) already
reports `max_import_kw: 0.0` once `state.soc >= self.soc_target`, so
`envelope_forecast` (which consumes this field via `AssetForecastPoint`) is
already correctly bounded by `soc_target`, consistent with
`capacity_forecast`'s own independent `ev_events` reimplementation of the
same bound. No divergence between the two modules here.

## The proposed unification: a two-argument engine

Proposal on the table: replace both bespoke computations with one function
of two arguments:

```
capacity_at(at: DateTime<Utc>, offset: Duration) -> (achievable_import_kw, achievable_export_kw)
```

- **Site Headroom** = sweep `at` across the plan's slot timestamps, `offset`
  fixed at `0`.
- **Capacity Forecast** = fix `at = now`, sweep `offset` from `0` to `48h`.
- **New capability, not available today**: fix `at` at *any future* plan
  instant and sweep `offset` from there — "if the plan runs as intended
  until 3pm, and then we abandoned it for a sustained extreme commitment,
  how does achievable power decay from there?" This only becomes possible
  once there's a shared way to resolve asset state at an arbitrary future
  `at`, which today only `envelope_forecast`'s forecast-frame machinery can
  do; `capacity_forecast` currently punts on it entirely (see its own doc
  comment on `start`, admitting a "first-order approximation").

**Important correction to the proposal, from the discussion**: this is not
literally one formula swept along two independent axes. `at` and `offset`
obey **different state-evolution rules**:

- Advancing `at` means evolving asset state **along the plan's own chosen
  schedule** (what `insert_simulated_points`/`simulate_forward` already do).
- Advancing `offset` means evolving asset state **along a sustained extreme
  commitment that ignores the plan** (the closed-form reservoir math already
  in `capacity_forecast.rs`).

What's actually shareable is narrower and more honest than "one function":
a **starting-state resolver** (given `at`, return each asset's forecasted
SoC/temperature — live snapshot when `at = now`, plan-forecasted otherwise)
feeding into a **single absolute extreme-commitment engine** (given a
starting state and `offset`, return achievable power — today's
`reservoir_events` family, corrected per Findings 1–2). Two pieces, not one
formula, but they compose exactly the way the two-argument proposal wants.

## Open points and recommendations

| # | Open point | Recommendation |
|---|---|---|
| 1 | Heater's Export term likely has the same bug as PV's Import term (Finding 2) | Fix it the same way, immediately after this document lands: no event when heater's extreme-for-export (off) contributes nothing; add a worked numeric example and a regression test mirroring `pv_export_uses_ceiling_not_ceiling_minus_current` before changing the implementation (test-first per repo convention) |
| 2 | Shiftable-load "up"-flex (`envelope_forecast::shiftable_up_kw`) has no absolute-framework equivalent — it's inherently "the plan chose to run this now; it could be deferred," which a plan-independent engine can't express | Do not fold it into the unified absolute engine. Keep it as a separate, explicitly-relative addendum computed alongside the unified core for Site Headroom only; Capacity Forecast has no analogous concept and shouldn't gain one |
| 3 | Shiftable-load placement in `capacity_forecast` is greedy (earliest feasible start), not MILP-optimal — flagged as a risk in the original (now-deleted, recovered from git history) `flexibility-capacity-forecast/design.md` and never revisited | Keep as a documented, accepted approximation unless/until it causes a visible user-facing disagreement with the actual plan; not worth a MILP re-solve just for this diagnostic curve |
| 4 | Heater's reservoir math ignores ongoing thermal loss and the forced-on floor re-engaging at `temp_min_c` — same original design doc, "optimistic" by construction | Same disposition as #3: documented, accepted simplification; revisit only if real-world curves are observed to overstate heater import capacity in a way that misleads a user decision |
| 5 | Unifying the two engines forces Site Headroom from a **relative** metric (today) to an **absolute** one (if it becomes `capacity_at(at, 0)`) | This is a product decision, not a refactor side-effect. Recommendation: make the call explicitly before writing any unification code — see below |
| 6 | `SiteHeadroomChart` renders headroom as a **band around the live grid-power line** (`gridPowerKw ± upKw/downKw`), which only makes visual sense for a relative delta | If #5 is resolved toward "absolute," this chart's rendering needs rework too — an absolute achievable total isn't naturally centered on the current live reading and could visually sit on the "wrong side" of the current-power line. Treat this as in-scope of the same change, not a follow-up |
| 7 | A future-anchored `at` (new capability) needs a UI/API control to pick it — otherwise it's a backend capability with no consumer, which this repo's own `no-half-built-features` convention treats as an incomplete delivery | Scope the anchor-time picker (a slider/scrubber on the Capacity Forecast page, most likely) as part of the same piece of work that adds future-`at` support, not a deferred follow-up |
| 8 | Forecast-anchored curves for future `at` are a forecast of a forecast — invalidated by the next replan, and degraded further out by the planner's coarser far-horizon zones and PV weather uncertainty | Surface this caveat directly in the UI wherever a non-`now` anchor is shown (e.g. "assumes the current plan holds until this point — will change if the plan is recomputed"), not just in code comments |
| 9 | The pre-existing `soc_trajectory_kwh` / `planned_state_by_asset` gap — real, already-computed planned SoC-over-time data, never surfaced in the VEN UI at all — is a *different* signal from anything in this document (it shows what the plan actually intends, not a hypothetical extreme) | Track as its own `ui-transparency` gap (per `docs/reference/KEY_LEARNINGS.md` conventions), not as part of this unification. Do not conflate "show the plan's real trajectory" with "show a hypothetical extreme-commitment curve" |
| 10 | The original, paused UI request (second y-axis + kWh curve pair on Controller/History, higher-contrast Site Headroom band) | Stays paused. Resume only after #1 (heater fix) and #5/#6 (relative-vs-absolute decision) are settled — otherwise the UI work would be polishing numbers that are still known to be wrong or about to change meaning |
| 11 | The "accumulated charge / capacity-headroom kWh curve" idea (the original critical-observation "c" that kicked off this whole investigation) | Still just an idea. Now clearly dependent on #1 (heater) and #5 (relative-vs-absolute) being settled first, since an accumulated-energy curve built on top of currently-wrong per-asset terms would just compound the error into a new, harder-to-audit derived series |

## Suggested sequencing

1. Fix Finding 2 (heater Export), test-first, mirroring the PV fix — small,
   self-contained, no design decision required.
2. Decide open point #5 (relative vs. absolute for Site Headroom) — a
   product conversation, not code. This gates everything downstream.
3. If #5 resolves toward unification: build the shared starting-state
   resolver + single absolute engine (per "The proposed unification"
   above), including the future-`at` capability, its UI anchor control
   (#7), and the forecast-uncertainty caveat (#8) in the same piece of work.
4. If #5 resolves toward keeping Site Headroom relative: at minimum, apply
   the same "commit to your own extreme, contribute nothing when that's
   off" principle inside `envelope_forecast.rs` wherever it might have an
   analogous issue (audited here only for PV/EV, not exhaustively), so the
   two curves stay individually correct even if they remain structurally
   separate.
5. Only after 1–4: resume the paused UI request (open point #10), and
   revisit whether the accumulated-charge/capacity kWh curve (#11) is still
   wanted once the underlying per-asset terms are trustworthy.
