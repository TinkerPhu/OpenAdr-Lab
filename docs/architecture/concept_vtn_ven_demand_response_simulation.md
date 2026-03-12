# Concept: Realistic Telemetry Simulation & Operator Analytics

## Context

The current VEN implementation uses a placeholder fake sensor that generates `power_w = timestamp % 100`. This concept designs a realistic simulation layer where:

- VENs produce configurable telemetry (base values + variance)
- VENs interpret events and react with configurable behavior profiles
- A simulated power model connects telemetry quantities and tracks energy
- A VTN-side operator module analyzes reports and calculates event effectiveness
- The operator can simulate grid conditions via UI controls that auto-generate appropriate OpenADR events

These are independent simulation modules: the VEN simulator replaces the fake sensor inside each VEN, while the VTN operator is a separate application consuming the BFF API.

---

## OpenADR 3.0 Signal Types (openleadr-rs EventType Enum)

The openleadr-rs implementation supports a rich set of event types, defined in `openleadr-wire/src/event.rs`:

### Price Signals
| EventType | Value Type | Description |
|---|---|---|
| `PRICE` | float | Import electricity price (e.g. EUR/kWh) |
| `EXPORT_PRICE` | float | Export electricity price |

### Control Signals
| EventType | Value Type | Description |
|---|---|---|
| `SIMPLE` | integer (0-3) | Level-based DR signal: 0=normal, 1=moderate, 2=high, 3=special/emergency |
| `DISPATCH_SETPOINT` | float | Absolute power setpoint (watts) |
| `DISPATCH_SETPOINT_RELATIVE` | float | Relative power change (watts, can be negative) |
| `CONTROL_SETPOINT` | depends | Generic control setpoint |
| `CHARGE_STATE_SETPOINT` | float | Battery/EV target state of charge |
| `OLS` | float (0.0-1.0) | Operating Limit Setpoint — fraction of rated power |
| `CURVE` | point pairs | Volt-var or other characteristic curves |

### Capacity Management (Dynamic Operating Envelopes)
| EventType | Value Type | Description |
|---|---|---|
| `IMPORT_CAPACITY_LIMIT` | float | Max import power (kW) — hard constraint |
| `EXPORT_CAPACITY_LIMIT` | float | Max export power (kW) — hard constraint |
| `IMPORT_CAPACITY_SUBSCRIPTION` | float | Subscribed import capacity |
| `IMPORT_CAPACITY_RESERVATION` | float | Reserved import capacity |
| `IMPORT_CAPACITY_RESERVATION_FEE` | float | Fee for capacity reservation |
| `IMPORT_CAPACITY_AVAILABLE` | float | Available import capacity |
| `IMPORT_CAPACITY_AVAILABLE_PRICE` | float | Price for available capacity |
| `EXPORT_CAPACITY_SUBSCRIPTION` | float | Subscribed export capacity |
| `EXPORT_CAPACITY_RESERVATION` | float | Reserved export capacity |
| `EXPORT_CAPACITY_RESERVATION_FEE` | float | Fee for export capacity reservation |
| `EXPORT_CAPACITY_AVAILABLE` | float | Available export capacity |
| `EXPORT_CAPACITY_AVAILABLE_PRICE` | float | Price for available export capacity |

### Environmental
| EventType | Value Type | Description |
|---|---|---|
| `GHG` | float | Greenhouse gas intensity (g CO2/kWh) |

### Alerts
| EventType | Value Type | Description |
|---|---|---|
| `ALERT_GRID_EMERGENCY` | string | Grid emergency — human-readable |
| `ALERT_BLACK_START` | string | Black start event |
| `ALERT_POSSIBLE_OUTAGE` | string | Possible outage warning |
| `ALERT_FLEX_ALERT` | string | Flexibility alert |
| `ALERT_FIRE` | string | Fire alert |
| `ALERT_FREEZING` | string | Freezing alert |
| `ALERT_WIND` | string | Wind alert |
| `ALERT_TSUNAMI` | string | Tsunami alert |
| `ALERT_AIR_QUALITY` | string | Air quality alert |
| `ALERT_OTHER` | string | Other alert |

