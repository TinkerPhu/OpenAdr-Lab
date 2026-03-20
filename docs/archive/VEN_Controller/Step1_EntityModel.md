# Step 1 — Complete Entity Model: VEN Controller (HEMS)

**Scope:** Single-site residential Home Energy Management System acting as OpenADR 3.1 VEN.  
**Version:** Draft 6  
**Companion to:** VEN-Controller.md (original concept)

---

## 1. Enumerations

### 1.1 AssetType
```
PV                  // photovoltaic producer
BATTERY             // bidirectional storage
EV                  // electric vehicle (consumer, storage-like)
HEATER              // thermal consumer with storage characteristics
HEAT_PUMP           // thermal consumer with storage characteristics
WASHING_MACHINE     // batch consumer
COOKING_STOVE       // heuristic/uncontrollable consumer
SITE_RESIDUAL       // virtual asset: unmodeled site consumption (see §3.8)
GENERIC_CONSUMER    // fallback
GENERIC_PRODUCER    // fallback
```

### 1.2 PowerAdjustability
```
NONE                // no control (observe only)
ON_OFF              // binary switching (equivalent to STEPPED with [0, MaxPower])
STEPPED             // discrete power levels (e.g. 0/3/6 kW)
STEPLESS            // continuously adjustable within range
CROPPABLE           // can be curtailed downward only (e.g. PV)
RECOMMENDATION      // VEN can suggest but not enforce
```
Note: ON_OFF is handled by the same algorithm logic as STEPPED. The Planner treats
ON_OFF as STEPPED with PowerSteps = [0, MaxPower_kW]. No separate code path needed.

### 1.3 DeviceResponsiveness
```
RESPONSIVE          // device confirms setpoints within expected delay
DEGRADED            // device responds but outside expected parameters
UNRESPONSIVE        // device not confirming setpoint changes
OFFLINE             // device not communicating at all
```

### 1.4 EnergyPacketStatus
```
PENDING             // not yet started, waiting for optimal slot
SCHEDULED           // planned start time assigned
ACTIVE              // currently executing (energy flowing)
PAUSED              // temporarily suspended (by conflict, VTN, or user)
COMPLETED           // target energy/SoC reached (FillPercentage = 1.0)
PARTIAL_COMPLETED   // deadline reached with FillPercentage < 1.0 and CompletionPolicy = STOP
ABANDONED           // all tiers exhausted or user cancelled
FAILED              // device failure prevented completion
```

### 1.5 PlanTrigger
```
PERIODIC            // regular planning cycle (every PlanTimeStep)
RATE_CHANGE         // new PRICE/GHG/EXPORT_PRICE event from VTN
CAPACITY_CHANGE     // new capacity limit/reservation from VTN
ALERT               // emergency/flex alert from VTN
USER_REQUEST        // new or modified EnergyPacket from user
DEVICE_DEVIATION    // significant actual vs. planned deviation detected
ASSET_STATE_CHANGE  // device connected/disconnected/failed
```

### 1.6 FlexibilityDirection
Used **only** in OadrCapacityRequest to indicate what we're requesting from the VTN.
Not used on assets — asset flexibility is computed dynamically (see AssetFlexibility in §3.5).
```
IMPORT              // requesting additional import capacity from VTN
EXPORT              // requesting additional export capacity from VTN
```

### 1.7 RateType
```
PER_KWH             // €/kWh or gCO2/kWh — used in per-timeslot optimization
PER_KW              // €/kW — capacity-based rate (translated into constraints before optimization)
```
Note: Monthly/periodic charges (penalties, demand charges) are modeled as PenaltyRule (§6.6),
not as rates. They are discrete threshold decisions: the full penalty cost is attributed to
the allocation that would cross the threshold. See §6.6 for details.

### 1.8 RateUnit
The numerator unit of a rate — what is being measured.
```
EUR                 // euros
USD                 // US dollars
CHF                 // Swiss francs
g_CO2_eq            // grams CO2 equivalent (used for grid intensity rates, OpenADR GHG)
kg_CO2_eq           // kilograms CO2 equivalent (used for user-facing CO2 budgets)
```

### 1.9 UserRequestMode
```
ASAP                // as soon as possible, cost-aware
ASAP_FREE           // as soon as possible, only free energy
BY_DEADLINE         // complete by deadline, cost-aware
BY_DEADLINE_FREE    // complete by deadline, only free energy
MAX_COST            // complete whenever, but within cost limit
OPPORTUNISTIC       // use only free/surplus energy, no deadline
```
Note: heuristic-driven assets (e.g. cooking stove) do NOT use EnergyPackets.
They are modeled entirely through AssetHeuristics and AssetForecast, appearing
in the baseline load. There is no "IMPLICIT" request mode — uncontrollable
assets are forecast inputs, not scheduling targets.

### 1.10 CompletionPolicy
What happens when the last explicit DeadlineTier expires and the packet is not fully complete.
```
STOP                // terminate immediately → PARTIAL_COMPLETED if FillPercentage < 1.0
                    // use when: asset is needed for another task (battery charge before discharge),
                    // or partial result is acceptable (hot water 80% is fine)
CONTINUE            // keep going, bidding at PostDeadlineComfortBid for priority
                    // the bid determines how aggressively the packet competes for energy
                    // after the deadline — high bid (washing machine mid-cycle) beats
                    // low bid (EV top-up) but both still pay the actual import rate
```
Default per asset type (set by User Request Manager):
- BATTERY:          STOP (asset typically needed for discharge after charge deadline)
- EV:              CONTINUE, low bid (top-up with cheap energy, no rush)
- WASHING_MACHINE: CONTINUE, high bid (mid-cycle must finish, but still competes on price)
- HEAT_PUMP:       STOP (temperature reached or not, continuing wastes energy)
- COOKING_STOVE:   n/a (heuristic baseline only, no packets — see §1.9 note)
- HEATER:          STOP (same as heat pump)
- PV:              n/a (producer, not scheduled by packets in this way)
- GENERIC_*:       STOP

### 1.10.1 StaleRatePolicy
How the Planner handles slots beyond the last known rate data when VTN communication is lost.
```
LAST_KNOWN          // repeat the last known rate for all future slots
                    // simple but may be wrong if rates change significantly
HEURISTIC_FORECAST  // use learned rate heuristics (day-of-week, time-of-day patterns from PastRates)
                    // best guess based on historical patterns
DEFER_TO_FLEXIBLE   // do not allocate beyond known rates — mark all unknown slots as FLEXIBLE
                    // most conservative: waits for VTN reconnection before committing
SAFE_AVERAGE        // use a configurable safety rate (e.g. 80th percentile of historical rates)
                    // ensures budget is not underestimated
```
Configured on VenController as `StaleRatePolicy`. Default: HEURISTIC_FORECAST.

### 1.11 ForecastSource
How the forecast for an asset was generated.
```
WEATHER_MODEL       // derived from external weather/irradiation data (PV)
DEVICE_CLOUD        // provided by manufacturer's cloud service (e.g. inverter API)
PHYSICAL_MODEL      // computed from physics (e.g. thermal model for heater)
HEURISTIC           // learned from historical usage patterns (e.g. cooking stove)
MANUAL              // user-provided schedule
NONE                // no forecast available or needed (fully controllable assets like battery)
```

