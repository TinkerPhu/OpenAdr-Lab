# VEN Simulator + Reactor: Milestones 1 & 2 (No VTN, No LLM, No SQLite)

This document is an **implementation-ready plan** for improving the VEN side of the OpenADR 3 lab by delivering:

1) a realistic **actor/sensor simulator** (replacing the current placeholder sensor), and  
2) a deterministic **reactor** that processes OpenADR events into setpoints and drives the simulated actors.

It intentionally excludes VTN/operator analytics and excludes LLM integration for now.

Sources for alignment with the current repository and the simulation concept:
- Repo architecture and current VEN description in README fileciteturn0file0
- Simulation concept, module separation, energy counter and reaction strategies fileciteturn0file1

---

## Why this is the right starting point (reasoning recap)

### Why simulator first
- Your current VEN uses a placeholder fake sensor (power derived from timestamp). That blocks “realistic” testing because **events cannot cause believable telemetry changes**. fileciteturn0file1  
- Without plausible telemetry, reactor debugging is misleading: you won’t know whether “wrong behavior” comes from control logic or from a non-physical simulated world.

### Why reactor second (but still in early scope)
- A reactor is required whether sensors/actors are simulated or real. It is the “always present” brain that interprets OpenADR events and generates actions. fileciteturn0file1  
- Implementing a small subset of signal types yields big coverage: price-like incentives and capacity-like hard constraints already cover many real use cases.

### Why persistence, and why *not* SQLite
You want continuity on restart for:
- **energy counters** (kWh totals)
- **stateful devices** (EV SOC, thermal temp, etc.)
- **reactor sticky state** (applied setpoints + ramp progress)

But you **do not** yet need queryability, migrations, or reporting history. A simple per-VEN `state.json` on a mounted volume is:
- fastest to implement
- easy to inspect
- adequate for lab fidelity

SQLite can be added later when you start needing multi-table history (“last 50 decisions”, scenario replay, etc.). For milestones 1 & 2, keep it **JSON file persistence only**.

---

## Non-goals (for these milestones)

- No changes to VTN / BFF / VTN UI
- No operator analytics module
- No LLM integration
- No SQLite / Postgres on VEN side
- No attempt to support the full OpenADR event type universe: implement a small, high-value subset first

---

## Definitions

- **Simulator**: modules that emulate sensors and actuators, including a simple power model and energy integration.
- **Reactor**: event-processing logic that selects active intervals, chooses a response strategy, and outputs **setpoints**.
- **Setpoint**: a target value for an actor (e.g., EV charge power, heater power, PV curtailment limit).

---

## Milestone 1 (M1): “Believable devices + simple event response” (high value, low effort)

### Goal
Replace the placeholder fake sensor with a small, consistent simulated world so that:
- power, energy, and device states evolve realistically over time
- the reactor can change outcomes in obvious, testable ways

### M1 Scope
1) **Simulator layer**
   - Actor models: choose **2–3** initial actor types:
     - **EV charger** (power → SOC)
     - **Heater / HVAC** (power → temperature or “thermal energy”)
     - **PV inverter** (production curve + optional curtailment)
   - Sensor snapshot generation from actor states:
     - power import/export (W)
     - voltage (optional), temperature (if HVAC), SOC (if EV/battery)
   - **Power model**
     - `net_power_w = base_load_w + ev_power_w + heater_power_w - pv_power_w`  
       (define sign convention clearly; recommended: positive = import from grid)
   - **Energy integration**
     - `kwh_total += (net_power_w / 1000) * (dt_seconds / 3600)`
   - **Persistence to JSON file**
     - Persist: `kwh_total`, device states (SOC/temp), last tick timestamp.
     - Path: use environment variable or existing persist mechanism (concept mentions `PERSIST_PATH`). fileciteturn0file1

2) **Reactor (minimal)**
   - Implement the event/interval selection:
     - determine “currently active interval” from event `intervalPeriod` + interval index
   - Implement initial response to **one hard constraint** and **one incentive**:
     - `EXPORT_CAPACITY_LIMIT` (hard constraint)
     - `PRICE` (incentive)
   - Output setpoints to simulator actors (not direct power measurements).

3) **VEN UI quick update** (minimal, optional but recommended)
   - Display:
     - current net power (W)
     - energy counter (kWh total)
     - key device state(s) (SOC, temp)
     - “active event” badge + current interval payload summary

### M1 Data model (recommended)