### Device-Specific
| EventType | Value Type | Description |
|---|---|---|
| `CTA2045_REBOOT` | integer | 0=SOFT, 1=HARD reboot |
| `CTA2045_SET_OVERRIDE_STATUS` | integer | 0=No Override, 1=Override |
| `Private(String)` | any | Custom/private event types (1-128 chars) |

### Event Structure (from openleadr-rs)

An event contains:
- `programID` — which program this event belongs to
- `eventName` — human-readable name
- `priority` — lower number = higher priority (None = lowest)
- `targets` — which VENs this event applies to
- `payloadDescriptors` — context for interpreting values (units, currency)
- `intervalPeriod` — default start time and duration
- `intervals[]` — list of time intervals, each containing `payloads[]`
- Each payload: `{ type: EventType, values: [Value] }`

Example from openleadr-rs test fixtures — dynamic pricing (24 hourly intervals):
```yaml
# dyn-price.oadr.yaml
events:
  - id: dp101-e0
    programID: dp101
    payloadDescriptors:
      - payloadType: PRICE
        currency: EUR
        units: KWH
    intervalPeriod:
      start: 2024-01-01T00:00Z
      duration: PT1H
    intervals:
      - id: 0
        payloads:
          - type: PRICE
            values: [ 0.42 ]
      - id: 1
        payloads:
          - type: PRICE
            values: [ 0.43 ]
      # ... 24 intervals total
```

Example — simple load shedding:
```yaml
# load-sched.oadr.yaml
events:
  - id: ls101-e0
    programID: ls101
    payloadDescriptors:
      - payloadType: SIMPLE
    intervalPeriod:
      start: 2024-01-01T13:37Z
      duration: PT4H
    intervals:
      - id: 0
        payloads:
          - type: SIMPLE
            values: [ 1 ]
```

---

## VEN Motivation: Not Always Cost Optimization

The VEN's motivation depends on who operates it and under which program:

| VEN Operator | Primary Motivation | Responds to |
|---|---|---|
| Commercial building | Cost optimization | PRICE, EXPORT_PRICE |
| Industrial site | Contractual obligation (DR program) | SIMPLE (levels 0-3) |
| PV + battery owner | Self-consumption + revenue | PRICE, EXPORT_PRICE, ExportCapacityLimit |
| EV fleet operator | Minimize fleet charging cost | PRICE, ChargeStateSetpoint |
| DSO-contracted DER | Grid service obligation | ImportCapacityLimit, ExportCapacityLimit, Curve |
| Any | Avoid penalties / comply with law | AlertGridEmergency, capacity limits |

The reactor's decision logic isn't just "minimize cost" — it's **"optimize my objective function given the signal type."** A VEN in a mandatory DR program *must* respond to SIMPLE level 3 regardless of cost.

---

## Demand Response Use Case Examples

### a) Peak Shaving — Price & Load Signals

**Grid condition:** High load on transformer / peak demand
**VTN signals:** `PRICE` (high import price) + `EXPORT_PRICE` (high export price) or `SIMPLE` level 2-3
**VEN actions:**
- Switch on thermal storage (pre-heat/pre-cool)
- Reduce discretionary loads (HVAC setback, dim lighting)
- Lower PV inverter export limit (keep energy local)
- EV: pause charging or discharge (V2G)

### b) Grid Needs Energy / Excess Renewables

**Grid condition:** Too much solar midday, not enough demand
**VTN signals:** `PRICE` (very low or negative import price) + `EXPORT_PRICE` (low, discouraging export)
**VEN actions:**
- Switch ON flexible loads (EV charging, water heating, battery charging)
- Increase consumption to absorb excess
- Reduce PV export (curtail or store)

**Inverse:** Grid needs energy (shortage at evening peak)
**VTN signals:** `PRICE` (high import) + `EXPORT_PRICE` (high, rewarding export)
**VEN actions:**
- Shed load, shift to later
- Maximize PV + battery export
- EV V2G discharge

### c) Reactive Power / cos(phi) — Voltage Management

This is a real and growing use case. The `Curve` event type in OpenADR 3.0 is specifically designed for **volt-var curves** (Q(U) characteristics). German grid code VDE-AR-N 4105 already requires PV inverters to provide reactive power support.