### 1.12 ExternalDataSourceType
```
WEATHER             // temperature, cloud cover, wind
IRRADIATION         // solar irradiation forecast
GRID_CO2_FORECAST   // CO2 intensity forecast (if not from VTN)
```

---

## 2. Core Value Types (Structs)

### 2.1 TimeStamp
```
Value:              datetime (RFC 3339, e.g. "2025-03-14T00:00:00Z")
```

### 2.2 Rate
```
Value:              float
Type:               RateType
Unit:               RateUnit        // e.g. EUR, g_CO2_eq
```

### 2.3 PowerRange
```
MinPower_kW:        float       // minimum controllable power (negative = export)
MaxPower_kW:        float       // maximum power (negative = export)
PowerSteps:         float[]?    // discrete levels, null if STEPLESS (e.g. [0, 3, 6])
```

### 2.4 RateSnapshot
Captures external price/CO2 signals at a point in time.
```
TimeStamp:          TimeStamp
ImportPrice:        Rate        // cost to import from grid (€/kWh)
ExportPrice:        Rate        // revenue for export to grid (€/kWh)
                                // Dual interpretation: when self-consuming PV surplus,
                                // ExportPrice is the OPPORTUNITY COST (forgone revenue).
                                // The algorithm uses ExportPrice as a cost in this context.
ImportCO2:          Rate        // carbon intensity of grid import (gCO2/kWh)
ImportCapacityLimit: float?     // kW, from IMPORT_CAPACITY_LIMIT event, null = no limit
ExportCapacityLimit: float?     // kW, from EXPORT_CAPACITY_LIMIT event, null = no limit
```

### 2.5 PowerSnapshot
```
TimeStamp:          TimeStamp
Power_kW:           float       // positive = consuming/importing, negative = producing/exporting
```

### 2.6 EnergySnapshot
Actual or planned energy measurement at a timestep.
```
TimeStamp:          TimeStamp
Power_kW:           float       // instantaneous power at this timestep
CumulativeEnergy_kWh: float     // cumulative energy delivered since packet start
```

### 2.7 ComfortRate
One point on the value curve: "at this fill %, I value more energy at this €/kWh and gCO2/kWh limit."
The MaxMarginalPrice is a **priority bid**, not the actual price paid. It determines which
packets win when competing for scarce capacity or cheap slots. The actual cost is always
the import rate of the slot where energy is consumed.
```
FillPercentage:     float       // 0.0 - 1.0, SoC or completion fraction
MaxMarginalPrice:   float       // max €/kWh the user bids — determines priority, not actual cost
MaxMarginalCO2:     float       // max gCO2/kWh user is willing to accept at this fill level
```

### 2.8 DeadlineTier
One tier in the temporal value model: "complete to this level by this time within this budget."
```
Deadline:           TimeStamp
MaxTotalCost:       float       // € total budget for this tier
MaxMarginalRate:    float       // €/kWh ceiling (per-timestep fast check)
MinCompletion:      float       // 0.0-1.0, below this the tier has zero value (gate)
```

### 2.9 ValueCurve
Complete user preference model for an EnergyPacket.
```
ComfortRates:       ComfortRate[]     // fill-based marginal value (sorted ascending by FillPercentage)
DeadlineTiers:      DeadlineTier[]    // time-based tiers (sorted by preference, Tier 0 = most preferred)
ActiveTierIndex:    int               // currently targeted tier (set by planner)
```

### ~~2.10 CalcCache~~ → moved to Step 4 (Algorithm Internals)
CalcCache is a transient working structure used during optimization.
It is built per-packet-per-timeslot, used to rank and allocate, then discarded.
The surviving output is `PacketAllocation.MarginalValue`. See Step 4 for definition.

### 2.11 ExternalDataSource
Shared external data source used by asset forecast strategies (e.g. weather for PV, outdoor temp for heat pump).
Configured at VenController level, consumed by assets that need it.
```
SourceID:           string
Type:               ExternalDataSourceType  // WEATHER, IRRADIATION, etc.
Url:                string                  // API endpoint
PollInterval:       duration                // e.g. "PT15M" (every 15 min)
LastFetch:          TimeStamp?              // when data was last fetched
FetchStatus:        enum (OK, STALE, FAILED, NEVER_FETCHED)
CachedData:         any                     // weather data, irradiation data, etc. (type depends on SourceType)
```

---

## 3. Asset Layer

### 3.1 AssetProfile
Static configuration of a device. Set at installation/configuration time.
```
AssetID:            string (unique)
AssetType:          AssetType
Name:               string                  // human-readable, e.g. "Rooftop PV"
PowerRange:         PowerRange
Adjustability:      PowerAdjustability
AutoFollow:         bool                    // can this device auto-adjust to fill gaps?
Bidirectional:      bool                    // can both consume and produce? (battery, V2G)
HasStorage:         bool                    // does it have an energy buffer? (battery, EV, thermal)
MaxCapacity_kWh:    float?                  // storage capacity if HasStorage
MinSoC:             float?                  // minimum SoC for discharge (e.g. 0.10 = 10% reserve). Null for non-storage.
Efficiency:         float                   // round-trip or conversion efficiency (0.0-1.0)
ResponseDelay_s:    float                   // expected time to confirm setpoint change
DeviationThreshold_kW: float               // |actual - planned| above this triggers replan
DefaultValueCurve:  ValueCurve?             // default user preference for this asset type
ThermalModelParams: ThermalModelParams?     // for thermal assets only (see §3.1.1). Null for non-thermal.
OadrResourceName:   string                  // maps to OpenADR resource.resourceName
```

### 3.1.1 ThermalModelParams
For thermal assets (HEAT_PUMP, HEATER). Allows conversion of TargetTemperature_C to TargetEnergy_kWh.
Recomputed each plan cycle using current temperature, outdoor forecast, and insulation parameters.
```
ThermalMass_kWh_per_K: float               // energy to raise mass by 1K (e.g. 2.5 for a water tank)
InsulationFactor:      float               // heat loss rate in kW/K (e.g. 0.1 = 100W per degree difference)
MinTemperature_C:      float               // safety minimum (e.g. 5°C for freeze protection)
MaxTemperature_C:      float               // safety maximum (e.g. 60°C for hot water)
```
Energy computation (each plan cycle):
```
currentTemp = AssetState.Temperature_C
targetTemp = from EnergyPacket (via UserRequest.TargetTemperature_C)
outdoorTemp = from ExternalDataSource (weather forecast, per slot)

// Static energy to reach target
staticEnergy = ThermalMass × (targetTemp - currentTemp)

// Ongoing heat loss over planning horizon
For each slot:
  lossRate_kW = InsulationFactor × (currentProjectedTemp - outdoorTemp[slot])
  slotLoss_kWh = lossRate_kW × slot.Duration_hours
  totalLoss += slotLoss_kWh

TargetEnergy_kWh = max(0, staticEnergy + totalLoss) / Efficiency
```
This is a simplified first-order model. Real implementations may use more
sophisticated models, but the interface (TargetEnergy output) stays the same.

