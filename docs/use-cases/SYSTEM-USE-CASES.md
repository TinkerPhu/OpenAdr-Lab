# OpenADR System – Core Use Cases

This document lists common, real-world OpenADR-style use cases that a VTN/VEN system should be able to handle. These are intentionally kept concise and implementation-oriented, so they can be directly used for lab testing.

---

## 1. Emergency Load Shed
**Description:**
A utility or grid operator requests immediate load reduction due to grid stress or contingency events.

**Characteristics:**
- Short notice (minutes)
- High priority
- Clear start and end

**Typical Signals:**
- Load reduction percentage
- kW limit
- On/off curtailment

**What to test:**
- Priority handling
- Event acknowledgment
- Correct start/stop timing

---

## 2. Renewable Export Limitation (Zero / Limited Export)
**Description:**
DERs (e.g., solar inverters) must limit or block export to the grid due to congestion or negative pricing.

**Characteristics:**
- Often group-targeted
- May include ramp-down and ramp-up phases

**Typical Signals:**
- Export capacity limit (% or kW)

**What to test:**
- Interval sequencing
- Unit interpretation
- Smooth recovery behavior

---

## 3. Time-of-Use / Dynamic Price Signal
**Description:**
The VTN publishes a price signal that varies by time interval; VENs optimize behavior locally.

**Characteristics:**
- Day-ahead or intra-day
- Many uniform intervals
- No direct control mandate

**Typical Signals:**
- Price per interval

**What to test:**
- Uniform interval handling
- Large interval counts
- Late updates or corrections

---

## 4. Planned Peak Shaving Event
**Description:**
A scheduled curtailment during predicted peak demand periods.

**Characteristics:**
- Known ahead of time
- Moderate curtailment levels
- Often recurring

**Typical Signals:**
- Load or power caps

**What to test:**
- Event lifecycle (far → near → active)
- Event modification handling

---

## 5. EV Charging Management
**Description:**
Control or limit electric vehicle charging to reduce peak demand.

**Characteristics:**
- Group-based targeting
- May overlap with other events

**Typical Signals:**
- Charging pause
- Max charging power

**What to test:**
- Overlapping events
- Priority resolution
- Group membership logic

---

## 6. Battery Dispatch Window
**Description:**
Request batteries to charge or discharge during specific time windows.

**Characteristics:**
- Directional control (charge vs discharge)
- Often irregular intervals

**Typical Signals:**
- Charge/discharge power limits

**What to test:**
- Interval timing accuracy
- Conflicting state requests

---

## 7. Program Enrollment / Connectivity Check
**Description:**
Non-operational events used to verify that VENs are reachable and responsive.

**Characteristics:**
- No real control impact
- Periodic

**Typical Signals:**
- No-op or informational payload

**What to test:**
- Acknowledgment handling
- Reporting / telemetry coupling

---

## 8. Event Cancellation
**Description:**
An active or upcoming event is withdrawn due to changing grid conditions.

**OpenADR 3 Implementation:**
In OpenADR 3 (and openleadr-rs), there is no "cancel" status field on events. Cancellation is achieved by **deleting the event** via `DELETE /events/{id}`. VENs detect cancellation when the event disappears from their next poll cycle.

**Characteristics:**
- Immediate effect (next poll cycle)
- Must override previous instructions

**Typical Signals:**
- Event deletion (no cancel payload — event simply vanishes)

**What to test:**
- VEN detects event removal on next poll
- Clean rollback behavior
- State consistency after cancel

**Demo:**
Run `python3 scripts/seed_vtn.py --vtn-url http://Node1:8200 --demo-cancel` to create a `cancel-demo-event`, wait 5 seconds for VEN polling, then delete it.

---

## 9. Reading a Report This Lab Sent
**Description:**
Anyone consuming this lab's reports — the VTN UI, `experiments/kpi.py`, a third
party — needs to know what the numbers mean. From 2026-09-20 they say so
themselves.

**What changed for a reader:**
- Every report carries `payloadDescriptors` declaring each payload type's
  `units`. It is optional in OpenADR and mandatory here.