**Grid condition:** High voltage on local feeder (too much PV injection midday)
**VTN signals:** `Curve` payload with volt-var points, e.g. `[{x: 1.05, y: -0.3}, {x: 1.02, y: 0.0}, {x: 0.98, y: 0.0}, {x: 0.95, y: 0.3}]`
**VEN actions:** PV inverter adjusts reactive power output based on measured local voltage against the curve. At V=1.06 pu, inject inductive reactive power (consume VAr) to pull voltage down.

Also: `OLS` (Operating Limit Setpoint, 0.0-1.0) can be used to curtail active power as a voltage management tool.

### d) EV Managed Charging — ChargeStateSetpoint

**Grid condition:** Overnight valley, cheap wind energy
**VTN signals:** `ChargeStateSetpoint` (target 80% SoC by 7:00) + `PRICE` schedule
**VEN action:** Optimize charging within time window to hit target at lowest cost. May pause/resume multiple times.

### e) Dynamic Operating Envelopes — Capacity Limits

**Grid condition:** Transformer at 90% capacity in a neighborhood
**VTN signals:** `ImportCapacityLimit` = 3.0 kW per household (normally 10 kW) + `ExportCapacityLimit` = 5.0 kW
**VEN action:** Hard constraint — VEN must throttle consumption/export to stay within envelope. Very real in Australia, being adopted by Dutch DSOs with OpenADR 3.0.

### f) GHG Signal — Carbon-Aware Scheduling

**Grid condition:** High-carbon marginal generator online (gas peaker)
**VTN signals:** `GHG` = 450 g CO2/kWh (high)
**VEN action:** Shift discretionary load to lower-carbon periods. Not price-based, purely environmental / corporate ESG compliance.

### g) Grid Emergency — Mandatory Curtailment

**Grid condition:** Frequency dropping, risk of blackout
**VTN signals:** `AlertGridEmergency` + `SIMPLE` level 3 (maximum)
**VEN action:** Immediate load shed, non-negotiable. No economic optimization — just comply.

### h) Dispatch Setpoint — Direct Power Control (Virtual Power Plant)

**Grid condition:** Aggregator needs exactly 2 MW reduction from portfolio
**VTN signals:** `DispatchSetpoint` = 3000 (watts) or `DispatchSetpointRelative` = -2000
**VEN action:** Set consumption to exactly 3 kW, or reduce by 2 kW from baseline. Used in virtual power plant aggregation.

### i) Frequency Response — Operating Limit Setpoint

**Grid condition:** Frequency deviation requires fast response
**VTN signals:** `OLS` = 0.5 (reduce to 50% of rated power)
**VEN action:** Immediately reduce consumption/generation to 50% of rated capacity. VTN modulates OLS for frequency regulation.

### j) Capacity Reservation — VEN Requests Extra Import Capacity

This is a **VEN-initiated** pattern where the VEN requests additional grid capacity beyond its base subscription, and the VTN grants, denies, or grants at a fee.

**Grid condition:** Normal operations, some local capacity available
**VEN action (initiates):** VEN submits report with `IMPORT_CAPACITY_RESERVATION` = 11.0 kW for PT2H + `IMPORT_CAPACITY_RESERVATION_FEE` (willingness to pay)
**VTN action:** Evaluates local transformer load, either grants (creates matching event) or denies (no event / event with reduced capacity)
**VEN response:** Sees granted event, proceeds with planned high-consumption activity (e.g., fast EV charging)

The flow uses OpenADR 3.0's capacity management event types natively:

```
VEN: "I need 11 kW for 2 hours starting at 18:00"
  -> POST /reports with IMPORT_CAPACITY_RESERVATION = 11.0 kW
     and IMPORT_CAPACITY_RESERVATION_FEE (how much VEN is willing to pay)

VTN evaluates grid capacity...

VTN: "Granted" or "Granted at a fee" or "Denied"
  -> Creates event with IMPORT_CAPACITY_RESERVATION = 11.0 (approved amount)
     and IMPORT_CAPACITY_RESERVATION_FEE = 0.05 EUR/kWh (actual fee)

VEN sees the event -> proceeds to consume (or not, if fee too high)
```

