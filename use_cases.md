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

**Characteristics:**
- Immediate effect
- Must override previous instructions

**Typical Signals:**
- Event cancel message or status update

**What to test:**
- Clean rollback behavior
- State consistency after cancel

---

## Notes
If the system can reliably handle all use cases above, it already matches the majority of real-world OpenADR deployments. More complex scenarios (stacked markets, transactive energy, multi-program arbitration) typically build on these fundamentals.