- `USAGE` is **energy over the interval, in kWh**, and the interval states the
  window it covers. It used to be instantaneous watts with no window, which is
  what `kpi.py` compensated for by multiplying by duration.
- `STORAGE_CHARGE_LEVEL` is a number in `PERCENT`, not a formatted string.
- `programID` is gone; `eventID` is a report's only object link.

**Reading it correctly:**
Take the unit from the report's own `payloadDescriptors` rather than assuming
one. A report with no descriptor predates the cutover and carries watts;
`kpi.py` applies that default and says on stderr when it did, so a KPI computed
across the cutover is never silently mixed.

**What to test:**
- `tests/features/ven_reporting_out.feature` — "A report on the wire declares
  what its values mean"
- `experiments/kpi.py --self-check` — the same run expressed both ways gives
  the same answer

---

## 10. A VEN That Was Given Nothing
**Description:**
A VEN whose VTN user exists but carries no scopes authenticates perfectly and
then sees an empty world: every request returns 200, every list comes back
empty. Indistinguishable, from outside, from a quiet grid.

**What the VEN does now:**
Reads the scopes from its own access token and reports the missing ones on
`GET /health` (`wire_conformance`) and as a notification, instead of polling
successfully for ever. Same surface shows an object the VTN sent that this VEN
refused — both are "something is wrong with what we are being given, and every
request still succeeds".

The same holds for a *value* the VEN cannot read. An event payload that declares a unit or
currency the lab profile does not read (a capacity limit in `VOLTS`, a price in `USD`) is not
used: that payload alone is dropped, `wire_conformance` turns degraded and names the event, the
payload type, what was declared and what the profile reads, and it clears when the event goes.
A payload whose unit nobody declared is legal: it is read from the profile default, health
stays ok, and the Dashboard's Health card lists it under "wire assumptions" so the assumption
is visible. Scenarios: `tests/features/ven_wire_units.feature`.

**What to test:**
- `tests/features/ven_health.feature` — health exposes wire conformance
- `VEN/src/controller/token_scopes.rs` unit tests

---

## 11. Watching the Whole Fleet Right Now
**Description:**
An operator who has just published a capacity limit to twenty sites wants to
see them react. Reports answer that eventually — they describe intervals that
have *closed*, on the report cadence — which is the right shape for a record
and the wrong shape for a live view.

**What the lab does:**
Every VEN also publishes its own state to the lab broker
(`openadr-lab/fleet/<venName>/telemetry`, the tick cadence, QoS 0 and
retained), and the BFF holds the latest message per VEN and serves it at
`GET /api/fleet/power`.

The two sources stay distinct on purpose. Reports are the record: durable,
spec-shaped, exact. Telemetry is the live feed: lossy, a few seconds old at
worst, and never the thing anything is *measured* by. Nothing a VEN publishes
here can change what it does, and grid instructions arrive as OpenADR events,
where the spec's targeting and priority rules apply. The one message the lab
sends a VEN on this channel is the operator's choice of its planner objective
(§14), the same switch the VEN UI has.

**Reading it correctly:**
- Power is signed: import positive, export negative. A fleet sum that clamped
  it would be wrong by exactly the export.
- A VEN that has said nothing contributes `null`, not zero, and the response
  reports `contributingVens` beside `knownVens` so a partial sum can be judged
  rather than trusted.
- `state` is `online`/`offline`, and `offline` is published by the broker's
  last-will when a VEN dies without saying goodbye. It is the only way to tell
  "gone" from "quiet".

**Where it is visible:**
The VTN UI dashboard health card shows the feed's state, how many VENs are
live out of how many are known, and the age of the last message. The VEN UI
diagnostics shows that VEN's own publisher.

The Fleet page (`/fleet`) draws **one line per VEN plus the fleet total** on a
shared time axis, over a window you pick (15 min / 1 h / 6 h / 24 h). The
legend toggles individual VENs in and out, which is how you isolate two sites
worth comparing out of twenty. Each VEN keeps the same colour across refreshes
because it is derived from the name, not from position in the list.

The curves come from the **live telemetry** the BFF stored — lossy by design,
5 s resolution — not from the VTN's reports. The chart says which resolution
it drew (raw samples, or 1-minute means once the raw rows have aged out).