The request is accepted when the VTN creates equivalent events — the VEN polls for events and sees the grant. This is documented in OpenADR 3.0: "Customers subscribe to a defined power limit. If a customer wants to charge faster, a digital automated request can be made to the utility for extra capacity for a specific time duration. The request may be just granted; it may be available but for a fee; or if there is no capacity available it will not be granted."

**Why it's effective for the grid operator:**

| Benefit | Explanation |
|---|---|
| Predictability | Instead of surprise load spikes, operator sees requests in advance |
| Revenue | Capacity reservation fees during peak times monetize scarce grid capacity |
| Fairness | Subscribed base capacity is guaranteed; extra capacity is market-priced |
| Grid investment deferral | Managing peaks via reservations avoids transformer upgrades |
| Congestion management | Can deny requests when local transformer is at limit |

Real-world adoption: Dutch DSOs are implementing this with OpenADR 3.0 for EV charge point operators. Australia uses the related `IMPORT_CAPACITY_LIMIT` / `EXPORT_CAPACITY_LIMIT` for similar purposes.

### k) Flexible Energy Budget — VEN Expresses Need, VTN Returns Optimized Schedule

This is the most sophisticated VEN-initiated pattern. The VEN doesn't request a fixed power level — it expresses an **energy need with constraints** and lets the VTN optimize the power-over-time schedule.

**Example scenario:** An EV needs 20 kWh before 06:00 tomorrow. It can take this as 8h at 2.5 kW, or 2h at 10 kW, or any combination in between. The VTN knows the grid conditions (overnight wind surplus, morning peak to avoid) and returns an optimized charging schedule.

**Grid condition:** Variable — operator has overnight wind surplus but expects morning peak
**VEN action (initiates):** VEN submits report expressing flexible energy need:

```json
{
  "reportName": "flex-request-ven-1",
  "payloads": [
    { "type": "ENERGY_NEED_KWH", "values": [20.0] },
    { "type": "DEADLINE", "values": ["2026-02-13T06:00:00Z"] },
    { "type": "POWER_MIN_W", "values": [2500] },
    { "type": "POWER_MAX_W", "values": [11000] }
  ]
}
```

**VTN action:** Runs optimization considering grid load forecast, other VEN requests, renewable forecast, and price signals. Returns an event with `DISPATCH_SETPOINT` intervals:

```json
{
  "eventName": "charge-schedule-ven-1",
  "payloadDescriptors": [{ "payloadType": "DISPATCH_SETPOINT", "units": "W" }],
  "intervalPeriod": { "start": "2026-02-12T22:00Z", "duration": "PT1H" },
  "intervals": [
    { "id": 0, "payloads": [{ "type": "DISPATCH_SETPOINT", "values": [0] }] },
    { "id": 1, "payloads": [{ "type": "DISPATCH_SETPOINT", "values": [0] }] },
    { "id": 2, "payloads": [{ "type": "DISPATCH_SETPOINT", "values": [5000] }] },
    { "id": 3, "payloads": [{ "type": "DISPATCH_SETPOINT", "values": [5000] }] },
    { "id": 4, "payloads": [{ "type": "DISPATCH_SETPOINT", "values": [5000] }] },
    { "id": 5, "payloads": [{ "type": "DISPATCH_SETPOINT", "values": [5000] }] },
    { "id": 6, "payloads": [{ "type": "DISPATCH_SETPOINT", "values": [0] }] },
    { "id": 7, "payloads": [{ "type": "DISPATCH_SETPOINT", "values": [0] }] }
  ]
}
```

This schedules 4 hours at 5 kW = 20 kWh, placed in the overnight wind surplus valley (00:00-04:00), avoiding the evening peak (22:00-00:00) and morning peak (06:00+).

**OpenADR 3.0 coverage:** OpenADR 3.0 has the building blocks (reports for requests, events with `DISPATCH_SETPOINT` for the response) but doesn't define a standardized schema for the flex request itself. The request payload uses `Private(String)` event types (`ENERGY_NEED_KWH`, `DEADLINE`, etc.) which is within OpenADR's extensibility model.