### 3.2 AssetState
Live snapshot of device status. Updated every measurement cycle.
```
TimeStamp:          TimeStamp
AssetID:            string (ref → AssetProfile)

CommandedPower_kW:  float                   // setpoint sent to device
ActualPower_kW:     float                   // measured power from device
PowerDeviation_kW:  float                   // = ActualPower - CommandedPower (derived)

Responsiveness:     DeviceResponsiveness
LastConfirmedResponse: TimeStamp            // when device last confirmed a setpoint

SoC:                float?                  // 0.0-1.0, null if not storage (battery, EV)
Temperature_C:      float?                  // for thermal assets (heater, heat pump)
IsConnected:        bool                    // physically connected (EV plugged in, etc.)
IsAvailable:        bool                    // logically available for control
```

### 3.3 AssetHeuristics
Learned or configured behavioral patterns for uncontrollable/implicit assets.
```
AssetID:            string (ref → AssetProfile)
DaytimeProfile:     PowerSnapshot[]         // typical power by time of day
WeekdayWeights:     float[7]                // Mon=0..Sun=6 relative activity
SeasonalFactor:     float                   // multiplier for current season
LastUpdated:        TimeStamp
```

### 3.4 Asset
The composite entity joining profile, state, heuristics, and forecast.
```
Profile:            AssetProfile
State:              AssetState
Heuristics:         AssetHeuristics?        // null for fully controllable assets
Forecast:           AssetForecast?          // null if ForecastSource = NONE (see §3.6)
Ledger:             AssetLedger?            // current accounting period (see §3.7)
ActivePackets:      EnergyPacket[]          // packets currently assigned to this asset

// --- Methods ---
GetFlexibility():   AssetFlexibility        // computed from current state (see §3.5)
UpdateForecast():   void                    // recomputes Forecast from Profile, State, Heuristics, ExternalDataSources
UpdateHeuristics(): void                    // learns patterns from historical measurements (e.g. daily)
```

### 3.5 AssetFlexibility
Computed per asset per timestep from current AssetState, AssetProfile, and active EnergyPackets.
This is **not stored** — it is derived on demand. Replaces static FlexibilityDirection on assets.
```
AssetID:            string
TimeStamp:          TimeStamp

CanIncreaseConsumption_kW:  float   // how much MORE it could consume right now
CanDecreaseConsumption_kW:  float   // how much LESS it could consume right now
CanIncreaseProduction_kW:   float   // how much MORE it could produce right now
CanDecreaseProduction_kW:   float   // how much LESS it could produce right now
```
Examples:
- Washing machine running at 2kW: CanDecreaseConsumption=2, rest=0
- Washing machine idle: CanIncreaseConsumption=2, rest=0
- Battery charging at 1kW (max 2kW charge, max 2kW discharge):
  CanIncreaseConsumption=1, CanDecreaseConsumption=3 (swing from +1kW to -2kW)
- PV producing at 10kW: CanDecreaseProduction=10 (croppable), CanIncreaseProduction=0
- EV not connected: all zeros

### 3.6 AssetForecast
Predicted power profile for an asset over the planning horizon.
Each asset type has its own forecast strategy; the Planner sees the same structure for all.
Forecast is recomputed by `Asset.UpdateForecast()` before each planning cycle.
```
AssetID:            string
UpdatedAt:          TimeStamp               // when this forecast was last computed
Source:             ForecastSource           // how it was generated
Confidence:         float                   // 0.0-1.0, overall forecast confidence
Profile:            PowerSnapshot[]          // predicted power per timestep over planning horizon
AvailabilityWindows: TimeRange[]?           // predicted connected/available periods (EV, portable assets)
                                            // null = always available. Phase 2 excludes slots outside windows.
                                            // Replan on ASSET_STATE_CHANGE corrects stale availability forecasts.
```
`TimeRange: { Start: TimeStamp, End: TimeStamp }`
Examples:
- PV:           Source=WEATHER_MODEL, Profile = expected production from irradiation × panel specs
- Heat pump:    Source=PHYSICAL_MODEL, Profile = expected consumption from outdoor temp × thermal model
- Cooking stove: Source=HEURISTIC, Profile = typical usage pattern from AssetHeuristics
- EV:           Source=HEURISTIC, Profile = expected availability/connection windows
- Site residual: Source=HEURISTIC, Profile = learned unmodeled consumption pattern (see §3.8)
- Battery:      Source=NONE, Forecast=null (fully controllable, no prediction needed)

### 3.7 AssetLedger
Per-asset accounting over a billing period. Updated by Monitor each measurement cycle.
Separates accounting (long lifecycle) from live state (overwritten each cycle).
```
AssetID:            string
PeriodStart:        TimeStamp               // e.g. start of month
PeriodEnd:          TimeStamp?              // null = ongoing

TotalConsumption_kWh:   float               // all energy consumed (packets + untracked)
TotalProduction_kWh:    float               // all energy produced
TotalImportCost_EUR:    float               // cost of imported energy attributed to this asset
TotalExportRevenue_EUR: float               // revenue from exported energy attributed to this asset
TotalCO2_g:             float               // CO2 attributed to this asset

TrackedByPackets_kWh:   float               // energy covered by EnergyPackets
UntrackedEnergy_kWh:    float               // = Total - Tracked (standby, uncontrolled usage, etc.)
```

### 3.8 Site Residual (Virtual Asset)
A virtual asset representing all unmodeled site consumption — fridge, lights, router,
phone chargers, and any other device not registered as an Asset. It participates in the
normal forecast pipeline as a regular asset with Adjustability=NONE.

Created automatically at system startup. One per site.
```
Profile:
  AssetType:          SITE_RESIDUAL
  Name:               "Unmodeled site load"
  Adjustability:      NONE                    // cannot be controlled
  AutoFollow:         false
  Bidirectional:      false
  HasStorage:         false
  Efficiency:         1.0
  OadrResourceName:   "site_residual"

State:
  ActualPower_kW:     (computed each cycle, see below)
  Responsiveness:     RESPONSIVE              // always "working" — it's a measurement, not a device
  IsConnected:        true                    // always present
  IsAvailable:        false                   // never available for control

Heuristics:
  DaytimeProfile:     PowerSnapshot[]         // learned: typical residual by time of day
  WeekdayWeights:     float[7]                // learned: weekday vs weekend
  SeasonalFactor:     float                   // learned: summer vs winter

Forecast:
  Source:             HEURISTIC
  Profile:            PowerSnapshot[]         // predicted from learned heuristics
```

**Measurement (each Monitor cycle):**
```
SiteResidual.State.ActualPower =
    SiteMeter.NetImport_kW - Σ(Asset.State.ActualPower for all non-SITE_RESIDUAL assets)

Where positive means the residual is consuming, negative means it's producing
(unlikely but possible if an unregistered generator exists).
```