The page opens on 24 h, so on a freshly started VTN the store holds less than one
bucket of it. The chart then says **"N VENs are reporting, but the store is still
collecting"** rather than claiming there is no telemetry; "No telemetry stored"
means no VEN is reporting at all. Pinned by the scenario "The fleet page does not
deny telemetry while VENs are reporting" in `tests/features/fleet_telemetry.feature`.

**Asking what the fleet did about one event:**
`GET /api/fleet/reactions?eventID=…` lists every VEN that said it saw that
event, when it saw it, when it next replanned, and its mean site power over
the minute before and the minute after. The VTN UI's Fleet page takes an event
id and shows the same table.

It deliberately draws no conclusion about whether a VEN "reacted". The numbers
are reported and the judgement is the operator's: a threshold decided in the
BFF would be a second opinion about a site's own behaviour, and wrong
differently for every asset mix in the fleet. A VEN that published no power in
one of the two windows shows a dash, never a zero.

**Seeing what the fleet was told:**
`GET /api/fleet/signals?from&to` resolves the VTN's events into per-VEN bands —
"ven-3 was under a 3 kW import limit from 09:10 to 09:40" — using the same
interval rule (`lab-core`) the VEN plans against, so a band drawn here and the
limit actually applied there cannot disagree about when an interval runs.

The Fleet page shades those windows behind the power curves, because a dip at
09:15 means something different depending on whether a limit was in force. The
Signals page (`/signals`) draws them per VEN as a timeline: one row per site,
one block per interval, coloured by payload type. A VEN under nothing is absent
rather than an empty row — it was not targeted, and listing it would imply it
was.

An event the BFF cannot parse is counted and shown, because fewer bands than
the VTN sent must not look like a quiet grid.

**What to test:**
- `tests/features/fleet_telemetry.feature`
- `VTN/bff/src/fleet.rs`, `VTN/bff/src/fleet_reactions.rs`,
  `VTN/bff/src/fleet_signals.rs` and `VEN/src/controller/telemetry_port.rs`
  unit tests

---

## 12. Staggering a Fleet's Response
**Description:**
A VTN asks many VENs to respond to one event but does not want them all to act on the same instant, because a synchronised step is itself a grid disturbance.

**Characteristics:**
- One event, many VENs
- `intervalPeriod.randomizeStart` gives the window within which each VEN may delay its start

**Typical Signals:**
- Any event with a window: SIMPLE, capacity limit, dispatch, price

**What to test:**
- Two VENs given the same event begin at different times, each inside the declared window
- A VEN keeps its offset across polls, and says how long it delays (an Info notification)
- Reports keep their requested cadence and are not delayed
- `tests/features/ven_simple_levels.feature` — "Two VENs begin a randomized event at their own offsets"

---

## 13. Editing the Tariff and Watching the Fleet
**Description:**
An operator changes the price schedule the fleet is told about and wants to see what the fleet does with it, without writing JSON or re-seeding the lab. The VTN UI's **Tariff** page loads an existing tariff event into a draft, lets the operator edit it, shows it beside the live tariff and the fleet's power, and publishes it.

**Characteristics:**
- The most common declaration is *one day of prices that repeats forever*: the intervals are sent once and `event.duration = "P9999Y"` repeats them. `P7D` repeats for a week; a duration shorter than the day drops the surplus intervals.
- The editor writes the recurrence only through `event.duration`, never a private field.
- Publishing creates a new event with a higher priority than the ones carrying the same signal; existing events are left unchanged.
- Every published price declares its unit and currency (`payloadDescriptors`).
- A repeating series repeats by elapsed time, so across a clock change it no longer follows local midnight; the page says so.
- Whole-series edits are a table of operators: scale, shift (wrapping inside a repeating pattern), cap, add to a range of hours, and flat / two-tier / three-tier templates. The series can also be shaped on the chart: drag a band's value (snapped to the signal's step) or a row boundary (snapped to its interval grid). A drag on any repetition edits the one pattern, so all repetitions move.
- After publishing, the page compares the fleet's *planned* import and export from just before the publish with now, per direction (energy and peak), and says how many of the targeted VENs have replanned; until all have, the change is marked not final. It reads the fleet plan feed, so it is the same after a reload.