In a full production stack, this pattern is typically handled by complementary protocols:
- **OSCP** (Open Smart Charging Protocol) — DSO provides 24h capacity forecasts, CPO requests adjustments
- **OCPP 2.0/2.1** — translates the schedule into charger-level smart charging profiles
- **ISO 15118** — EV tells charger its energy need and departure time

The full protocol stack layers as:
```
EV <--ISO 15118--> Charger <--OCPP--> CPO/CPMS <--OSCP--> DSO
                                          |
                                     <--OpenADR--> VTN/Aggregator
```

For our lab simulation, we model this entirely within OpenADR using private payload types, since we control both VEN and VTN.

**Why it's effective for the grid operator:**

| Benefit | Explanation |
|---|---|
| Maximum optimization headroom | "20 kWh by 06:00" gives 8 hours to shape load vs. VEN just consuming 10 kW immediately |
| Valley filling | Schedule EV charging into overnight wind surplus periods |
| Peak avoidance | Avoid allocating power during 18:00-20:00 evening peak |
| Aggregation | With 100 VENs each expressing flexible needs, operator solves a global optimization |
| Grid investment deferral | Transformer handling 100 simultaneous 10 kW chargers needs upgrading; 100 chargers scheduled across 8h does not |
| Renewable integration | Align consumption with forecasted solar/wind production |

### Comparison: Price Signal vs. Capacity Reservation vs. Flexible Energy Budget

| Aspect | Price signal (a,b) | Capacity reservation (j) | Flexible energy budget (k) |
|---|---|---|---|
| Who initiates? | VTN broadcasts | VEN requests | VEN requests |
| Signal type | Incentive/hint | Fixed power for fixed time | Energy need + constraints |
| Guarantee? | None — VEN may or may not react | Explicit grant/deny | Optimized schedule returned |
| VEN autonomy | Full — VEN decides how to react | Partial — VEN chooses when to request | Minimal — VTN decides the schedule |
| Grid operator visibility | Post-facto (sees in reports) | Pre-facto (sees demand before it happens) | Pre-facto + optimization authority |
| Best for | General load shifting | Specific large loads (EV fast charge) | Flexible loads with energy targets (overnight EV, thermal storage) |
| Optimization potential | Low (each VEN optimizes locally) | Medium (operator can deny/delay) | High (operator shapes global schedule) |

---

## VEN Module Architecture

### Separation: Reactor, Sensor Simulator, Actor Simulator

The reactor is required in any case (with real or simulated sensors), so it must be cleanly separated from the simulation layer.

```
VEN/src/
  reactor/              <-- ALWAYS present, even with real sensors
    mod.rs              -- watches events, decides actions, outputs setpoints
    strategy.rs         -- fast/medium/slow/delayed/ignore reaction profiles

  simulator/            <-- ONLY for simulated environments
    sensor.rs           -- base + variance telemetry generation
    actor.rs            -- simulated actuators (heater, EV, inverter)
    power_model.rs      -- power = f(actuator states), energy counter

  (future: real/)       <-- for real device integration
    mqtt.rs             -- real sensor via MQTT
    modbus.rs           -- real actuator via Modbus
```

The **reactor** is the brain — it interprets events and decides what to do. It outputs **setpoints** (target temperature, charge rate, power limit, cos phi).

