# Fleet planning experiment: objectives, signals, and a tariff that relaxes the fleet's grid power

Intent: **aggregate/system** (see `docs/guidelines/FLEET_EXPERIMENT_DESIGN.md`). Heterogeneous
fleet is fine; per-category numbers are reported as underpowered. Measured by the *planned* fleet
net power (`GET /api/fleet/plan`, summed) because the question is how planning reacts, and
confirmed by metered power (`GET /api/fleet/power`, telemetry) for the steps that matter.

Times below are UTC unless marked; local is CET = UTC+1 until 2027-03-28.

## 0. Starting state (checked 2026-10-29 08:30 CET)

| Item | State |
|---|---|
| Hosts | Node1 load 1.1, 2.5 GB available of 3.8 GB, disk 48%. Node2 load 1.9, 2.0 GB available, disk 16%. Node3 idle. Docker lock on Node1 FREE. |
| Fleet | 20 VENs online: ven-1..3 on Node1, ven-4..20 on Node2, VTN stack + BFF (`:8220`) + VTN UI (`:8221`) on Node1. |
| Objectives | **Mixed**, left by an earlier run on 25 Oct: ven-1..3 `opportunistic`, ven-4..20 `min_cost` (all source `fleet`, profile objective `min_cost`). Reset before any run. |
| Tariff | Repeating 24 h hourly pattern, import 0.06 to 0.40 EUR/kWh: night low 02-03Z (0.06), morning peak 08Z (0.25), midday shoulder 12Z (0.13), evening peak 17-18Z (0.40/0.38). Export = import - 0.02..0.10. |
| Asset mix | PV 12, battery 9, EV 12, heater 10 of 20 VENs (all have base load). Combination categories hold 1 to 5 VENs. |
| Current fleet plan (next 24 h) | Outside the night 4 to 29 kWh per hour. **02-04Z (03-05 CET): 85 and 111 kWh per hour**, four to ten times the rest of the day. Matches the observed morning peak. (Plan slots are 5 min near-term and 10 to 15 min later; energy is prorated by each slot's own length.) |

## 1. Why the standard tariff makes the morning peak

Every VEN solves the same cost problem against the same 0.06 EUR/kWh window. Batteries, EVs and
heaters all want their energy in 02-03Z, each capped only by its own charge rate, so the
shifts add up instead of spreading. Equal incentive for 20 sites is a coincident peak. That is
herding, and a single uniform price cannot avoid it by itself: wherever the price is flat across
slots, sites are indifferent and tie-break the same way (see KEY_LEARNINGS, EV tie-break).

## 2. Experiment sequence (each step is wait-for-replan, about 4 to 8 minutes)

Preconditions: DOCKER lock is not needed (no build, no suite); the experiment only talks to the
BFF/VTN. Take a fresh snapshot of `/api/fleet/plan` before every step; the Fleet page's
"n of m VENs replanned" gate says when a step is final. Record each step's summed plan
(hourly import, export, peak) into `experiments/results/<run>/`.

| Step | Action | Expected reaction | Read |
|---|---|---|---|
| A0 | Fleet objective: Release all, then Apply all `min_cost` | Mixed fleet becomes uniform; ven-1..3 leave simulated forecast | objective column all `min_cost`; plan `solve_status` OPTIMAL |
| A1 | Baseline plan, standard tariff | The 02-04Z spike above | hourly table |
| B1 | Apply all `opportunistic` | Night spike shrinks, evening import follows load; planned cost rises | Fleet plan card before/after, Service delivered |
| B2 | Apply all `min_grid`, then `min_import`, then `min_ghg`, `max_revenue` | hypotheses to check: min_grid/min_import flatten without a tariff; min_ghg follows the GHG column (cleanest near 12-13Z in the seed); max_revenue moves export to the high-export-price hours | per-objective hourly table |
| B3 | Back to `min_cost` | Plan returns to A1 within one replan | diff against A1 |
| C1 | Price spike: +0.30 EUR/kWh for 2 h at 12-14Z (event, priority above background) | PV-hour batteries and EVs stop charging, grid import moves out of the window | `reactions?eventID=`, Fleet plan card |
| C2 | Import capacity limit 40 kW per VEN group for 02-04Z (`IMPORT_CAPACITY_LIMIT`) | Night spike truncated; energy moves to 01Z and 04-06Z | hourly table |
| C3 | Export capacity limit 0 kW at 11-13Z | PV-owning VENs shift flexible load into the window or curtail | export column |
| C4 | `randomizeStart` 30 min on C1 | Start offsets spread; edge of the window less steep | per-VEN reaction start |
| C5 | Cancel by editing the event timing (UC 8) | Plans return to A1 | diff |
| D | Tariff search (section 3) | Peak import and peak export of the fleet both below the standard tariff's | peaks and spread (section 3) |

Each C step is separate: create, wait for all targeted VENs to replan, read, delete the event.
Stop and report if any VEN shows `INFEASIBLE`, a `CAPACITY_VIOLATION` it did not have in A1, or goes
offline.

## 3. Finding a tariff that equalises grid power

Metric: for the fleet plan P(t) = net import - net export, summed, per hour over a 24 h repeat,
the peak import (max P), the peak export (-min P) and the standard deviation of P; the range
J = max P - min P is reported too but one outlier hour dominates it. Also planned fleet cost, and the Service
delivered card (unserved energy must stay 0, otherwise the relaxed grid is bought with comfort).

Method: **coordination by price iteration**, with the VENs acting as the solver.

1. Start from the standard tariff (A1).
2. Read the plan, compute P(t) per hour and its target P* (the daily mean).
3. Update: price(t) <- price(t) + k * (P(t) - P*) / P_scale, clipped to [0.03, 0.45], keeping the
   export price at import minus a fixed spread so import and export do not cross.
4. Publish as a new higher-priority `PRICE`/`EXPORT_PRICE` event via the Tariff page or
   `run_experiment.py` `price_series` (declared unit and currency, `P9999Y`).
5. Wait for all 20 VENs to replan, repeat. Shrink k when the sign of the error flips (damping).
   Stop at 8 iterations or when J drops below 25% of A1's.

Two known obstacles the iteration will meet, and what to do about each:
- **Price plateaus herd.** At a converged flat price, VENs are indifferent and move together.
  Use a slightly sloped price across the window (a staircase of 0.005 to 0.01 EUR per hour) so the
  order in which sites fill it is defined, or target two or three VEN groups with different
  windows through enrolment.
- **Price is the wrong lever for bounds.** A tariff cannot guarantee a ceiling. After the
  price converges, add `IMPORT_CAPACITY_LIMIT` as the guarantee (C2), with the price doing the
  steering and the limit holding the peak.

## 4. Does a VTN-side MILP help?

Not as the first tool. A direct MILP would need every site's assets, forecasts, comfort rules and
current state, which the VTN deliberately does not have (and should not: it would duplicate
every asset's own competence, see `asset-competence-assurance`). The VENs already are the
solver of the inner problem, so the VTN's problem is a bilevel one, and price iteration (section
3) solves it by asking. What the VTN does have: the ad hoc `USAGE_FORECAST` reports, the fleet plan
feed, and per-VEN reactions. Build a MILP only if section 3 fails: for example a small aggregate
model fitted from the iteration data (price to energy response per hour), solved to pick the next
price vector in one step instead of eight. Decide after D.

## 5. Output

- `experiments/results/<run>/steps.csv`: step, time, hourly import/export/net, J, cost, unserved.
- Journal entry in `docs/history/project_journal.md` with the converged tariff and whether it
  held across a second day.
- Cleanup: Release all objectives (return to profile), delete experiment events, confirm
  `/tariffs` on one VEN per host equals the standard series.
