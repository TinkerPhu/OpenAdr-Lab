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

**What to test:**
- `tests/features/ven_health.feature` — health exposes wire conformance
- `VEN/src/controller/token_scopes.rs` unit tests

---

## Notes
If the system can reliably handle all use cases above, it already matches the majority of real-world OpenADR deployments. More complex scenarios (stacked markets, transactive energy, multi-program arbitration) typically build on these fundamentals.