**Learning (daily via UpdateHeuristics):**
```
Aggregates residual measurements over past 24h into DaytimeProfile bins.
Fresh install: DaytimeProfile = flat default (e.g. 0.5 kW).
After 3 days: reasonable weekday profile.
After 2 weeks: weekday/weekend differentiation.
After 3 months: seasonal factor calibration.
```

**Planning:** The Planner treats SITE_RESIDUAL like any other uncontrollable asset —
its forecast populates the baseline load in each slot. This ensures the optimizer
accounts for real site consumption even when most devices are unmodeled.

**Limitation:** SITE_RESIDUAL absorbs ALL measurement discrepancies between the
grid meter and the sum of known assets. This includes genuine unmodeled consumption
AND forecast errors from known assets (e.g. PV forecast says 8kW but actual is 6kW
→ residual increases by 2kW). Over time, the heuristic learning averages out short-term
forecast errors, but in real-time, the residual is a composite signal. This is an
accepted approximation — the alternative (per-asset forecast error tracking) would
require ground-truth measurements for each asset, which are not always available.

### 3.9 SiteMeter
The grid connection point meter. Measures actual power flow between the site and the
grid. Required infrastructure — without it, the system cannot compute site-level import/
export or derive the site residual.
```
MeterID:            string
TimeStamp:          TimeStamp               // last measurement time
NetImport_kW:       float                   // positive = importing from grid, negative = exporting
Voltage_V:          float?                  // grid voltage (optional, for power quality)
Frequency_Hz:       float?                  // grid frequency (optional)
CumulativeImport_kWh: float                 // total imported energy (meter reading)
CumulativeExport_kWh: float                 // total exported energy (meter reading)
MeasurementInterval: duration               // how often the meter is read (e.g. PT1S, PT5S)
IsOnline:           bool                    // meter communication status
```

**Relationship to other entities:**
- Monitor reads SiteMeter to compute PastEnergySum (the site-level aggregation).
- Monitor uses SiteMeter + Σ(AssetState) to derive SITE_RESIDUAL.State.ActualPower.
- OpenADR IF uses SiteMeter-derived data for USAGE reports to VTN.
- PastEnergySum[] was previously described as "actual measured net power per timestep" —
  it is sourced from SiteMeter.NetImport_kW, not from summing individual assets.
  The sum of individual assets may differ from the meter (that difference IS the site residual).

---

## 4. Energy Packet Layer

### 4.1 EnergyPacket
The central scheduling unit. Represents a discrete energy task.
```
PacketID:           string (unique)
AssetID:            string (ref → Asset)
Status:             EnergyPacketStatus

// --- Temporal Bounds ---
EarliestStart:      TimeStamp               // cannot begin before this
LatestStart:        TimeStamp?              // must begin by this or abandon (null = no constraint)
LatestEnd:          TimeStamp               // absolute latest completion (from last DeadlineTier)

// --- Energy Target ---
TargetEnergy_kWh:   float                   // total energy required for 100% completion
TargetSoC:          float?                  // target SoC if storage asset (alternative to energy)

// --- Value ---
ValueCurve:         ValueCurve              // user preference (comfort + deadline tiers)
RequestMode:        UserRequestMode         // how the user expressed this request
CompletionPolicy:   CompletionPolicy        // what happens after last deadline (STOP or CONTINUE)
PostDeadlineComfortBid: float?              // €/kWh bid for priority after last deadline expires
                                            // only used when CompletionPolicy = CONTINUE
                                            // high bid = high priority (e.g. washing machine mid-cycle)
                                            // low bid = opportunistic (e.g. EV top-up with free energy)
                                            // null when CompletionPolicy = STOP

// --- Power Profile ---
PlannedPowerProfile: EnergySnapshot[]       // optimizer output: planned power at each timestep
PastPowerProfile:   EnergySnapshot[]        // actual measurements recorded during execution

// --- Derived / Computed (methods) ---
TotalEnergy():      float                   // = TargetEnergy_kWh
PlannedEnergy():    float                   // = sum of planned profile
PastEnergy():       float                   // = sum of past profile (actual delivered)
UndeliveredEnergy(): float                  // = TargetEnergy - PastEnergy (physically not yet delivered)
FillPercentage():   float                   // = PastEnergy / TargetEnergy (or SoC if storage)
PlannedEnd():       TimeStamp?              // = last timestep in PlannedPowerProfile
Started():          bool                    // = PastPowerProfile.length > 0
ShortestDuration(): duration                // = UndeliveredEnergy / MaxPower
ShortestFreeEndTime(): TimeStamp            // = now + ShortestDuration
IsOnTrack():        bool                    // can active tier still be met?

// --- Budget Tracking ---
AccumulatedCost_EUR: float                  // Σ(PastPower × ImportPrice × dt) so far
AccumulatedCO2_g:   float                   // Σ(PastPower × CO2Rate × dt) so far

// --- Planner Estimates (updated each plan cycle) ---
EstimatedCost_EUR:  float                   // FIRM: Σ(PacketAllocation.CostInSlot) + FLEXIBLE: envelope estimate
EstimatedCO2_g:     float                   // FIRM: Σ(PacketAllocation.CO2InSlot) + FLEXIBLE: envelope estimate
EstimatedCompletion: float                  // 0.0-1.0, expected fill at active tier deadline
LastEstimateAt:     TimeStamp               // when these estimates were last computed
```

### 4.2 DeviceSession
Links an EnergyPacket to actual device execution. Tracks the real-time control loop.
```
SessionID:          string (unique)
PacketID:           string (ref → EnergyPacket)
AssetID:            string (ref → Asset)

StartTime:          TimeStamp               // when this session began
EndTime:            TimeStamp?              // null if still active

CommandedSetpoint_kW: float                 // current setpoint sent to device
MeasuredPower_kW:   float                   // current actual power from device
CumulativeDelivered_kWh: float              // energy delivered in this session

Responsiveness:     DeviceResponsiveness    // mirrors AssetState but session-scoped
DeviationCount:     int                     // how many consecutive deviation-above-threshold readings
```

---

## 5. Grid Interface Layer (OpenADR Mapping)

### 5.1 OadrProgramConfig
Maps to an OpenADR program the VEN is enrolled in.
```
ProgramID:          string                  // OpenADR program.id
ProgramName:        string                  // OpenADR program.programName
PayloadTypes:       string[]                // e.g. ["PRICE", "GHG", "EXPORT_PRICE"]
ReportTypes:        string[]                // e.g. ["USAGE", "DEMAND"]
Currency:           string?                 // e.g. "EUR"
Units:              string?                 // e.g. "KWH"
IsCapacityProgram:  bool                    // participates in capacity management
```

### 5.2 OadrEventCache
Internal representation of a received OpenADR event, translated into domain terms.
```
EventID:            string                  // OpenADR event.id
ProgramID:          string                  // ref → OadrProgramConfig
EventName:          string?
ReceivedAt:         TimeStamp

// Translated content (denormalized from intervals)
RateSnapshots:      RateSnapshot[]          // extracted PRICE, EXPORT_PRICE, GHG per interval
CapacityLimits:     RateSnapshot[]?         // extracted IMPORT/EXPORT_CAPACITY_LIMIT per interval
AlertType:          string?                 // e.g. "ALERT_GRID_EMERGENCY", null if not alert
AlertMessage:       string?
DispatchSetpoints:  PowerSnapshot[]?        // extracted DISPATCH_SETPOINT per interval

// Report obligations
ReportDescriptors:  OadrReportObligation[]  // what reports VTN expects from us
```

