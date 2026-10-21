## Context

See proposal.md - Why. What the code does today, read from it and from the fleet's live history:

- `UserRequestMode::non_planning_dispatch` maps ASAP and BY_DEADLINE to `StartNow`, the rest to
  `SurplusOnly`. `request_regime` turns `StartNow` into "full effort, out of the pass".
- An EV with no session whose switch is on is offered surplus only. The ten fleet EVs declare
  `usage_forecast`, which reaches the MILP planner only, so under this objective nothing states their need.
- The opportunistic forecast runs `request_regime` per slot, so whatever changes here is planned too.

## Goals / Non-Goals

**Goals**: the opportunistic objective serves a request's mode as it says (now, or as late as the deadline
allows), and a plugged-in EV below target is not left idle; both through the one decision the live tick and
the forecast already share. **Non-Goals**: a tariff-aware wait (that is planning); a latest start for the
heater (needs its thermal model, filed); a required-rate controller (considered, D5); the service-level KPI on
the Fleet card (filed).

## Inventory of existing logic reused or consolidated

| Concept | Where | This change |
|---|---|---|
| How a request is served without look-ahead | `design_vocabulary.rs::non_planning_dispatch`, `NonPlanningDispatch` | Extended: takes the request's deadline; `StartBy(deadline)` joins `StartNow` and `SurplusOnly` |
| Regime a request sets for an asset | `opportunistic.rs::request_regime` | The one place that decides start-now / wait / take part; gains `now` and the asset's two declarations; live tick and forecast both call it |
| Active requests per asset | `opportunistic.rs::opportunistic_requests` | Passes each request's deadline (session `departure_time`, heater `ready_by`, load `latest_end`) into `non_planning_dispatch`; no second map |
| Energy to time at full effort | nowhere (assets know energy and power; the arbiter must not convert) | One typed asset method `full_effort_time_to_target_s`, built on `entities/units.rs`; no inline conversion elsewhere |
| What an asset does with no request | `request_regime`'s `None if SurplusCharge && !switch` arm | Replaced by an asset declaration `dispatch_without_request`; the arm is deleted, not extended |
| PV surplus claim | `opportunistic_pass`, `allocate` | Unchanged; a waiting asset simply takes part in the pass |
| Per-slot decision in the forecast | `opportunistic_forecast.rs::slot_decision` | Passes the slot's `now`; owns no rule |
| Opportunistic outcome on the wire | `OpportunisticOutcome`, `ArbiterDiagnostics.ts` (generated), `StatusRows.tsx` | Gains `waiting`; same row, same generated type |

Searched for a second copy of "when must this asset start": `ev_trip_series`, `ev_comfort` and the MILP
deadline constraints decide it inside the solve (a different mechanism: optimisation, not a rule); none is
reachable without a plan, so nothing is consolidated, and the reason is recorded here.

## Decisions

**D1 `NonPlanningDispatch::StartBy(deadline)`.** BY_DEADLINE becomes `StartBy(deadline)`, ASAP stays
`StartNow`. Alternative: keep the enum and a second per-asset deadline map - rejected: two maps that must
agree, and a mode that needs a deadline would be able to lose it.

**D2 The asset declares its time to target.** `Asset::full_effort_time_to_target_s() -> Option<f64>`, carried
on `AssetSnapshot` (like `surplus_claim_rank`): the seconds the asset needs at full effort to reach the
target it currently holds. EV: remaining energy to its effective target (the tick input already carries the
session target) over its max charge power, including its charge efficiency, all through `entities/units.rs`.
Shiftable load: its run time (`duration_min`). Heater: `None` (filed). `None` means the request is served
as ASAP. Alternative: the arbiter computes it from `soc`, `battery_kwh`, `max_charge_kw` - rejected: the
`asset-competence-assurance` rule and the architecture audit forbid reading raw asset values.

**D3 Latest start with an additive margin.** In `request_regime`, for `StartBy(d)`: with
`slack = (d - now) - time_to_target`, the asset starts when `slack <= DEADLINE_MARGIN_S`
(15 minutes, one named constant beside `request_regime`), else it takes part in the pass like a
surplus-only request. Why additive: the check runs every tick with the time to target recomputed from the
live state, so once the asset runs at full power the time to target falls by exactly the time that passes
and `slack` stays constant. The asset therefore stays started with no stored state (no latch to hold or
persist across a restart). A proportional margin (the first idea) makes `slack` grow while running and would
stop the asset again, which needs an explicit latch; not worth it. Surplus taken while waiting also lowers
the time to target, so the latest start moves later by itself. A limit cut or lagging charger lowers the
rate: `slack` shrinks and the asset stays started. A deadline that cannot be met gives `slack < 0`: start now.
The margin is the one tunable: pinned by `opportunistic_tests.rs`, revisit only on evidence.

**D4 An unrequested plugged-in EV is an implicit ASAP request.** `Asset::dispatch_without_request() ->
Option<NonPlanningDispatch>`, carried on the snapshot: the EV declares `StartNow` while it can charge
(plugged, below target, so its time to target is positive) and the switch is on; every other asset declares
`None`. `request_regime` reads it for an asset with no request; the arm that excluded a switch-off EV goes
away because the declaration already depends on the switch. The switch keeps its meaning in planning
objectives (the surplus overlay); under this objective "on" now means "charge automatically", which the
EV card says. Alternative: a second setting for "charge greedily" - rejected for now: one more switch for a
behaviour no user would turn off while leaving surplus charging on; reopen on evidence. A predicted
departure (`usage_forecast`) is not a deadline: it is too uncertain to wait on (probability, jitter), which is
why the implicit request is ASAP, not BY_DEADLINE.

**D5 Required-rate rejected.** Charging at `remaining / time left` from plug-in keeps slack and spreads
the fleet's load, but under the lab tariff it charges slowly through the evening peak. Latest start at full
power was chosen; the comparison and the fleet-peak trade-off go in the docs as the known cost of D3.

**D6 The outcome lists what waits.** `OpportunisticOutcome.waiting: Vec<{asset_id, start_by}>`, filled by
`request_regime`'s decision (the one place that knows), shown in the existing VEN UI opportunistic row
(`ui-transparency`).

## Risks / Trade-offs

The saving a latest start brings under this tariff is the tariff's shape, not the rule's knowledge: it is
price-blind and wins only because evenings are dear and early mornings cheap. The docs and the card caption
must not present it as cost optimisation. A fleet of EVs with similar deadlines starts within the same hour at
full power: a tariff-blind peak that ASAP does not have. The heater's BY_DEADLINE stays start-now until its
time to target exists. A stale `departure_time` or a car that leaves early ends below target: the same risk
any deadline controller carries; the diagnostics row makes the wait visible.