**Typical Signals:**
- `PRICE`, `EXPORT_PRICE` (later `GHG` and capacity envelopes use the same workbench)

**What to test:**
- The wire profile declares `PRICE` as per-kWh in EUR (`GET /api/signals/registry`)
- A draft that repeats forever previews as repeating, resolved by the interval rule the VEN uses
- A tariff published with a recurrence is in force on the second day on the VEN
- A stored repeating tariff loads as its 24-interval pattern, not as an expansion
- The fleet plan: after a VEN adopts a plan it is on `GET /api/fleet/plan`; after a replan, "now" follows and an instant before the new price still answers with the earlier plan (`tests/features/fleet_plan_feed.feature`)
- A price dragged up on the chart in a browser changes one grid price to the value the drag readout showed; publishing it creates a second event carrying that price and `P9999Y`, the VEN's `/tariffs` shows it, and the fleet plan holds a plan the VENs adopted after the publish that the plan of a second before it did not (`@ui` scenario in `tests/features/tariff_editor.feature`)
- `tests/features/tariff_editor.feature`; the unit behaviour is in `VTN/ui/src/__tests__/signalDraft.test.ts`, `recurrence.test.ts` and `Tariff.test.tsx`

---

## 14. Comparing the Fleet With and Without Planning
**Description:**
A lab operator wants to see what planning is worth: the same fleet, the same day, once planning
and once dispatching like a HEMS that does not plan. That needs one switch for the whole fleet,
a view of what the fleet expects to draw either way, and a way back.

**What the lab does:**
- **Switch** on the VTN UI's Fleet page: per VEN (the Fleet command column) or for all (Fleet
  objective, Apply to all; one VEN every 10 s, so their replans do not solve at once). The BFF
  publishes a retained command on the VEN's `command/objective` leaf; the VEN applies it and
  reports the objective, its source ("fleet") and the accepted objectives back on its
  `objective` leaf. The Objective column shows what each VEN reports, and "pending" until it
  reports the command applied.
- **Before and after:** the Fleet plan card (the fleet's adopted plans summed, `GET
  /api/fleet/plan`) names the objectives the fleet runs under and compares the plan as of just
  before the first VEN of the newest switch changed (the BFF notes when each VEN's objective
  changed) with the plan now, the way the Tariff page compares a published tariff: "n of m VENs
  replanned", the planned import and export energy and peak before against now, and a chart that
  opens on the accumulated planned energy (kWh) with the faint "before" curve beside it, power one
  click away. Accumulated energy is the quickest reading: each VEN's plan shows its effect the
  moment that VEN has replanned. The experiment is the same fleet under one objective and then the
  other, one after the other. Every
  VEN also reports a `USAGE_FORECAST` to the VTN, ad hoc: at once when its plan's objective
  changes, else every 15 minutes (`WIRE_PROFILE.md` v3).
- **Drill-down:** a row opens to that VEN's adopted plan (net and per-asset power, from the same
  plan store, `GET /api/fleet/vens/:ven/plan`); under the opportunistic objective it is a
  simulated forecast.
- **Release:** Release (per row) or Release all withdraws the command and each VEN returns to its
  profile objective. A change made on the VEN UI afterwards stays.
- **Restart:** a VEN restarted while commanded applies the retained command again; after a
  release it starts on its profile objective.

**What to test:**
- `tests/features/fleet_objective_control.feature`: one VEN and the fleet switched, the VEN
  reporting the fleet as source, release, a refused objective, the plan from before a switch and
  the VEN's new plan, the ad hoc usage forecast, the Fleet page's comparison and accumulated chart
  (`@ui`), restart (`@resilience`)
- The VEN side of the same objective: UC-19 in `HEMS-USE-CASE-OBSERVATION-MANUAL.md`

---

## Notes
If the system can reliably handle all use cases above, it already matches the majority of real-world OpenADR deployments. More complex scenarios (stacked markets, transactive energy, multi-program arbitration) typically build on these fundamentals.