### 5.3 OadrReportObligation
Tracks what reports the VTN has requested.
```
EventID:            string
PayloadType:        string                  // e.g. "USAGE", "DEMAND", "CAPACITY_RESERVATION"
ReadingType:        string                  // e.g. "DIRECT_READ", "FORECAST"
StartInterval:      int
NumIntervals:       int
Frequency:          int
Repeat:             int
Historical:         bool
DueAt:              TimeStamp?              // derived: when should this report be sent
Fulfilled:          bool                    // has this obligation been met
```

### 5.4 OadrCapacityState
Tracks the current capacity management state with the VTN.
```
ImportSubscription_kW:  float?              // from IMPORT_CAPACITY_SUBSCRIPTION event
ExportSubscription_kW:  float?              // from EXPORT_CAPACITY_SUBSCRIPTION event
ImportReservation_kW:   float?              // granted by VTN via CAPACITY_RESERVATION event
ExportReservation_kW:   float?              // granted by VTN
ImportCapacityLimits:   PowerSnapshot[]     // time-series from IMPORT_CAPACITY_LIMIT events
ExportCapacityLimits:   PowerSnapshot[]     // time-series from EXPORT_CAPACITY_LIMIT events

PendingReservationRequest: OadrCapacityRequest?  // our outstanding request, if any
LastReservationResponse: TimeStamp?
```

### 5.5 OadrCapacityRequest
A capacity reservation request we want to send to the VTN.
```
Direction:          FlexibilityDirection    // IMPORT or EXPORT
RequestedPower_kW:  float
Intervals:          PowerSnapshot[]         // time-series of requested capacity
OfferedFee:         float?                  // what we're willing to pay per kW
Reason:             string                  // human-readable justification
```

---

## 6. Planning Layer

### 6.1 PlanningHorizon
Defines the temporal scope of a planning cycle.
```
StartTime:          TimeStamp               // = now (truncated to PlanTimeStep)
EndTime:            TimeStamp               // = max(MinPlanTime from now, furthest EnergyPacket deadline)
StepSize:           duration                // = PlanTimeSteps setting (e.g. PT5M)
NumSteps:           int                     // = (EndTime - StartTime) / StepSize

NearHorizon:        TimeStamp               // = now + NearHorizonDuration (e.g. 2h, fine granularity)
FarHorizon:         TimeStamp               // = EndTime (coarser granularity OK)
```

### 6.2 PlanTimeSlot
One timestep in the planning grid. The optimizer fills these.
```
Index:              int
TimeStamp:          TimeStamp               // start of this slot
Duration:           duration                // = StepSize
Commitment:         SlotCommitment          // FIRM or FLEXIBLE (see below)

// --- External Conditions (from RateSnapshot) ---
ImportPrice_per_kWh:  float
ExportPrice_per_kWh:  float
CO2Rate_gPerKWh:      float
GridEffectiveCost:    float                 // = ImportPrice + (CO2Rate × CO2Weight)
                                            // Slot-level cost assuming pure grid import.
                                            // Used for: FLEXIBLE slot scoring, early firm-up variance,
                                            // Phase 4 storage profitability, Phase 7 envelope estimates.
                                            // Phase 2 computes per-packet surplus-aware EffectiveCost
                                            // for FIRM slots; GridEffectiveCost is the fallback.
RateEstimated:        bool                  // true if rate data was filled by StaleRatePolicy (VTN offline),
                                            // false if from actual VTN event. Used for PlanWarning generation.
ImportCapacityLimit_kW: float               // effective limit (subscription + reservation + event limit)
ExportCapacityLimit_kW: float

// --- Planned Allocations (optimizer output, FIRM slots only) ---
Allocations:        PacketAllocation[]      // which packets get how much power in this slot
NetPlannedPower_kW: float                   // sum of all allocations (positive = import, negative = export)
PlannedImport_kW:   float                   // max(0, NetPlannedPower)
PlannedExport_kW:   float                   // max(0, -NetPlannedPower)

// --- Surplus (derived from baseline) ---
SurplusAvailable_kW: float                  // = max(0, -BaselineLoad). PV surplus above fixed loads.
                                            // Shared pool: consumed by packets at ExportPrice (opportunity cost).

// --- Flexibility (derived after planning) ---
ImportFlexibility_kW: float                 // how much more we COULD import
ExportFlexibility_kW: float                 // how much more we COULD export
```

### 6.2.1 SlotCommitment
```
FIRM                // near-horizon: allocated to specific packets, Dispatcher will execute
FLEXIBLE            // far-horizon: not allocated to specific packets, flexibility preserved
```
FIRM slots have PacketAllocations. FLEXIBLE slots do not — their capacity is described
by FlexibilityEnvelopes (§6.9) and reported to VTN as available flexibility.

### 6.3 PacketAllocation
Assignment of power from one EnergyPacket in one PlanTimeSlot.
Only exists in FIRM slots.
```
PacketID:           string (ref → EnergyPacket)
AssetID:            string (ref → Asset)
AllocatedPower_kW:  float                   // total power allocated to this packet in this slot
SurplusPower_kW:    float                   // portion from PV surplus (opportunity cost = ExportPrice)
GridPower_kW:       float                   // portion from grid import (cost = ImportPrice)
                                            // AllocatedPower = SurplusPower + GridPower
MarginalValue:      float                   // effective priority at time of allocation (from CalcCache)
CostInSlot_EUR:     float                   // = SurplusPower×ExportPrice×dt + GridPower×ImportPrice×dt
CO2InSlot_g:        float                   // = GridPower × CO2Rate × dt (surplus has zero CO2)
```

### ~~6.4 Plan~~ → replaced by §6.10 Plan (updated)
Plan now has a two-layer structure (FIRM + FLEXIBLE sections). See §6.10 below.

### 6.5 PlanWarning
```
Severity:           enum (INFO, WARNING, CRITICAL)
PacketID:           string?                 // null if system-level warning
Message:            string                  // e.g. "EV charge cannot meet Tier 1 deadline within budget"
SuggestedAction:    string?                 // e.g. "Falling back to Tier 2"
```