The **simulator** takes those setpoints and produces fake telemetry that *looks like* a real device responding. The **actor** simulates physical inertia (HVAC doesn't cool instantly, EV battery has charge curves).

With real devices, the reactor would send setpoints to actual hardware instead.

### Why Internal Modules, Not a Sidecar

For VENs, internal modules (not a separate app) are recommended because:
- The reactor needs real-time access to `AppState.events` (already in memory)
- The simulator feeds directly into `SensorSnapshot` (same process)
- A sidecar would need IPC for every 10-second tick — overhead for no gain
- The configuration (YAML profile) provides the external customization

The key insight: the separation is at the **module level** (clean Rust crate boundaries), not the **process level**. Each VEN instance runs one binary but with a different profile YAML.

### A1. Telemetry Profile (Base + Variance)

Each VEN gets a YAML config file defining its "device personality":

```yaml
# profiles/ven-1.yaml
device_type: "HVAC"
quantities:
  temperature_c:
    base: 21.5
    variance: 0.8        # random +/-0.8
    drift_per_hour: 0.0   # no natural drift
  voltage_v:
    base: 230.0
    variance: 2.0
  power_w:
    base: 4200.0          # 4.2 kW nominal
    variance: 150.0
```

On each tick (10s), the simulator produces:
`value = base + random_uniform(-variance, +variance) + drift`

This replaces the current `(timestamp % 100)` placeholder.

### A2. Event Reactor (Behavior Profiles)

The reactor watches `AppState.events` and modifies the active telemetry profile based on a **reaction strategy**:

```yaml
# profiles/ven-1.yaml (continued)
reaction:
  strategy: "ramp"       # ramp | delayed | partial | ignore
  ramp_speed: "medium"   # fast (30s) | medium (5min) | slow (15min)
  delay_secs: 0           # additional delay before reacting
  compliance: 1.0         # 0.0 = ignore, 0.5 = half effect, 1.0 = full
```

When an event becomes active (current time within interval):
1. Reactor reads event signal (e.g., "reduce by 50%")
2. Applies strategy: ramp gradually, delay, partial compliance, or ignore
3. Modifies the effective `base` for affected quantities
4. When event ends: ramp back to original base

**State machine per event:**

```
IDLE --> [event active] --> RAMPING_DOWN --> HOLDING --> [event ends] --> RAMPING_UP --> IDLE
        [delay if configured]
```

### A3. Power Model & Energy Counter

Power is the "master" quantity — other telemetry values connect to it via configurable factors:

```yaml
power_model:
  # power_w = base_power * product(factor adjustments)
  factors:
    temperature_c: -0.05   # +1 deg C --> -5% power (HVAC works less)
  energy_counter:
    initial_kwh: 0.0       # reset on startup or persist
```

Energy counter: simple integration — `energy_kwh += power_w * dt_hours` on each tick. Persisted via existing `PERSIST_PATH` mechanism.

The VEN API gets a new endpoint `GET /energy` returning `{ kwh: 42.7, since: "2026-02-12T..." }`.

### A4. Configuration Delivery

Profiles mounted as Docker volumes:

```yaml
# VEN/docker-compose.yml
ven-1:
  volumes:
    - ./profiles/ven-1.yaml:/config/simulator.yaml
  environment:
    SIMULATOR_PROFILE: "/config/simulator.yaml"
```

Each VEN gets a different personality — one HVAC, one EV charger, one generic load. Different reaction strategies make the simulation interesting.

---

## VTN Operator Module Architecture

### Why a Separate Application

The operator is a *consumer* of the system, not part of the infrastructure. It reads reports and events through the BFF like a human operator would. This keeps it decoupled and testable independently.

**Tech choice:** Python makes sense here — it's analytics/comparison work, not high-performance serving. It can use the BFF HTTP API directly.

```
VTN/operator/
  operator.py         -- main loop: poll reports + events, run analysis
  energy.py           -- energy accounting per VEN
  effectiveness.py    -- compare actual vs expected event impact
  config.yaml         -- BFF URL, poll intervals, expected effects
  Dockerfile          -- Python slim image
```

### Two Roles: Grid Simulator + Analytics Engine

The operator module serves two functions:

1. **Grid Simulator** (input side): UI with sliders/toggles representing grid conditions. Each condition maps to a rule that generates appropriate OpenADR events via BFF API. This is the "scenario engine."

2. **Analytics Engine** (output side): Reads VEN reports, calculates energy, compares with expected event effects.

```
+------------------------------------------------+
|           Operator Module (separate app)        |
|                                                  |
|  +----------------------+  +-------------------+ |
|  | Grid Simulator       |  | Analytics Engine  | |
|  |                      |  |                   | |
|  | UI: sliders,         |  | Report parser     | |
|  | toggles, presets     |  | Energy accounting | |
|  |         |            |  | Effectiveness     | |
|  |         v            |  |         ^         | |
|  | Rule engine:         |  |         |         | |
|  | condition --> event  |  | GET /api/reports  | |
|  |         |            |  +-------------------+ |
|  |         v            |                        |
|  | POST /api/events     |  Output: dashboard,   |
|  | via BFF              |  scores, energy totals |
|  +----------------------+  +-------------------+ |
+------------------------------------------------+
```

### Grid Condition Controls (Operator UI)

| Control | Maps to EventType(s) | VEN Effect |
|---|---|---|
| a) Energy price (slider: low - high) | `PRICE`, `EXPORT_PRICE` | Load shifting, self-consumption |
| b) Grid load (slider: low - high) | `SIMPLE` (0-3) or `DispatchSetpointRelative` | Load shedding / increase |
| c) Local voltage (slider: low - high) | `Curve` (volt-var) or `OLS` | Reactive power, active power curtailment |
| d) Renewable generation (slider: low - high) | `PRICE` (inverse), `GHG` | Absorb excess or shift away |
| e) Transformer capacity (slider: %) | `ImportCapacityLimit`, `ExportCapacityLimit` | Hard power envelope |
| f) Carbon intensity (slider: low - high) | `GHG` | Carbon-aware scheduling |
| g) Emergency toggle | `AlertGridEmergency` + `SIMPLE` 3 | Immediate mandatory curtailment |
| h) EV fleet target (SoC% + deadline) | `ChargeStateSetpoint` | Managed charging schedule |