#### Runtime state (in memory)
```text
SimState
  now_ts
  last_tick_ts
  energy:
    kwh_total
  devices:
    ev:
      plugged: bool
      soc: 0.0..1.0
      max_charge_kw
      commanded_charge_kw
    heater:
      temp_c
      temp_target_c (optional)
      max_kw
      commanded_kw
    pv:
      rated_kw
      irradiance_factor (0..1)
      curtailed_ols (0..1) or curtailed_kw
  grid:
    voltage_v (optional)
```

#### Persisted state (state.json)
Persist the minimal subset required for continuity:
```json
{
  "version": 1,
  "saved_at": "2026-02-13T12:00:00Z",
  "energy": { "kwh_total": 42.7, "last_tick_ts": "..." },
  "devices": {
    "ev": { "soc": 0.55, "plugged": true },
    "heater": { "temp_c": 20.8 },
    "pv": { "irradiance_factor": 0.62 }
  },
  "control": {
    "mode": "EXPORT_CAP|PRICE|NONE",
    "active_event_id": "optional",
    "active_interval_id": 0,
    "setpoints": { "ev_charge_kw": 3.0, "heater_kw": 0.0, "pv_ols": 1.0 },
    "ramp": { "started_at": "...", "from": 0.0, "to": 3.0, "duration_s": 300 }
  }
}
```

### M1 Reactor behavior rules (simple and deterministic)

#### Priority rule (hard constraints beat incentives)
1) If `EXPORT_CAPACITY_LIMIT` is active → comply with export cap first  
2) Else follow price behavior

#### Export cap response (no battery assumed in M1)
Goal: keep `export_w <= export_cap_w`.

Strategy order:
1) increase flexible consumption if available (EV charge, heater) up to limits  
2) curtail PV as last resort (reduce PV output via OLS/curtailment)

Implementation sketch (conceptual):
- compute predicted export with current setpoints
- if export exceeds cap:
  - raise EV/heater power (bounded)
  - if still exceeds: reduce PV output factor (bounded)

#### Price response (very simple)
- if price >= HIGH_PRICE: reduce flexible loads
- if price <= LOW_PRICE: increase flexible loads (valley fill)
- else: normal

Hardcode thresholds in M1; make configurable in M2.

### M1 Tick loop (single deterministic control loop)
Recommended periodic tick (e.g., every 1s or 5s):
1) Load events snapshot (already polled in VEN app) fileciteturn0file0  
2) Reactor computes setpoints from active events
3) Simulator updates actor states with inertia (SOC/temp)
4) Power model computes net power and derived telemetry
5) Energy integration updates kWh
6) Persist state every N seconds (e.g., 10–30s) and on shutdown

### M1 “Definition of Done”
- On restart, SOC/temp/kWh continue (no reset surprise)
- An export-cap event visibly causes:
  - EV/heater increases and/or PV curtailment
  - net export stays under cap (within tolerance)
- A price event visibly causes:
  - EV/heater shifts consumption up/down

---

## Milestone 2 (M2): “Behavior profiles + ramp/delay/partial + arbitration + explainability”

### Goal
Make the VEN feel like real-world behavior:
- not perfect compliance
- not instantaneous response
- clear reasons for decisions
- consistent results across restarts

### M2 Scope
1) **Reaction strategy profiles (per VEN instance)**
   - Implement the strategies listed in your concept: ramp / delayed / partial / ignore. fileciteturn0file1
   - Add config file per VEN (YAML) mounted via Docker volume:
     - device mix + limits
     - reaction strategy + parameters
     - price thresholds
     - export/import cap preferences
     - comfort/deadline constraints (basic)

2) **Reactor finite-state machine**
For each “winning” control objective, implement:
- `IDLE`
- `DELAYING` (optional)
- `RAMPING_TO_TARGET`
- `HOLDING`
- `RAMPING_BACK`
- (optional) `OVERRIDDEN` if local constraints prevent compliance

Store the FSM’s current ramp progress in persisted `state.json` so restarts don’t jump.

3) **Event arbitration**
When multiple events overlap:
- Choose “winning” event by:
  1) explicit OpenADR priority if present (lower number = higher priority)
  2) “hard constraints” (import/export caps) override incentives (price)
  3) tie-break: most recently started event or highest severity (SIMPLE if you add it later)

Note: For M2 you still can keep the supported signal types small; arbitration logic should be general.

4) **Explainability / decision trace**
Add a lightweight in-memory ring buffer + optional persisted “last decision”:
- timestamp
- active events summary
- chosen mode
- constraints binding (e.g., “EV max 7kW”, “PV min OLS 0.3”)
- setpoints chosen
- whether partial/delayed/ignored and why