### 6.6 PenaltyRule
Models periodic/conditional charges that are NOT per-kWh rates but triggered by threshold breach.
Penalties are discrete: the full cost applies once if the threshold is crossed during the period.
Once breached, the marginal cost of further breach is zero for the rest of the period.
The Planner treats these as binary barriers: "this allocation would cross the threshold → its
effective cost includes the full penalty. If already breached this period → no additional cost."
```
RuleID:             string
Description:        string                  // human-readable, e.g. "Peak demand charge"
Condition:          PenaltyCondition
Threshold:          PenaltyThreshold
Cost:               float                   // total cost in local currency if triggered (e.g. €100)
CostUnit:           RateUnit                // e.g. EUR
Period:             duration                // billing period, e.g. "P1M"
MeasurementWindow:  duration                // rolling average window for threshold evaluation (e.g. "PT15M")
                                            // Must be ≥ DispatchCycleTime (need at least one reading).
                                            // Typical values: PT15M (utility standard), PT1M (strict), PT5S (instantaneous).
                                            // Matches the utility's actual metering method.
Active:             bool                    // can be temporarily disabled
BreachedThisPeriod: bool                    // true once threshold crossed this period
BreachTimeStamp:    TimeStamp?              // when breach occurred (null if not breached)
CurrentPeakValue:   float?                  // rolling average peak demand or cumulative usage in period
RollingAverage:     float?                  // current rolling average of SiteMeter.NetImport over MeasurementWindow
```
Monitor logic (each cycle):
- Compute RollingAverage = mean(SiteMeter.NetImport readings within last MeasurementWindow).
- Update CurrentPeakValue = max(CurrentPeakValue, RollingAverage) over the period.
- If CurrentPeakValue ≥ Threshold AND BreachedThisPeriod = false:
    BreachedThisPeriod = true, BreachTimeStamp = now.

Planner logic:
- **Before breach** (BreachedThisPeriod = false):
  For each candidate allocation, project what RollingAverage would be if allocation is committed.
  If projected average would push CurrentPeakValue past Threshold
  → this allocation carries the full penalty Cost as a barrier.
  Compare against the packet's comfort bid: if bid > Cost spread over the energy gained,
  proceed. Otherwise, find a different schedule that avoids the breach.

- **After breach** (BreachedThisPeriod = true):
  The penalty cost is sunk — the €100 is already incurred for this period.
  But the system CONTINUES to enforce the threshold as a soft constraint.
  Reason: higher peak values may affect future contract terms, trigger additional
  penalty tiers, or simply violate the user's expressed preference.
  The Planner still tries to keep planned power ≤ Threshold in every slot.
  The difference: before breach, exceeding costs €100 (hard barrier).
  After breach, exceeding costs €0 additional (soft preference) — so the Planner
  may allow brief exceedances if the alternative is much worse for the user,
  but it does NOT relax all restrictions and run unchecked.

### 6.7 PenaltyCondition
```
PEAK_DEMAND_EXCEEDED        // any timestep power > threshold_kW during period
ENERGY_BUDGET_EXCEEDED      // total consumption > threshold_kWh during period
EVENT_NONCOMPLIANCE         // didn't follow DR/emergency event within tolerance
EXPORT_LIMIT_EXCEEDED       // exported more than allowed at any timestep
```

### 6.8 PenaltyThreshold
```
Threshold_kW:       float?                  // for PEAK_DEMAND_EXCEEDED, EXPORT_LIMIT_EXCEEDED
Threshold_kWh:      float?                  // for ENERGY_BUDGET_EXCEEDED
EventID:            string?                 // for EVENT_NONCOMPLIANCE (ref to OadrEventCache)
TolerancePercent:   float?                  // for EVENT_NONCOMPLIANCE (e.g. 10% = OK if within 10%)
```

### 6.9 FlexibilityEnvelope
Describes a packet's flexible demand in the far horizon. Not a commitment — it declares
"this packet needs energy somewhere in this window." Reported to VTN as available flexibility.
One FlexibilityEnvelope per packet that has unallocated energy in FLEXIBLE slots.
```
PacketID:           string (ref → EnergyPacket)
AssetID:            string (ref → Asset)

// --- What ---
EnergyNeeded_kWh:   float                   // UndeliveredEnergy minus any energy in FIRM slots
MaxPower_kW:        float                   // asset's max power
MinPower_kW:        float                   // asset's min power (if STEPPED, smallest nonzero step)

// --- When ---
WindowStart:        TimeStamp               // earliest FLEXIBLE slot for this packet
WindowEnd:          TimeStamp               // latest FLEXIBLE slot (LatestEnd for STOP, open for CONTINUE)
SlotsAvailable:     int                     // number of FLEXIBLE slots in window

// --- Value ---
MaxAcceptableRate:  float                   // min(ComfortBid at current fill, ActiveTier.MaxMarginalRate)
                                            // "the most expensive slot this packet will accept"
MinAcceptableRate:  float                   // ComfortBid at projected fill after full delivery
                                            // "the cheapest slot would still be accepted at this price"
BudgetRemaining_EUR: float                  // MaxTotalCost - AccumulatedCost - FIRM slot costs

// --- Estimate (based on average rate in flexible window) ---
EstimatedCost_EUR:  float                   // EnergyNeeded × average(eligible slot GridEffectiveCost)
EstimatedCO2_g:     float                   // EnergyNeeded × average(eligible slot CO2Rate)
```

**Relationship to VTN reports:**
- USAGE_FORECAST: FIRM allocations are reported as point values (readingType=FORECAST).
  FLEXIBLE envelopes are reported as ranges: power = 0 to MaxPower_kW during the window.
- IMPORT_CAPACITY_RESERVATION: sum of FlexibilityEnvelope.MaxPower across all flexible packets
  in each slot = total flexible demand the VTN could trigger.
- DOWN_REGULATION_AVAILABLE: flexible packets represent shiftable load — the VTN can
  ask "charge your EV at 02:00 instead of 20:00" by sending favorable prices at 02:00.

### 6.10 Plan (updated)
The complete output of one planning cycle.
```
PlanID:             string (unique)
CreatedAt:          TimeStamp
Trigger:            PlanTrigger              // what caused this replan
Horizon:            PlanningHorizon
FirmBoundary:       TimeStamp               // = now + effective near-horizon duration

// --- FIRM section (near horizon) ---
FirmSlots:          PlanTimeSlot[]          // slots with Commitment=FIRM and PacketAllocations
FirmSummary:
  TotalFirmCost_EUR:    float
  TotalFirmCO2_g:       float
  TotalFirmImport_kWh:  float
  TotalFirmExport_kWh:  float

// --- FLEXIBLE section (far horizon) ---
FlexibleSlots:      PlanTimeSlot[]          // slots with Commitment=FLEXIBLE (no allocations)
Envelopes:          FlexibilityEnvelope[]   // per-packet flexibility declarations
FlexibleSummary:
  TotalFlexibleEnergy_kWh: float            // sum of all envelopes' EnergyNeeded
  EstimatedFlexCost_EUR:   float            // sum of envelope estimates
  EstimatedFlexCO2_g:      float

// --- Combined ---
Packets:            EnergyPacket[]          // all packets considered (snapshot at plan time)
Warnings:           PlanWarning[]
```

---

## 7. Dispatcher Layer

### 7.1 DispatchCommand
Issued by dispatcher to an asset each dispatch cycle. Only references FIRM slot allocations.
```
AssetID:            string
TimeStamp:          TimeStamp
CommandedPower_kW:  float
SourcePacketID:     string?                 // which EnergyPacket this serves (null if auto-follow/idle)
Reason:             string                  // e.g. "plan", "auto-follow", "emergency override"
```