### Presets vs. Free-Form

The operator UI could offer both:
- **Presets**: "Summer afternoon peak", "Winter evening shortage", "Voltage issue on feeder 3" — one click creates a realistic scenario
- **Free-form**: individual sliders for each grid parameter, operator crafts their own scenario

### Report Parser & Energy Accounting

The operator polls `GET /api/reports` via BFF, groups by VEN, and extracts power readings over time. It calculates:

- **Energy per VEN** (kWh): integrate power_w readings from report timestamps
- **Baseline energy**: expected energy without events (from program/VEN baseline config)
- **Actual energy during events**: energy consumed while events were active

### Event Effectiveness Analysis

For each completed event, the operator compares:

```
expected_reduction = event_signal * baseline_power * event_duration
actual_reduction   = baseline_energy - actual_energy_during_event
effectiveness      = actual_reduction / expected_reduction
```

This gives a per-VEN, per-event effectiveness score:
- `1.0` = perfect compliance
- `0.0` = no reaction
- `> 1.0` = over-performed
- `< 0.0` = increased consumption (rebellion)

### Operator Output

The operator exposes results via a lightweight API (FastAPI) on its own port (e.g., 8225). The VTN UI adds an "Operator Analytics" page that fetches from this API.

Endpoints:
- `GET /energy?ven=ven-1` — energy accounting
- `GET /effectiveness?event=<id>` — event effectiveness
- `GET /dashboard` — summary across all VENs

### Operator Configuration

```yaml
# VTN/operator/config.yaml
bff_url: "http://bff:8090"
poll_interval_secs: 60
vens:
  ven-1:
    baseline_power_w: 4200
    device_type: "HVAC"
  ven-2:
    baseline_power_w: 7500
    device_type: "EV_CHARGER"
  ven-3:
    baseline_power_w: 2000
    device_type: "GENERIC"
```

---

## Full Architecture Diagram

```
+---------------------------------------------------+
|                    VTN Stack                        |
|                                                     |
|  +-----------+  +-----------+  +------------------+ |
|  | VTN       |  | BFF       |  | Operator (NEW)   | |
|  | :8200     |<>| :8220     |<-| :8225            | |
|  |           |  |           |  | Python/FastAPI   | |
|  +-----------+  +-----------+  | - grid simulator | |
|       |                        | - energy acct    | |
|  +-----------+                 | - effectiveness  | |
|  | DB        |                 +------------------+ |
|  | :8201     |                                      |
|  +-----------+                 +------------------+ |
|                                | VTN UI :8221     | |
|                                | + analytics page | |
+--------------------------------+------------------+--+

        | openadr-net |

+---------------------------------------------------+
|                    VEN Stack                        |
|                                                     |
|  +----------------------------------+  x3 instances |
|  | VEN (with Simulator module)      |               |
|  | +----------------+ +----------+  |               |
|  | | Reactor        | | Sampler  |  |               |
|  | | - event watch  | | - snap   |  |               |
|  | | - setpoints    | | - report |  |               |
|  | +-------+--------+ +----------+  |               |
|  |         |                        |               |
|  | +-------v--------+              |               |
|  | | Simulator      |              |               |
|  | | - sensor sim   |              |               |
|  | | - actor sim    |              |               |
|  | | - power model  |              |               |
|  | | - energy ctr   |              |               |
|  | +----------------+              |               |
|  |  ^ config: profiles/ven-N.yaml  |               |
|  +----------------------------------+               |
+---------------------------------------------------+
```

