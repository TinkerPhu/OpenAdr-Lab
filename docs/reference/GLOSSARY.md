# OpenADR Lab - Glossary

## Organizations & Roles

| Term | Full Name | Explanation |
|---|---|---|
| **Utility** | Electric Utility | Company that generates, transmits, and/or distributes electricity to customers. Operates the grid and runs demand response programs. Examples: PG&E, E.ON, ConEdison. |
| **DSO** | Distribution System Operator | Entity responsible for operating the local electricity distribution network. In Europe, DSOs are often separate from energy retailers. They manage grid stability at the distribution level. |
| **TSO** | Transmission System Operator | Entity responsible for the high-voltage transmission grid. Coordinates large-scale grid balancing. Examples: TenneT, Amprion, National Grid ESO. |
| **Aggregator** | DR Aggregator | Company that bundles many small DER/load resources from multiple customers into a single portfolio large enough to participate in wholesale energy markets or utility DR programs. Acts as intermediary between the utility/DSO and end customers. |
| **Prosumer** | Producer + Consumer | End customer that both consumes and produces electricity (e.g. a home with solar panels and a battery). |

## OpenADR Protocol

| Term | Full Name | Explanation |
|---|---|---|
| **OpenADR** | Open Automated Demand Response | Open standard protocol for communicating DR signals between utilities/aggregators and customer energy management systems. Current version is OpenADR 3. |
| **VTN** | Virtual Top Node | The server side of OpenADR. Operated by the utility, DSO, or aggregator. Creates programs and sends events (DR signals) to VENs. Receives reports back. |
| **VEN** | Virtual End Node | The client side of OpenADR. Runs at the customer site (building, factory, EV fleet). Receives events from the VTN, controls local devices, and reports back telemetry. |
| **BFF** | Backend For Frontend | An API proxy between the VTN UI and the VTN. Handles authentication, caching, and simplifies the VTN API for the web frontend. Not part of the OpenADR spec — an architectural pattern used in this lab. |

## HEMS — Home Energy Management System

| Term | Full Name | Explanation |
|---|---|---|
| **HEMS** | Home Energy Management System | Software and hardware system that monitors and controls energy flows within a home or small site. Coordinates DERs (solar, battery, EV charger, HVAC) to minimise cost, maximise self-consumption, or respond to DR signals from a VTN. In this lab, the HEMS controller runs inside the VEN. |
| **Planner** | Energy Planner | Component of the HEMS that schedules future energy use. Given rate forecasts, device constraints, and DR obligations, it builds an optimal time-slot plan (FIRM + FLEXIBLE slots) by solving a Mixed-Integer Linear Program (MILP) over a 24-hour horizon. |
| **Dispatcher** | Setpoint Dispatcher | Real-time control loop (1 s tick) that translates the planner's slot schedule into live device setpoints and accumulates the asset ledger. |
| **Asset Ledger** | — | Cumulative energy accounting record maintained by the Dispatcher. Tracks how much energy each asset imported/exported and the associated cost/revenue. |
| **Energy Packet** | — | A schedulable unit of energy delivery: a fixed amount of energy (kWh) for a specific asset, with a time window and status (PENDING → ACTIVE → COMPLETED / ABANDONED). |
| **FIRM slot** | Firm Commitment Slot | A planner output slot that must be executed — typically driven by a hard user request or minimum SOC constraint. Cannot be deferred. |
| **FLEXIBLE slot** | Flexible Opportunity Slot | A planner output slot that can be shifted or cancelled if constraints change. Typically price-driven charging windows. |
| **Rate Snapshot** | — | A point-in-time price signal parsed from a VTN event payload. Contains start time, duration, import price ($/kWh), and export price. |
| **Capacity State** | — | Current grid capacity constraints parsed from a VTN event: import limit (kW) and export limit (kW) the VEN must not exceed. |
| **User Request** | — | An explicit energy delivery request submitted by the occupant (e.g. "charge EV to 80% by 07:00"). The planner honours these as FIRM slots. Supports ASAP, BY_DEADLINE, and SCHEDULED modes. |
| **SOC target** | State-of-Charge Target | Desired battery or EV charge level (%) at a given deadline. The planner back-calculates the required charging power and window to meet it. |
| **FlexibilityEnvelope** | — | The range of power the HEMS can flex (increase or decrease) in each time slot, as seen from the grid. Exposed to aggregators to let them predict available DR capacity. |
| **PV** | Photovoltaic (Solar) | Solar panel array modelled in the VEN simulator. Output follows a `sin(π*(hour-6)/12)` curve between 06:00–18:00; zero otherwise. Sign convention: negative = export (generation). |
| **Sign convention** | Grid Sign Convention | Positive = power imported from grid; negative = power exported to grid. Applies uniformly to setpoints, ledger entries, and reports in this lab. |