### 7.2 DispatchState
Snapshot of the current dispatch reality.
```
TimeStamp:          TimeStamp
Commands:           DispatchCommand[]       // active commands to all assets
NetActualPower_kW:  float                   // measured total (positive = import)
NetPlannedPower_kW: float                   // what the plan said for this timestep
NetDeviation_kW:    float                   // = Actual - Planned
DeviationSignificant: bool                  // |NetDeviation| > threshold for ReplanTriggerDuration
```

---

## 8. User Request Layer

### 8.1 UserRequest
What the user actually asks for (translated into EnergyPackets by the system).
```
RequestID:          string (unique)
AssetID:            string (ref → Asset)
Mode:               UserRequestMode
CreatedAt:          TimeStamp

// --- What ---
TargetEnergy_kWh:   float?                  // e.g. "charge 30 kWh"
TargetSoC:          float?                  // e.g. "charge to 80%"
TargetTemperature_C: float?                 // e.g. "heat to 55°C"

// --- When ---
EarliestStart:      TimeStamp?              // null = now
Deadlines:          UserDeadline[]          // ordered list of deadline preferences
CompletionPolicy:   CompletionPolicy?       // null = use asset-type default (see §1.10)

// --- How much ---
MaxTotalCost_EUR:   float?                  // overall budget
MaxMarginalRate:    float?                  // per-kWh ceiling
MaxCO2_g:           float?                  // overall CO2 budget
WarnThreshold_EUR:  float?                  // alert user if cost will exceed this

// --- Generated ---
PacketIDs:          string[]                // EnergyPacket(s) created from this request
```

### 8.2 UserDeadline
One deadline preference from the user, maps to a DeadlineTier.
```
Deadline:           TimeStamp
MaxCost_EUR:        float?                  // willing to pay up to this total for this deadline
MinCompletion:      float?                  // e.g. 0.6 = "at least 60%"
Label:              string?                 // e.g. "tonight", "by Friday"
```

### 8.3 UserNotification
Alerts generated by the system for the user.
```
NotificationID:     string
TimeStamp:          TimeStamp
Severity:           enum (INFO, WARNING, ALERT)
RelatedPacketID:    string?
RelatedAssetID:     string?
Message:            string                  // e.g. "EV charge will cost €1.30, exceeding your €1.00 limit"
RequiresAction:     bool                    // user needs to decide something
ActionOptions:      string[]?               // e.g. ["Accept higher cost", "Defer to Friday", "Cancel"]
```

---

## 9. Controller State (Top-Level Singleton)

### 9.1 VenController
The root entity. One instance per site.
```
// --- Configuration ---
SiteID:             string
MinPlanTime:        duration                // e.g. "PT24H"
PlanTimeStep:       duration                // e.g. "PT5M"
DispatchCycleTime:  duration                // e.g. "PT5S"
NearHorizonDuration: duration               // e.g. "PT2H"
ReplanCooldown:     duration                // minimum time between replans (e.g. "PT30S")
DeviationReplanThreshold_kW: float          // net deviation that triggers replan
SustainedDeviationTime: duration            // how long deviation must persist before replan (e.g. "PT30S")
CO2Weight:          float                   // user's €/gCO2 weighting factor for optimization
StaleRatePolicy:    StaleRatePolicy         // behavior when VTN rates expire (see §1.10.1)
StaleContinueTimeout: duration              // abandon CONTINUE packets after this duration with no progress (e.g. "P7D")
ContinueHorizonExtension: duration          // how far beyond LatestEnd to plan for CONTINUE packets (e.g. "PT24H")

// --- OpenADR Connection ---
VtnUrl:             string
ClientID:           string
ClientName:         string
Programs:           OadrProgramConfig[]
CapacityState:      OadrCapacityState
EventCache:         OadrEventCache[]
ReportObligations:  OadrReportObligation[]

// --- Assets ---
Assets:             Asset[]                 // includes SITE_RESIDUAL virtual asset
ExternalDataSources: ExternalDataSource[]   // shared data for asset forecasts (weather, irradiation)

// --- Site Metering ---
SiteMeter:          SiteMeter               // grid connection point meter (required)

// --- Rate Forecasts ---
PlannedRates:       RateSnapshot[]          // future rates from VTN events
PastRates:          RateSnapshot[]          // historical rates (for reporting + heuristic learning)
RateHeuristic:      RateHeuristic           // learned rate patterns for StaleRatePolicy.HEURISTIC_FORECAST

// --- Energy Planning ---
ActivePlan:         Plan?                   // current optimizer output
PlanHistory:        Plan[]                  // recent plans for diagnostics
ActivePackets:      EnergyPacket[]          // all non-terminal packets
CompletedPackets:   EnergyPacket[]          // recent completed/abandoned for reporting
PenaltyRules:       PenaltyRule[]           // active penalty/demand charge rules

// --- Aggregated State ---
PlannedEnergySum:   PowerSnapshot[]         // planned net power per timestep (from ActivePlan)
PastEnergySum:      PowerSnapshot[]         // actual measured net power per timestep

// --- Dispatch ---
CurrentDispatch:    DispatchState
ActiveSessions:     DeviceSession[]

// --- User ---
PendingRequests:    UserRequest[]
Notifications:      UserNotification[]

// --- Methods ---
GetImportFlexibility(): PowerSnapshot[]     // per timestep: how much more we could import
GetExportFlexibility(): PowerSnapshot[]     // per timestep: how much more we could export
GetNetDeviation():  float                   // current actual vs. planned
NeedsReplan():      bool                    // should we trigger a planning cycle?
HasAutoFollowCapacity(): bool               // = any asset where Profile.AutoFollow=true AND State.Responsiveness=RESPONSIVE AND State.IsAvailable=true
AutoFollowHeadroom_kW(): float              // = Σ GetFlexibility() across all auto-follow-capable assets (total swing range available for deviation absorption)
```

### 9.2 RateHeuristic
Learned rate patterns from PastRates history. Used by StaleRatePolicy.HEURISTIC_FORECAST
when VTN rates are unavailable. Same learning structure as AssetHeuristics.
```
DaytimeProfile:     RateSnapshot[]          // typical rates by time of day (per 15-min or hourly bins)
WeekdayWeights:     float[7]                // Mon=0..Sun=6 relative rate multiplier
SeasonalFactor:     float                   // multiplier for current season (summer vs winter pricing)
LastUpdated:        TimeStamp

// --- Methods ---
predict(timeOfDay, dayOfWeek): RateSnapshot  // returns predicted ImportPrice, ExportPrice, CO2
UpdateHeuristics(): void                     // learns from PastRates, called daily
```
Learning (daily):
```
Aggregates PastRates over past 30 days into DaytimeProfile bins.
Fresh install: flat default (e.g. national average ImportPrice €0.20, ExportPrice €0.08).
After 1 week: reasonable day/night pattern.
After 1 month: weekday/weekend differentiation.
After 3 months: seasonal factor calibration.
```

---

## 10. Entity Relationships (Summary)