---

## Summary of Architectural Decisions

| Question | Recommendation | Rationale |
|---|---|---|
| VEN simulator: internal module or sidecar? | **Internal module** | It replaces the sensor — same process, no IPC. Clean module boundary via `simulator/` directory. |
| VEN reactor: internal module or sidecar? | **Internal module** | Needs real-time access to `AppState.events` in memory. Universal (real or simulated). |
| VTN operator: inside BFF or separate? | **Separate app** | It's an analytics consumer, not infrastructure. Different language (Python), different concerns. |
| Operator talks to VTN how? | **Via BFF API** | Same interface as UI. No special privileges needed. |
| Profile config format? | **YAML files, Docker-mounted** | Easy to edit, one per VEN, no DB needed. |
| Operator output? | **Own API (FastAPI)** | VTN UI can fetch analytics; also usable standalone. |

---

## References

- [Top 5 use cases of OpenADR 3.0](https://codibly.com/blog/articles/5-use-cases-openadr-3-0)
- [OpenADR 3.0 and renewables integration (pv magazine)](https://pv-magazine-usa.com/2025/01/10/openadr-3-0-standard-can-maximize-demand-flexibility-to-help-integrate-renewables/)
- [OpenADR 3.0 for DER management (Smart Energy)](https://www.smart-energy.com/industry-sectors/distributed-generation/openadr-3-0-launched-for-distributed-energy-resource-management/)
- [Reactive power management with DERs (IEA PVPS)](https://iea-pvps.org/wp-content/uploads/2024/04/Reactive_power_management_with_DERs.pdf)
- [OpenADR Events reference (GridFabric)](https://plaid-docs.gridfabric.io/reference/openadr-events/)
- [Transforming Demand Response using OpenADR 3.0 (ACEEE)](https://www.aceee.org/sites/default/files/proceedings/ssb24/assets/attachments/20240722163109015_bb500916-3461-4580-816a-9ef02398162d.pdf)
- [OpenADR 3.0 official page](https://www.openadr.org/openadr-3-0)
- [OpenADR 3.1.0 changes](https://www.openadr.org/index.php?option=com_dailyplanetblog&view=entry&year=2025&month=09&day=17&id=100:what-you-need-to-know-about-the-latest-version-of-openadr-3-openadr-3-1-0-)
- [UK Demand Flexibility Service (NESO)](https://www.neso.energy/document/363911/download)
- [Ireland Demand Flexibility Product (ESB Networks)](https://esbnetworksprdsastd01.blob.core.windows.net/media/docs/default-source/publications/demand-flexibility-product-proposal-consultation-doc-oct-2024.pdf)
- [Flexibility Services via OpenADR at DSO Level (MDPI)](https://www.mdpi.com/1424-8220/20/21/6266)
- [Convention over specification — OpenADR 3.0 deep dive (Codibly)](https://codibly.com/blog/articles/open-adr-3-0-features)
- [OSCP — Open Smart Charging Protocol (Solidstudio)](https://solidstudio.io/blog/about-open-smart-charging-protocol-oscp)
- [OSCP explained (EVBoosters)](https://evboosters.com/ev-charging-academy/articles-blogs/oscp-explained/)
- [EV smart charging fundamentals — protocol stack (AMPECO)](https://www.ampeco.com/blog/what-every-cpo-needs-to-know-about-openadr/)
- openleadr-rs source: `openleadr-wire/src/event.rs` (EventType enum, EventPayloadDescriptor, EventValuesMap)