## Energy & Grid Concepts

| Term | Full Name | Explanation |
|---|---|---|
| **DR** | Demand Response | Strategy where electricity consumers reduce or shift their energy usage in response to grid signals (e.g. high prices, grid emergencies). Instead of building more power plants, utilities pay customers to use less during peak times. |
| **DER** | Distributed Energy Resource | Any small-scale energy generation or storage device connected to the distribution grid. Includes solar panels, batteries, EV chargers, small wind turbines, controllable loads (HVAC, water heaters). |
| **HVAC** | Heating, Ventilation, and Air Conditioning | Building climate control system. One of the most energy-intensive systems in commercial buildings, making it a prime target for DR programs. |
| **EV** | Electric Vehicle | Vehicle powered by electricity. EV chargers are significant controllable loads — a single Level 2 charger draws 7-19 kW, a DC fast charger up to 350 kW. |
| **SOC** | State of Charge | Battery charge level expressed as a percentage (0-100%). Used in EV and stationary battery reports. |
| **Baseline** | Demand Baseline | The expected energy consumption of a customer without any DR intervention. Used to calculate how much load was actually reduced during a DR event. |
| **Curtailment** | Load Curtailment | Actively reducing electricity consumption in response to a DR signal. Can be voluntary (price-based) or mandatory (grid emergency). |
| **Load Shedding** | — | Controlled, deliberate reduction of electrical load to prevent grid overload. More aggressive than curtailment — may involve disconnecting entire circuits. |
| **M&V** | Measurement & Verification | Process of verifying that DR actually delivered the promised load reduction. Uses protocols like IPMVP to compare actual consumption against the baseline. |

## ISO 8601 Duration Syntax

OpenADR uses ISO 8601 duration format for all time periods. The format is `P[n]Y[n]M[n]DT[n]H[n]M[n]S` where `P` marks the start and `T` separates date from time components.

| Duration | Meaning | Typical use in OpenADR |
|---|---|---|
| `PT5M` | 5 minutes | Short reporting interval |
| `PT15M` | 15 minutes | Standard DR reporting interval |
| `PT30M` | 30 minutes | Common event interval |
| `PT1H` | 1 hour | Typical DR event duration |
| `PT2H` | 2 hours | Extended DR event |
| `PT4H` | 4 hours | Peak period event |
| `P1D` | 1 day | Day-ahead pricing program |
| `P1M` | 1 month | Program duration |
| `P1Y` | 1 year | Annual program span |

**Combining components:** `P1DT2H30M` = 1 day, 2 hours, and 30 minutes.

**Key rule:** `M` means months before the `T`, and minutes after the `T`:
- `P2M` = 2 months
- `PT2M` = 2 minutes

## Units

| Unit | Meaning | Context |
|---|---|---|
| **W** | Watts | Power (instantaneous energy flow) |
| **kW** | Kilowatts (1,000 W) | Typical building/device power. A home draws ~2-5 kW, an office building 50-500 kW. |
| **MW** | Megawatts (1,000 kW) | Grid-scale power. A DR program might target 10-100 MW of reduction. |
| **kWh** | Kilowatt-hours | Energy (power × time). Running 10 kW for 1 hour = 10 kWh. |
| **V** | Volts | Voltage. Residential: 230V (EU) / 120V (US). |
| **A** | Amps | Current. Power = Voltage × Current. |
| **kVA** | Kilovolt-Amperes | Apparent power (includes reactive component). |
| **kVAR** | Kilovolt-Amperes Reactive | Reactive power. Important for grid stability but doesn't do useful work. |