```
VenController (singleton)
 ├── OadrProgramConfig[]          ── enrollments in VTN programs
 ├── OadrEventCache[]             ── received events, translated to RateSnapshots
 ├── OadrCapacityState            ── capacity subscription/reservation state
 ├── OadrReportObligation[]       ── pending report obligations to VTN
 │
 ├── ExternalDataSource[]         ── shared weather/irradiation data for forecasts
 │
 ├── RateHeuristic               ── learned rate patterns (for StaleRatePolicy)
 │
 ├── SiteMeter                    ── grid connection point (NetImport, cumulative readings)
 │
 ├── Asset[]                      ── physical devices + SITE_RESIDUAL virtual asset
 │    ├── AssetProfile            ── static config
 │    ├── AssetState              ── live measurements
 │    │                              (SITE_RESIDUAL: derived from SiteMeter - Σ other assets)
 │    ├── AssetHeuristics?        ── learned patterns (updated by UpdateHeuristics())
 │    ├── AssetForecast?          ── predicted power profile (updated by UpdateForecast())
 │    ├── AssetLedger?            ── per-period cost/energy accounting
 │    ├── GetFlexibility()        ── computed AssetFlexibility (not stored)
 │    └── EnergyPacket[]          ── assigned packets (ref, not owned)
 │
 ├── RateSnapshot[] (Planned)     ── future rates from VTN
 ├── RateSnapshot[] (Past)        ── historical rates
 │
 ├── EnergyPacket[]               ── the scheduling units (owned here)
 │    ├── ValueCurve              ── user preference
 │    │    ├── ComfortRate[]      ── fill-based value
 │    │    └── DeadlineTier[]     ── time-based tiers
 │    ├── EnergySnapshot[] (Planned)
 │    ├── EnergySnapshot[] (Past)
 │    ├── AccumulatedCost/CO2     ── actual spend tracking (updated by Dispatcher)
 │    └── EstimatedCost/CO2       ── planned spend forecast (updated by Planner)
 │
 ├── PenaltyRule[]                ── periodic/conditional charge rules
 │    └── PenaltyThreshold        ── trigger conditions
 │
 ├── Plan                         ── current optimizer output
 │    ├── PlanningHorizon
 │    ├── FirmBoundary            ── divides FIRM from FLEXIBLE slots
 │    ├── PlanTimeSlot[] (FIRM)   ── near-horizon committed allocations
 │    │    └── PacketAllocation[]
 │    ├── PlanTimeSlot[] (FLEXIBLE) ── far-horizon, no allocations
 │    ├── FlexibilityEnvelope[]   ── per-packet flexible demand declarations
 │    └── PlanWarning[]
 │
 ├── DispatchState                ── current dispatch reality
 │    └── DispatchCommand[]
 │
 ├── DeviceSession[]              ── active asset↔packet execution links
 │
 ├── UserRequest[]                ── pending user requests
 └── UserNotification[]           ── alerts for user
```

---

## 11. OpenADR 3.1 Mapping Reference

How internal entities map to OpenADR API objects:

| Internal Entity | OpenADR Direction | OpenADR Object | Notes |
|---|---|---|---|
| Asset.Profile.OadrResourceName | VEN → VTN | resource.resourceName | Registered during enrollment |
| RateSnapshot.ImportPrice | VTN → VEN | event interval payload PRICE | Per-interval from pricing event |
| RateSnapshot.ExportPrice | VTN → VEN | event interval payload EXPORT_PRICE | Per-interval |
| RateSnapshot.ImportCO2 | VTN → VEN | event interval payload GHG | Per-interval, g/kWh |
| RateSnapshot.ImportCapacityLimit | VTN → VEN | event interval payload IMPORT_CAPACITY_LIMIT | Per-interval, kW |
| OadrCapacityState.ImportSubscription | VTN → VEN | event payload IMPORT_CAPACITY_SUBSCRIPTION | Long-duration event |
| OadrCapacityState.ImportReservation | VTN → VEN | event payload IMPORT_CAPACITY_RESERVATION | Per request/response |
| PastEnergySum | VEN → VTN | report payload USAGE | Per resource, per interval. Sourced from SiteMeter |
| AssetState.ActualPower | VEN → VTN | report payload DEMAND | readingType = DIRECT_READ |
| SiteMeter.NetImport_kW | VEN → VTN | report payload DEMAND (site-level) | Whole-site demand reading |
| GetImportFlexibility() | VEN → VTN | report payload IMPORT_CAPACITY_RESERVATION | Capacity request report |
| GetExportFlexibility() | VEN → VTN | report payload EXPORT_CAPACITY_RESERVATION | Capacity request report |
| Asset.State.SoC | VEN → VTN | report payload STORAGE_CHARGE_LEVEL | PERCENT |
| Asset.Forecast.Profile | VEN → VTN | report payload USAGE_FORECAST | FIRM slots: point forecast. FLEXIBLE: range (0 to MaxPower in window) |
| FlexibilityEnvelope[] | VEN → VTN | report payload DOWN_REGULATION_AVAILABLE | Shiftable demand the VTN can influence via price signals |
| Alert events | VTN → VEN | event payload ALERT_GRID_EMERGENCY etc. | Translated to high-value pseudo-packet |

---

*End of Step 1 (Draft 6, amended). Changes from Draft 5:*
- *Renamed RemainingEnergy() → UndeliveredEnergy() on EnergyPacket (§4.1): clarifies this is physical undelivered energy, distinct from algorithm-internal "energy still needing slot assignment"*
- *Added SlotCommitment enum to PlanTimeSlot (§6.2): FIRM (near-horizon, allocated) vs FLEXIBLE (far-horizon, flexibility preserved)*
- *Added SurplusAvailable_kW to PlanTimeSlot (§6.2): PV surplus above fixed loads, shared pool consumed at ExportPrice opportunity cost*
- *Added SurplusPower_kW, GridPower_kW to PacketAllocation (§6.3): tracks how much energy came from surplus vs grid import. CostInSlot and CO2InSlot computed accordingly.*
- *Added FlexibilityEnvelope entity (§6.9): per-packet declaration of flexible demand in far horizon. Includes energy needed, power range, time window, acceptable rate range, budget remaining, and cost/CO2 estimates.*
- *Replaced Plan entity (§6.4 → §6.10): two-layer structure with FirmBoundary, FirmSlots + FlexibleSlots + Envelopes, separate summaries for firm and flexible portions*
- *Updated EnergyPacket estimates (§4.1): EstimatedCost/CO2 now combines FIRM actuals + FLEXIBLE envelope estimates*
- *Updated entity relationship tree (§10): Plan shows FirmBoundary, FIRM/FLEXIBLE slots, FlexibilityEnvelope[]*
- *Updated OpenADR mapping (§11): USAGE_FORECAST distinguishes FIRM point forecast vs FLEXIBLE range. Added FlexibilityEnvelope → DOWN_REGULATION_AVAILABLE mapping.*

*Changes from Drafts 1–5 (preserved):*
- *SITE_RESIDUAL, SiteMeter, CompletionPolicy, PenaltyRule thresholds, per-packet estimates, forecasts*

*Proceed to Step 2 (Controller Architecture) after review.*
