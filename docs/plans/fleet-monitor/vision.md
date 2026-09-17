# Fleet Monitor — Vision

A dashboard and control board on the VTN side that lets an operator create simple events,
see the signals the fleet is under (tariffs, envelopes, short-term signals), and watch the
fleet react live — as a summed fleet power curve, as each VEN's own grid power, and as each
VEN's possible power and near-term capacity.

It serves three audiences with overlapping but distinct questions:

| Audience | Core question |
|---|---|
| **VTN controller** (operator) | Did my signal reach every VEN, and did they act on it? |
| **Energy provider** (market/tariff) | How much load moved, what did it cost, what flexibility can I sell? |
| **Grid controller** (DSO) | Is the fleet staying inside the grid envelope, now and in the next hours? |

This document is the full feature vision. The first delivery step and the environment it
needs are in [phase-0-foundation.md](phase-0-foundation.md).

---

## Decisions already taken

- **One MQTT broker** — the existing Mosquitto on Node1. No second broker; lab traffic is
  separated by topic namespace (`openadr-lab/fleet/...`) and, where needed, by an
  authenticated listener, not by a second process.
- **Two data sources in parallel** — OpenADR reports (what a real VTN operator sees) *and* an
  MQTT side channel (what the lab can see live). Every series in the UI carries a source badge
  (`report` / `live`), so the gap between the two views is itself observable.
- **Host UI** — the fleet views live in the existing VTN UI (`VTN/ui`), fed by the VTN BFF.
- **First views** — §1 Signal timeline, §2 Fleet power chart, §5 Reaction tracing (plus the
  quick event composer they need to be driven).

---

## Data sources

| Source | Latency | Content | Realism |
|---|---|---|---|
| OpenADR reports (VEN → VTN → BFF poll) | minutes (report frequency + polling) | only what the program's `reportDescriptors` request | what a real VTN sees |
| MQTT side channel (VEN → broker → BFF) | seconds | anything the VEN knows: grid power, plan, capability, per-asset state | lab-only view |
| VTN objects (programs, events, VENs) | BFF poll | signals, targets, versions | real |

Design principle: the report view is the reference; the live view shows what the reference
misses. A future "report-only mode" toggle hides everything the operator could not see
through OpenADR alone.

---

## Core views (all audiences)

1. **Signal timeline** — one time axis showing every active and upcoming event as a band:
   tariff steps, GHG, import/export capacity limits, alerts, dispatch setpoints. Each band
   shows its program, target (fleet / group / single VEN) and version. Scrubbable into the
   past as well as forward.
2. **Fleet power chart** — summed grid power of all VENs, stackable by VEN or by asset type,
   with the active limits and setpoints drawn over it, so a breach is visible at a glance.
3. **Per-VEN small multiples** — one tile per VEN: grid power, a short plan preview, the
   flexibility band, SoC/temperature mini-gauges, connection health and time since the last
   report.
4. **Flexibility envelope** — per VEN and summed over the fleet: how far power can still move
   up or down now and over the next ~15 min to 4 h, against the planned power. The assets and
   the planner already know this (`/flexibility`, the capacity curves); the monitor must read
   it from them, never recompute it.
5. **Reaction tracing** — pick an event and see its chain: created → first poll per VEN →
   replan → power change → report. Measures latency and response magnitude per VEN.
6. **Replay** — record a session (signals + fleet response) and play it back at a chosen speed;
   useful for experiment windows and for comparing planner versions.

---

## VTN controller (operator view)

- **Quick event composer** — one-click templates ("price spike 18–20 h", "import cap 5 kW for
  1 h", "grid emergency now") with a live preview of the intervals on the signal timeline
  before publishing.
- **Targeting** — whole fleet, a VEN group, or a single VEN; the affected VENs are shown before
  publishing.
- **Event lifecycle** — edit (version bump), cancel (DELETE), duplicate, shift in time, with a
  diff between versions.
- **Delivery status per VEN** — seen (inferred from polls) / acted on (replanned) / report
  overdue.
- **Program/enrollment matrix** — VENs × programs: enrollment, credentials, report obligations.

## Energy provider (market / tariff view)

- **Tariff and GHG curves over fleet consumption** — how much load moved into cheap or green
  windows, estimated cost and CO₂ against baseline.
- **Baseline vs. actual vs. plan** — fleet and per VEN, with shifted energy (kWh) and effective
  price elasticity (Δ kW per Δ EUR/kWh).
- **What-if price signal** — send a draft tariff as a test event to a subset of VENs and compare
  against a control group (A/B).
- **Portfolio position** — planned fleet energy per 15-min slot as a forecast, with the
  after-the-fact deviation (an imbalance view).
- **Flexibility offer book** — summed up/down flexibility per time slot, ranked by the planners'
  marginal cost; what the fleet could sell.

## Grid controller (DSO view)

- **Capacity envelope view** — import/export limits, reservations and subscriptions as a band
  per VEN and aggregated; violations highlighted, with a `CAPACITY_VIOLATION` alarm list.
- **Feeder/transformer grouping** — assign VENs to a virtual feeder with a rating and show its
  loading %; makes coincident peaks visible (everyone charging at the start of a cheap window).
- **Rebound detection** — mark the load recovery after an event ends; flag new peaks caused by a
  price signal.
- **Emergency panel** — the red button (`ALERT_GRID_EMERGENCY` / `ALERT_BLACK_START`), then the
  fleet response curve and time-to-comply per VEN.
- **Headroom forecast** — how far below the limit the fleet will stay in the next hours,
  including plan uncertainty; early warning before intervention is needed.
- **Conflict view** — when a price signal and a capacity limit pull in opposite directions,
  which one each VEN honoured.

---

## Nice-to-haves

- **KPI strip** — fleet kW now, compliance %, flexibility up/down, response latency, report
  freshness.
- **Annotations** on the timeline ("heater profile changed here").
- **Export** of a time window as CSV/JSON for the experiment KPIs in
  `docs/plans/strategic_roadmap.md` §4.
- **Fleet-scale mode** — multiply simulated VENs to test aggregation and UI performance.

---

## Constraints from the project rules

- **DTO passthrough** — OpenADR field names (`payloadType`, `intervalPeriod`, `clientName`, …)
  end to end: VEN, MQTT payloads, BFF, UI.
- **One concept, one function** — "when does interval i run" and "which limit applies at t" come
  from the existing shared rules (`entities::time_window::TimeWindow`,
  `controller::event_timing`, `tightest_capacity_limit`); the UI must not grow a third copy.
  Where the UI needs the same rule in TypeScript, the BFF delivers the resolved answer instead.
- **Asset competence** — possible power, flexibility and capacity are the assets' answer,
  published by the VEN; the monitor displays, never derives.
- **UI transparency / no half-built features** — every new feed (MQTT topic, report type, BFF
  stream) ships with its visible surface in the same piece of work.
- **Use-case BDD** — each view gets a scenario in `tests/features/` that exercises what its user
  does.