Expose it in VEN UI (“Decision Trace” tab) and/or via a simple JSON endpoint.

5) **Configurable actor realism (still simple)**
- EV charging curve (optional): taper near 100% SOC
- Heater inertia: temperature changes slowly, bounded by ambient loss/gain
- PV production: daily sinusoid + noise, with occasional cloud dips

### M2 Configuration file (suggested schema)
Example `profiles/ven-2.yaml`:
```yaml
devices:
  ev:
    enabled: true
    max_charge_kw: 7.4
    initial_soc: 0.40
    soc_min: 0.20
    soc_target: 0.80
    plug_schedule:
      arrive: "18:30"
      depart: "07:30"
  heater:
    enabled: true
    max_kw: 3.0
    temp_initial_c: 20.5
    temp_min_c: 19.0
    temp_max_c: 22.0
  pv:
    enabled: true
    rated_kw: 8.0

reactor:
  strategy: "ramp"          # ramp|delayed|partial|ignore
  ramp_duration_s: 300      # 5 min
  delay_s: 0
  compliance: 0.8           # partial compliance
  price:
    low: 0.10               # EUR/kWh
    high: 0.35

simulator:
  tick_s: 1
  persist_every_s: 15
```

### M2 Reactor decision logic (more realistic but still deterministic)

#### Step 1: Determine active controls
- parse all active events → list of active payloads for current time
- map payloads into “control intents”:
  - export cap intent
  - import cap intent (optional later)
  - price intent

#### Step 2: Select winning intent (arbitration)
- hard caps win over price
- use event priority if present
- compute a single “target setpoint vector” for this tick

#### Step 3: Apply behavior strategy
- `ignore`: do nothing
- `partial`: move only `compliance` fraction toward target
- `delayed`: wait `delay_s` after event start before moving
- `ramp`: interpolate from current setpoints to target over ramp_duration

#### Step 4: Enforce local constraints
- clamp to device max/min
- maintain comfort bounds (heater temp)
- maintain deadlines (optional): e.g., ensure EV can still reach target SOC by departure

If constraints prevent full compliance, record reason in decision trace.

### M2 Persistence rules (still JSON)
Persist at minimum:
- energy totals + last tick timestamp
- device states (SOC/temp)
- current applied setpoints
- reactor mode + FSM ramp progress
- last active event/interval IDs (optional)

Do **not** persist full event lists; re-fetch from VTN.

### M2 “Definition of Done”
- Different VENs behave differently based on their profile (ramp vs delayed vs partial)
- Restart does not cause step jumps (ramp continues or cleanly re-initializes)
- Decision trace makes it clear *why* a VEN chose its actions
- Export cap and price behaviors remain correct and stable even with overlapping events

---

## Suggested implementation sequence (task breakdown)

### Step A: Create simulator modules
- `simulator/actor.rs`: EV, heater, PV structs + update(dt)
- `simulator/power_model.rs`: compute net power + export/import
- `simulator/sensors.rs`: build telemetry snapshot from SimState
- `simulator/persist.rs`: load/save `state.json`

### Step B: Integrate into existing VEN loop
- replace placeholder sensor with simulator snapshot generation
- add tick loop and persistence cadence

### Step C: Add reactor modules
- `reactor/mod.rs`: compute setpoints from events
- `reactor/fsm.rs`: implement ramp/delay states (M2)
- `reactor/arbitration.rs`: choose winning intent (M2)
- `reactor/trace.rs`: decision trace buffer (M2)

### Step D: Minimal UI additions
- show device state and active setpoints
- show energy counter
- show last decision trace entries

---

## Practical notes & guardrails

- Keep everything **deterministic** by default:
  - seed RNG per VEN profile so noise is repeatable
- Make sign conventions explicit:
  - recommended: `net_power_w > 0` = import; `net_power_w < 0` = export
- Ensure unit consistency:
  - OpenADR capacity events often expressed in kW in examples; normalize to W internally.
- Avoid event thrash:
  - in M2, if intervals change frequently, ramping prevents “setpoint chatter”.

---

## Summary

- **M1** delivers believable simulated devices + energy counter + minimal reactor (export cap + price).
- **M2** adds per-VEN profiles, ramp/delay/partial strategies, event arbitration, and decision explainability.
- Persistence is done via **per-VEN JSON state file** (no SQLite), which is sufficient for continuity and quick debugging.

