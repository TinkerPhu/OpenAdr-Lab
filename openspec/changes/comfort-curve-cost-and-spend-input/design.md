# Design

## Context

Observed in the code (see proposal.md for motivation):

- A curve is `Vec<ComfortRate>` (`fill`, `max_marginal_price_eur_kwh`, `max_marginal_co2`), edited per asset in
  `ComfortCurveCard.tsx`, stored through `services/comfort.rs`, served by `routes/hems/comfort.rs`
  (`GET/POST/DELETE /assets/:id/comfort_curve`). The card lists ev, heater and battery.
- The planner prices an EV by `assets/ev_comfort.rs::ev_energy_segments`: bands from current SoC to a ceiling, at most 5 %
  wide, priced at the midpoint, CO2 monetized in, equal-priced neighbours merged. A test pins "sum of
  kWh x price = area under the curve". Only the EV has bands.
- `validate_curve` (`entities/comfort.rs`) is the one validator; its non-increasing-bid rule keeps the MILP binary-free.
- The plan carries `AssetAllocation.cost_eur`; `session_flexibility.rs::solved_session_cost` already sums an asset's
  allocation cost over a window.
- `budget_eur` / `max_total_cost_eur` are hard MILP cost ceilings (`ev_milp.rs` ~206), unrelated to the curve.
- Routes may not import `assets/` or `simulator/`; they reach assets through `SimReadPort`
  (`default_comfort_rates` is the model to follow).

## Goals / Non-Goals

**Goals:** show maximum cost and planned spend beside the curve; accept and show cumulative spend; one
implementation of every concept involved; two parts shippable on their own.

**Non-Goals:** any MILP or planner change; merging with `budget_eur`; per-request (session) curves in the UI;
cumulative input for heater/battery; CO2 in the spend view; an OpenADR wire change (VEN-local HTTP API only).

## Inventory: where each concept lives today (one-concept-one-function)

| Concept | Lives in | Plan |
|---|---|---|
| "price the energy ahead by the curve" | `ev_comfort::ev_energy_segments` | **Reuse** for max cost (with `w_ghg = 0`) and for the cumulative view (running sum of the same bands). No second integral. |
| "which curve applies" | `entities::comfort::effective_comfort_rates` | Reuse. |
| "is this curve acceptable" | `entities::comfort::validate_curve` | Reuse on the derived marginal curve; the cumulative route only adds a spend-vocabulary pre-check for the message. |
| "what the plan spends on an asset" | `session_flexibility::solved_session_cost` | **Generalise**: the window end becomes a parameter (the card passes the plan end); move it next to the plan (`entities/plan.rs` or a sibling) so the card and session flexibility call one function. Searched `cost_eur` and `grid_power_kw * ... import_tariff`: this is the only copy. |
| "asset reports its own bands" | none | **New** `Asset::comfort_bands`, `None` by default, `Some` on `EvCharger`. The asset is the authority (`asset-competence-assurance`); the route never reads `soc`. |
| "kWh per unit of fill" | `battery_kwh` inside `EvCharger` | Reused through the same asset method; the caller never supplies it. |

No copy found that needs consolidating beyond `solved_session_cost`.

## Decisions

**D1 - Max cost is price-only.** `ev_energy_segments` takes `w_ghg_eur_kg`; passing 0 drops the monetized CO2, so
the figure is comparable with `cost_eur` (euros paid). Alternative: include CO2 - rejected, it is a value, not a bill.
Bands run from current SoC to the **vehicle's charge limit** (the planner's ceiling), not the session target: the card
describes the curve, not one request.

**D2 - One route, preview semantics.** `POST /assets/:id/comfort_curve/cost` takes rates and returns
`{ max_cost_eur, energy_kwh, planned_cost_eur }`; it stores nothing. The UI calls it with the rows on screen
(saved or edited), so there is no client-side integral. Alternative: add the figures to `GET` - rejected, an edited
unsaved curve would not be covered. An asset without bands gets `max_cost_eur: null`; the UI renders nothing for it
(presence-driven, no per-type branch: `declare-dont-branch`).

**D3 - Planned spend may exceed the maximum.** A hard-deadline request is obliged to charge beyond the curve's value,
so `planned > max` is legitimate. The card shows both and says why instead of clamping or hiding.

**D4 - Cumulative spend is measured from the current charge; the stored curve stays marginal.** Stored form,
validator, planner and `GET` are unchanged. The cumulative view is always derived from the stored curve, so it can
never disagree with it. Alternative: anchor at 0 % - rejected, nobody means "EUR 2 from empty". Cost: the curve persists
in absolute SoC, so "EUR 2 to 60 %" entered at 20 % means something different at 30 %; the card labels the figures
"from X % (current charge)" and re-derives them live.

**D5 - Intervals become steps.** Each interval's marginal is constant (added spend / interval kWh). The stored curve is
piecewise-linear, so a step is written as two points, `(lo, p)` and `(hi - e, p)` with e = 0.001 fill, followed by the
next interval's `(hi, p_next)`. Below the first point the curve clamps to the first price (`value_at_fill`). Error: the
ramp band of width e distorts at most e x battery_kwh x delta_p, about EUR 0.006 per knot at 60 kWh and a 0.10 EUR/kWh
step; the round-trip test pins at most EUR 0.01. Alternative: points at interval midpoints - rejected, linear
interpolation between them misses the knots the user typed.

**D6 - Spend input has its own route.** `POST /assets/:id/comfort_curve/spend` with
`[{ fill, cumulative_max_cost_eur, max_marginal_co2? }]` returns 201 and the stored `ComfortCurveResponse`; 422 for a
non-concave curve or an asset that declares no bands (the same presence check as D2, one source). CO2 bids are carried
per point unchanged (default 0): the spend input covers price only. Alternative: a mode flag on the existing POST -
rejected, two bodies behind one route is the per-case branching the project avoids.

**D7 - `budget_eur` stays separate.** Every kWh the planner buys has a tariff no higher than its bid, so a curve's
spend is bounded by its own area (the "maximum cost") without a constraint. `budget_eur` is a different, hard,
per-session ceiling. The card neither edits nor displays it.

**D8 - Concavity wording.** The user-facing rule becomes "each interval must add no more per kWh than the one before".
`validate_curve` stays the gate; the spend route reports the same violation in spend terms before the marginal check.

## Risks / Trade-offs

- [Stale intent] A spend curve typed at one SoC drifts as SoC moves -> label the anchor, re-derive live, record the
  per-session-curve alternative (below).
- [e distortion] Thin ramp bands add a few cents of error -> pinned by test. Band count matters (R-97: 14 unmerged
  bands cost 56x solve time) -> a test asserts a 3-point spend curve yields at most 8 planner bands.
- [Misreading max as a budget] -> copy says "most this curve can spend", never "budget".
- [Preview cost] one cheap pure computation per edit -> debounce in the UI.

## Migration Plan

None: no stored data changes, no change to existing routes. Part A ships alone (new route and card text). Part B adds a
route and an editor mode. Rollback is a revert.

## Open Questions

- Should cumulative input later live on the **request** (`comfort_rates` already exists on the EV request, and SoC is
  known at creation) instead of the asset? Neither part needs the answer; revisit after Part B is in use.
