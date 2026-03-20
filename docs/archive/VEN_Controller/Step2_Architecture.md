# Step 2 — Controller Architecture & Component Responsibilities

**Scope:** Single-site residential HEMS acting as OpenADR 3.1 VEN.  
**Version:** Draft 5  
**Prerequisite:** Step 1 Entity Model (Draft 6)

---

## 1. Component Overview

The VEN Controller is decomposed into six components. Each runs on its own cycle or trigger
and communicates with others through shared state on the VenController singleton.

```
┌─────────────────────────────────────────────────────────────────┐
│                        VenController                            │
│                                                                 │
│  ┌──────────────┐    ┌──────────────┐    ┌───────────────────┐  │
│  │   OpenADR    │    │    User      │    │     Monitor       │  │
│  │  Interface   │    │   Request    │    │    (Deviation     │  │
│  │              │    │   Manager    │    │     Detector)     │  │
│  └──────┬───────┘    └──────┬───────┘    └────────┬──────────┘  │
│         │                   │                     │             │
│         │   ┌───────────────┴─────────────┐       │             │
│         │   │                             │       │             │
│         ▼   ▼                             │       │             │
│  ┌──────────────┐                         │       │             │
│  │              │◄────────────────────────┘       │             │
│  │   Planner    │◄────────────────────────────────┘             │
│  │              │                                               │
│  └──────┬───────┘                                               │
│         │                                                       │
│         ▼                                                       │
│  ┌──────────────┐    ┌───────────────────┐                      │
│  │              │    │                   │                      │
│  │  Dispatcher  │───►│ Asset Controller  │◄───► [Devices]       │
│  │              │    │                   │                      │
│  └──────────────┘    └───────────────────┘                      │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
                           │
                           ▼
                    ┌──────────────┐
                    │     VTN      │
                    │  (OpenADR)   │
                    └──────────────┘
```

**Data flow summary:**
- OpenADR Interface receives events from VTN → writes RateSnapshots, CapacityState, EventCache
- User Request Manager receives user input → writes EnergyPackets
- Monitor watches AssetState vs. Plan → triggers replans
- Planner reads everything, produces Plan → writes ActivePlan, updates EnergyPacket profiles
- Dispatcher reads Plan + AssetState → writes DispatchCommands
- Asset Controller executes commands on devices → writes AssetState
- OpenADR Interface reads PastEnergySum, Flexibility, AssetState → sends reports to VTN

---

## 2. Component Details

---

### 2.1 OpenADR Interface

**Purpose:** Translates between the OpenADR 3.1 REST API and the internal domain model.
This is the only component that knows about OpenADR JSON, HTTP, webhooks, and OAuth.

**Cycle:** Event-driven (webhook callbacks + periodic polling as fallback).

#### Owns (creates/modifies)
```
OadrProgramConfig[]         — updated on program discovery/change
OadrEventCache[]            — created on each received event
OadrReportObligation[]      — extracted from event reportDescriptors
OadrCapacityState           — updated from capacity subscription/reservation/limit events
PlannedRates[]              — translated from PRICE/EXPORT_PRICE/GHG event intervals
PastRates[]                 — moved from PlannedRates as time passes
```

#### Reads
```
ActivePlan                  — to compute flexibility for capacity reports
PastEnergySum[]             — for USAGE reports
Assets[].State              — for DEMAND, STORAGE_CHARGE_LEVEL, OPERATING_STATE reports
Assets[].Forecast           — for USAGE_FORECAST reports
OadrReportObligation[]      — to know what reports are due and when
OadrCapacityState           — to decide if capacity requests are needed
GetImportFlexibility()      — for capacity reservation request reports
GetExportFlexibility()      — for capacity reservation request reports
```

#### Triggers received (inputs)
```
VTN webhook notification    → new/updated event arrives
Polling timer               → periodic check for new events (fallback)
ReportObligation.DueAt      → time to send a report
Planner output              → flexibility changed, may need to update capacity request
```

#### Triggers emitted (outputs)
```
→ PlanTrigger.RATE_CHANGE       when new PRICE/GHG/EXPORT_PRICE event processed
→ PlanTrigger.CAPACITY_CHANGE   when new capacity limit/reservation event processed
→ PlanTrigger.ALERT             when ALERT_* event received
```

#### VTN → VEN Event Translation Rules
```
Event payload type              → Internal target
─────────────────────────────────────────────────────────────────
PRICE                           → RateSnapshot.ImportPrice
EXPORT_PRICE                    → RateSnapshot.ExportPrice
GHG                             → RateSnapshot.ImportCO2
IMPORT_CAPACITY_SUBSCRIPTION    → OadrCapacityState.ImportSubscription_kW
EXPORT_CAPACITY_SUBSCRIPTION    → OadrCapacityState.ExportSubscription_kW
IMPORT_CAPACITY_LIMIT           → RateSnapshot.ImportCapacityLimit (per interval)
EXPORT_CAPACITY_LIMIT           → RateSnapshot.ExportCapacityLimit (per interval)
IMPORT_CAPACITY_RESERVATION     → OadrCapacityState.ImportReservation_kW
EXPORT_CAPACITY_RESERVATION     → OadrCapacityState.ExportReservation_kW
CAPACITY_AVAILABLE              → available capacity info (used when deciding requests)
CAPACITY_AVAILABLE_FEE          → cost of requesting capacity (used when deciding requests)
ALERT_GRID_EMERGENCY            → high-priority pseudo-EnergyPacket (via Planner)
ALERT_BLACK_START               → high-priority pseudo-EnergyPacket (via Planner)
ALERT_POSSIBLE_OUTAGE           → UserNotification + PlanTrigger.ALERT
ALERT_FLEX_ALERT                → PlanTrigger.ALERT (shift/shed signal)
SIMPLE                          → mapped to load shed level → affects plan constraints
DISPATCH_SETPOINT               → direct override → Dispatcher (bypasses Planner)
CHARGE_STATE_SETPOINT           → target SoC → creates/modifies EnergyPacket
```

#### VEN → VTN Report Generation Rules
```
Report obligation type          → Source
─────────────────────────────────────────────────────────────────
USAGE                           → PastEnergySum per resource, per interval (from SiteMeter)
DEMAND                          → AssetState.ActualPower per resource
STORAGE_CHARGE_LEVEL            → AssetState.SoC per storage resource
STORAGE_MAX_CHARGE_POWER        → AssetProfile.PowerRange.MaxPower
STORAGE_MAX_DISCHARGE_POWER     → AssetProfile.PowerRange.MinPower (abs)
OPERATING_STATE                 → derived from DeviceResponsiveness + EnergyPacketStatus
USAGE_FORECAST                  → FIRM slots: point forecast per resource (readingType=FORECAST)
                                  FLEXIBLE slots: range per resource (0 to MaxPower in window)
IMPORT_CAPACITY_RESERVATION     → GetImportFlexibility() + Σ FlexibilityEnvelope.MaxPower per slot
EXPORT_CAPACITY_RESERVATION     → GetExportFlexibility() (capacity request report)
UP_REGULATION_AVAILABLE         → sum of AssetFlexibility.CanDecreaseConsumption + CanIncreaseProduction
DOWN_REGULATION_AVAILABLE       → Σ FlexibilityEnvelope.MaxPower (shiftable demand VTN can influence)
                                  + sum of AssetFlexibility.CanIncreaseConsumption + CanDecreaseProduction
```

Key point: FlexibilityEnvelopes make far-horizon flexible demand visible to the VTN.
The VTN can respond with price signals (cheaper rate in a specific window) or direct
dispatch events. Either way, the next plan cycle will firm up those slots using the
new information — naturally resolving the flexibility into commitment.

#### Startup / Enrollment Flow
```
On first startup or configuration change:
1. Authenticate with VTN (OAuth client_credentials grant → access token)
2. GET /programs → discover available programs
3. For each program the user has enrolled in (via utility account):
   POST /vens → register this VEN with the program
   POST /resources → register each Asset (via OadrResourceName) into the program
4. VTN responds with program-specific event subscriptions
5. Store OadrProgramConfig[] for each enrolled program
6. Begin polling for events (or register webhook callback URL)

On token expiry: refresh before expiry using refresh_token. If refresh fails,
re-authenticate. If re-auth fails, enter degraded mode (VTN connection lost).

Token storage: in memory or secure local storage. Never in PlanHistory or logs.
```

#### CHARGE_STATE_SETPOINT Handling
```
When VTN sends CHARGE_STATE_SETPOINT (e.g. "charge EV to 90%"):
  If an existing user packet targets the same asset:
    - VTN setpoint MODIFIES the existing packet's TargetSoC/TargetEnergy.
    - ValueCurve from user is preserved (user's comfort preferences still apply).
    - If VTN target exceeds user budget → PlanWarning, UserNotification:
      "VTN requests 90% SoC but user budget allows only 80%. Using 80%."
    - User can override: accept VTN target with higher budget, or decline.
  If no existing packet:
    - Create new EnergyPacket with VTN target.
    - Use asset's DefaultValueCurve for ComfortRates.
    - Budget = unconstrained (VTN-initiated, no user cost limit unless configured).
    - CompletionPolicy = STOP at event end time.
```

#### Report Obligation Lifecycle
```
Report obligations are created when events contain reportDescriptors.
  - Same report type from multiple programs → deduplicate by shortest interval.
    (If program A wants USAGE every 15min and program B every 5min → report every 5min.)
  - Obligation cancelled when: event is updated with reportDescriptor removed,
    OR event expires, OR program unenrollment.
  - Maximum report backlog: configurable (e.g. 1000 entries). If exceeded during
    VTN outage, oldest entries are aggregated into summary reports.
```

#### Multi-Program Conflict Resolution
```
When events from different programs conflict for the same time interval:
  Pricing (PRICE, EXPORT_PRICE, GHG):
    Use the lowest price (benefits the user). If a program offers €0.05 while
    another offers €0.12, use €0.05. Log the override for compliance reporting.
  Capacity limits (IMPORT_CAPACITY_LIMIT, EXPORT_CAPACITY_LIMIT):
    Use the most restrictive limit (safety first). min() of all programs' limits.
  Alerts (ALERT_*):
    Process all alerts. Multiple simultaneous alerts each generate their own
    synthetic packet. The Planner resolves priority through MarginalValue.
  DISPATCH_SETPOINT:
    If two programs send conflicting setpoints → reject both, notify user.
    Direct device control requires unambiguous authority.
```

---

### 2.2 User Request Manager

**Purpose:** Accepts user input, validates it, translates it into EnergyPackets with ValueCurves,
and handles user notifications. This is the only component that knows about the user-facing
interface (UI, API, voice, etc.).

**Cycle:** Event-driven (user actions).

#### Owns (creates/modifies)
```
UserRequest[]               — created from user input
EnergyPacket[]              — created from UserRequest translation
  └── ValueCurve            — built from UserDeadline[] + defaults
  └── DeadlineTier[]        — built from UserDeadline[]
UserNotification[]          — created when system needs to inform user
```

#### Reads
```
Assets[]                    — to validate request against asset capabilities
AssetProfile.DefaultValueCurve  — fallback when user doesn't specify full preferences
AssetState.SoC              — to calculate TargetEnergy from TargetSoC
AssetState.IsConnected      — to reject requests for disconnected assets
ActivePackets[]             — to detect conflicts with existing requests
ActivePackets[].Estimated*  — to generate cost/CO2 notifications from planner estimates
ActivePlan.Warnings[]       — to generate user notifications from plan warnings
```

#### Triggers received (inputs)
```
User creates new request    → translate to EnergyPacket(s)
User modifies request       → update existing EnergyPacket(s), mark modified
User cancels request        → set EnergyPacket.Status = ABANDONED
Plan produces warnings      → translate warnings to UserNotifications
Planner tier fallback       → notify user of degraded service
Planner estimates updated   → generate initial estimate notification for new packets
                            → generate cost/CO2 warning notifications if thresholds approached
```

#### Triggers emitted (outputs)
```
→ PlanTrigger.USER_REQUEST      when new or modified EnergyPacket created
```

#### Translation Logic: UserRequest → EnergyPacket
```
1. Validate: asset exists, is connected, is available

1a. Reject uncontrollable assets: heuristic-driven assets (COOKING_STOVE, etc.) do NOT get
   EnergyPackets. They are modeled via AssetHeuristics and AssetForecast only,
   appearing in the baseline load. If a user tries to create a request for an
   asset with Adjustability = NONE, reject with explanation.

2. Calculate TargetEnergy_kWh:
   - If TargetSoC given: (TargetSoC - CurrentSoC) × MaxCapacity_kWh / Efficiency
   - If TargetTemperature given: use ThermalModelParams (§3.1.1) to compute energy.
     TargetEnergy is RECOMPUTED each plan cycle (weather changes → heat loss changes).
     At request time: initial estimate using current outdoor temp + weather forecast.
   - If TargetEnergy given: use directly

3. Multi-deadline requests produce ONE packet with multiple tiers, NOT multiple packets.
   "Charge by tonight for €5, otherwise by Friday for €1" → one EnergyPacket with:
     DeadlineTiers: [ {tonight, €5}, {Friday, €1} ]
   This avoids asset commitment conflicts between concurrent packets on the same asset.

4. Determine CompletionPolicy:
   - If UserRequest.CompletionPolicy is set: use it
   - Else: use asset-type default (see §1.10 in entity model)
5. Build DeadlineTiers from UserDeadline[]:
   - Each UserDeadline → one DeadlineTier
   - MaxMarginalRate derived from MaxTotalCost / TargetEnergy (if not specified)
   - If CompletionPolicy = CONTINUE:
     Append implicit post-deadline tier:
       Deadline = far future
       MaxTotalCost = ∞ (no budget limit — bid controls priority, actual rate controls cost)
       MaxMarginalRate = PostDeadlineComfortBid (from asset-type default or user override)
       MinCompletion = 0
   - If CompletionPolicy = STOP:
     No implicit tier appended. LatestEnd is a hard cutoff.
6. Build ValueCurve:
   - If asset has DefaultValueCurve, use as base
   - Override with user-specified ComfortRates if provided
   - Attach DeadlineTiers from step 5
7. Compute PostDeadlineComfortBid:
   - If user specifies: use directly
   - Else derive from asset-type defaults:
     WASHING_MACHINE: high bid (e.g. €5.00/kWh — almost always wins priority)
     EV: low bid (e.g. €0.02/kWh — near-zero-cost energy only)
     Other CONTINUE assets: moderate bid from DefaultValueCurve
8. Create EnergyPacket with:
   - Status = PENDING
   - EarliestStart = UserRequest.EarliestStart ?? now
   - LatestEnd = last explicit DeadlineTier.Deadline (NOT the implicit post-deadline tier)
   - RequestMode from UserRequest.Mode
   - CompletionPolicy from step 4
   - PostDeadlineComfortBid from step 7 (null if STOP)
9. If WarnThreshold_EUR set:
   - Register internal watch: if AccumulatedCost_EUR approaching threshold → notify
```

Important: PostDeadlineComfortBid is a **priority bid**, not a price the user pays.
A washing machine bidding €5.00/kWh will almost never be shed by the Planner, but
the actual energy cost is still the import rate of the slot (e.g. €0.30/kWh).
This is the same mechanism as ComfortRate.MaxMarginalPrice — it determines who wins
when packets compete, not what they pay.

#### Heuristic Assets (Baseline Only)
```
Assets with Adjustability = NONE and heuristic-driven behavior (e.g. COOKING_STOVE)
do NOT get EnergyPackets. They are modeled entirely through AssetHeuristics and
AssetForecast, appearing in the baseline load. The Planner optimizes around them
but does not schedule them.

Rationale: creating packets for uncontrollable assets would double-count their energy
(once in baseline from forecast, once from packet allocation). Instead, they are
purely forecast inputs — the "background" the optimizer works around.
```

---

### 2.3 Planner

**Purpose:** The optimization engine. Takes all current state and produces a Plan — the
allocation of EnergyPackets to time slots, respecting constraints and optimizing for cost,
CO2, and comfort. This is the most complex component; algorithm detail is in Step 4.

**Cycle:** Triggered (not periodic by itself — triggered by PlanTrigger events).
Throttled by ReplanCooldown to prevent thrashing.

#### Owns (creates/modifies)
```
Plan                        — the complete optimizer output (two-layer: FIRM + FLEXIBLE)
  └── FirmSlots[]           — near-horizon: per-timestep PacketAllocations
       └── PacketAllocation[] — per-packet power in each FIRM slot
  └── FlexibleSlots[]       — far-horizon: no allocations, capacity preserved
  └── FlexibilityEnvelope[] — per-packet flexible demand declarations (far horizon)
  └── PlanWarning[]         — issues detected during planning
PlanHistory[]               — previous plans (for diagnostics)
PlannedEnergySum[]          — aggregated planned power per timestep (FIRM slots only)

EnergyPacket updates:
  └── PlannedPowerProfile   — rewritten each plan cycle (FIRM allocations only)
  └── ValueCurve.ActiveTierIndex — which tier we're targeting
  └── Status                — PENDING→SCHEDULED, or SCHEDULED→ABANDONED if infeasible
  └── EstimatedCost_EUR     — FIRM: Σ(CostInSlot) + FLEXIBLE: envelope estimate
  └── EstimatedCO2_g        — FIRM: Σ(CO2InSlot) + FLEXIBLE: envelope estimate
  └── EstimatedCompletion   — expected fill % at active tier deadline
  └── LastEstimateAt        — timestamp of this estimate
```

#### Reads
```
PlannedRates[]              — future price/CO2/capacity landscape
ActivePackets[]             — all non-terminal EnergyPackets to schedule
Assets[]                    — profiles, states, heuristics, flexibility, forecasts
  └── AssetProfile          — power ranges, adjustability, efficiency
  └── AssetState            — current SoC, power, responsiveness, connectivity
  └── AssetForecast         — predicted power per timestep (PV production, heuristic consumption, etc.)
  └── GetFlexibility()      — what each asset can offer right now
OadrCapacityState           — subscription/reservation limits
PenaltyRules[]              — discrete threshold barriers (includes BreachedThisPeriod state)
ActiveSessions[]            — what's currently executing (can't abruptly stop some)
HasAutoFollowCapacity()     — is there a fast-response buffer asset available?
AutoFollowHeadroom_kW()     — how much deviation can be absorbed in real time?
```

#### Triggers received (inputs)
```
PlanTrigger.PERIODIC            — regular cycle (e.g. every PlanTimeStep = 5 min)
PlanTrigger.RATE_CHANGE         — from OpenADR Interface
PlanTrigger.CAPACITY_CHANGE     — from OpenADR Interface
PlanTrigger.ALERT               — from OpenADR Interface (high priority, skip cooldown)
PlanTrigger.USER_REQUEST        — from User Request Manager
PlanTrigger.DEVICE_DEVIATION    — from Monitor
PlanTrigger.ASSET_STATE_CHANGE  — from Monitor
```

#### Triggers emitted (outputs)
```
→ PlanWarning[]                 picked up by User Request Manager for notifications
→ Updated PlannedEnergySum      picked up by OpenADR Interface for flexibility reports
→ Updated EnergyPacket profiles picked up by Dispatcher for execution
→ Updated EnergyPacket estimates picked up by User Request Manager for user notifications
```

#### Pre-Planning Step: Forecast Update
```
Before optimization begins, the Planner orchestrates forecast updates:

1. Refresh ExternalDataSources if stale (weather, irradiation)
   - Check each source: if now - LastFetch > PollInterval → fetch new data
   - If fetch fails: mark FetchStatus=STALE, use cached data, add PlanWarning

2. Update asset forecasts:
   - For each Asset: call UpdateForecast()
   - Each asset type computes its own forecast from its Profile, State, and relevant ExternalDataSource:
     · PV: irradiation data × panel capacity × orientation → production profile
     · Heat pump: outdoor temperature × thermal model × target temperature → consumption profile
     · Cooking stove: AssetHeuristics.DaytimeProfile × WeekdayWeights → consumption profile
     · EV: heuristic connection/disconnection windows → availability profile
     · Site residual: AssetHeuristics.DaytimeProfile × WeekdayWeights → unmodeled consumption profile
     · Battery: Source=NONE, no forecast needed (fully controllable)

3. Build the planning grid:
   - For each PlanTimeSlot: populate expected uncontrollable production/consumption from AssetForecast[]
   - This is the "baseline" the optimizer works around
```

#### Optimization Objective
```
Minimize total effective cost across all active EnergyPackets over the planning horizon,
where for each timeslot:

  EffectiveCost(slot) = ImportPrice + (ImportCO2 × CO2Weight)     [for imported energy]
  EffectiveRevenue(slot) = ExportPrice                             [for exported energy]

Subject to all hard and soft constraints listed below.

In plain terms:
  - Schedule consumption into slots where EffectiveCost is lowest
  - Schedule production/export into slots where EffectiveRevenue is highest
  - When multiple packets compete for a cheap slot, allocate to the packet
    with the highest MarginalValue (comfort × time pressure)
  - Storage assets (battery) act as time-shifters: charge when EffectiveCost is low,
    discharge when EffectiveCost is high, accounting for round-trip efficiency losses
  - PV surplus decision: export if EffectiveRevenue(now) > EffectiveCost(future slot) × efficiency,
    else store for later self-consumption
  - Uncontrollable assets (from AssetForecast): treated as fixed load/production in each slot,
    optimizer schedules controllable assets around them

The output Plan assigns every active packet a power level in every timeslot.
PacketAllocation.CostInSlot and CO2InSlot are computed per allocation.
```

#### Post-Planning Step: Estimate Computation
```
After optimization, the Planner computes per-packet estimates:

1. For each active EnergyPacket:
   FirmCost = Σ(PacketAllocation.CostInSlot) for FIRM slots assigned to this packet
   FirmCO2 = Σ(PacketAllocation.CO2InSlot) for FIRM slots assigned to this packet
   FirmEnergy = Σ(AllocatedPower × Duration) for FIRM slots

   If packet has a FlexibilityEnvelope (energy still in FLEXIBLE slots):
     FlexCost = Envelope.EstimatedCost_EUR     // based on average rate in eligible window
     FlexCO2 = Envelope.EstimatedCO2_g
     FlexEnergy = Envelope.EnergyNeeded_kWh
   Else:
     FlexCost = FlexCO2 = FlexEnergy = 0

   EstimatedCost_EUR = AccumulatedCost + FirmCost + FlexCost
   EstimatedCO2_g = AccumulatedCO2 + FirmCO2 + FlexCO2
   EstimatedCompletion = (PastEnergy + FirmEnergy + FlexEnergy) / TargetEnergy
   LastEstimateAt = now

   Note: FlexCost is an estimate, not a commitment. It uses the average GridEffectiveCost
   across eligible FLEXIBLE slots as a proxy. If the VTN publishes better prices later,
   the next plan cycle will revise the estimate downward. This means estimates for
   packets with mostly-FLEXIBLE energy have wider uncertainty bands — which is honest.

2. Tier feasibility check:
   - If EstimatedCost_EUR > ActiveTier.MaxTotalCost → tier infeasible → fall back to next tier
   - If EstimatedCompletion < ActiveTier.MinCompletion → tier infeasible → fall back
   - If all tiers exhausted → Status = ABANDONED, PlanWarning (CRITICAL)

3. Budget warnings:
   - If EstimatedCost_EUR > WarnThreshold_EUR (from UserRequest) → PlanWarning (WARNING)
   - If EstimatedCost_EUR > 0.9 × ActiveTier.MaxTotalCost → PlanWarning (INFO: approaching limit)

4. New packet notifications:
   - For packets created since last plan cycle (first estimate):
     → PlanWarning (INFO) with estimate summary
     → User Request Manager translates this to initial user notification:
       "EV charge to 80% scheduled. Est. cost: €0.85 / 340g CO2. Completion by 18:45."
       (If mostly FLEXIBLE: "Est. cost: ~€0.85 (may improve with VTN pricing).")

5. Build FlexibilityEnvelopes:
   For each packet with unallocated energy beyond the FIRM boundary:
     Envelope = FlexibilityEnvelope {
       EnergyNeeded = UndeliveredEnergy - FirmEnergy
       MaxPower = Asset.MaxPower
       MinPower = Asset.MinPower (or smallest STEPPED level)
       WindowStart = max(FirmBoundary, packet.EarliestStart)
       WindowEnd = packet.LatestEnd (STOP) or far horizon end (CONTINUE)
       MaxAcceptableRate = min(ComfortBid at current fill, ActiveTier.MaxMarginalRate)
       MinAcceptableRate = ComfortBid at projected fill after full delivery
       BudgetRemaining = MaxTotalCost - AccumulatedCost - FirmCost
       EstimatedCost = EnergyNeeded × avg(GridEffectiveCost for eligible FLEXIBLE slots)
       EstimatedCO2 = EnergyNeeded × avg(CO2Rate for eligible FLEXIBLE slots)
     }
```

#### Planner Constraints (inputs to optimization)
```
Hard constraints (must not violate):
  - AssetProfile.PowerRange (min/max per asset)
  - AssetProfile.PowerSteps (discrete levels for STEPPED assets)
  - RateSnapshot.ImportCapacityLimit (site import ceiling from VTN)
  - RateSnapshot.ExportCapacityLimit (site export ceiling from VTN)
  - OadrCapacityState.ImportSubscription + ImportReservation (total allowed import)
  - EnergyPacket.EarliestStart (cannot start before this)
  - EnergyPacket.CompletionPolicy = STOP → zero power for this packet's asset after LatestEnd
    (asset becomes available for other packets, e.g. battery switches from charge to discharge)
  - AssetState.IsConnected = false → zero power for that asset
  - AssetState.Responsiveness = UNRESPONSIVE|OFFLINE → treat as fixed load (current actual)

Soft constraints (violations have costs):
  - DeadlineTier.MaxTotalCost (budget ceiling → tier fallback)
  - DeadlineTier.MaxMarginalRate (per-kWh ceiling → skip expensive slots)
  - ComfortRate.MaxMarginalPrice (priority bid — determines who wins scarce capacity)
  - DeadlineTier.MinCompletion (below this → tier has zero value, abandon tier)

Penalty thresholds (discrete barrier decisions):
  PenaltyRules are NOT per-kWh costs. Each is a binary threshold:
  - For each candidate allocation, compute whether it would push CurrentPeakValue
    past PenaltyThreshold.
  - If BreachedThisPeriod = false AND allocation would cross threshold:
    → This allocation carries the full PenaltyRule.Cost (e.g. €100).
    → Compare: is the benefit of this allocation (comfort bid × energy) worth €100?
    → If no: find alternative schedule that avoids the breach.
    → If yes: accept the penalty, schedule the allocation, set BreachedThisPeriod = true.
  - If BreachedThisPeriod = true:
    → Penalty already incurred. No additional penalty cost as hard barrier.
    → But threshold remains a soft constraint: Planner still tries to stay under,
      spending a small budget on rescheduling (e.g. 5% of penalty cost).
    → Will allow exceedances only if avoidance is expensive relative to user comfort.
  This means penalty avoidance is strongest early in the billing period (full €100 at stake)
  and transitions to a soft preference once breached (sunk cost, but still enforced).

CompletionPolicy awareness:
  - STOP packets: Planner knows the asset is freed at LatestEnd. Can schedule a different
    packet on the same asset starting at LatestEnd (e.g. battery charge → discharge).
  - CONTINUE packets past deadline: treated as normal packets with PostDeadlineComfortBid
    as their effective MaxMarginalPrice. High bid → high priority, low bid → opportunistic.
    No special logic needed — the bid enters the standard optimization.

Planning risk margin (derived from auto-follow availability):
  - If HasAutoFollowCapacity() = true:
      Plan aggressively. Forecast errors within AutoFollowHeadroom_kW are absorbed
      in real time by the Dispatcher. Capacity limit headroom can be small.
  - If HasAutoFollowCapacity() = false:
      Plan conservatively. No fast-response buffer exists — any deviation becomes
      immediate grid import/export. Keep larger margin below capacity limits.
      Favor earlier scheduling of critical packets (less room to recover from surprises).
      Avoid scheduling multiple high-power assets in the same slot where forecast
      uncertainty is high (e.g. slots depending on PV production).
  - CONTINUE packets with high PostDeadlineComfortBid (e.g. washing machine):
      Planner should avoid starting these close to capacity crunches or expensive windows.
      If a washing machine cycle takes 2h, don't start at 17:00 if peak pricing hits 18:00 —
      the high bid will force it to keep running through the expensive window.
      This is not a hard constraint but a planning risk: the expected cost of overrun
      should be included in the scheduling decision.
```

#### Alert Handling
```
Alerts are processed by the Planner (not OpenADR IF) during the immediate replan
triggered by PlanTrigger.ALERT. The Planner creates synthetic EnergyPackets that
compete through the same bid mechanism as user packets. No special "override" logic
— just very high comfort bids.

OpenADR IF's role: translate alert event → emit PlanTrigger.ALERT (skip ReplanCooldown).
Planner's role: create synthetic packet based on alert type and severity.

ALERT_GRID_EMERGENCY:
  - Create synthetic EnergyPacket with:
    - Negative TargetEnergy (reduce consumption)
    - Very high comfort bid (derived from potential VTN penalty cost)
      e.g. if noncompliance penalty = €100 over 2h at 10kW reduction:
      effective bid = €100 / 20kWh = €5.00/kWh
    - Immediate deadline
  - Skip ReplanCooldown, replan immediately
  - The high bid ensures the emergency packet wins over most other packets.
    But a washing machine mid-cycle bidding €5.00/kWh COULD match it —
    and that's correct: the Planner should shed cheaper loads first.

ALERT_FLEX_ALERT:
  - Create synthetic packet or increase effective cost of import during alert window
  - Bid derived from flex alert incentive/penalty
  - Replan to shift load away from alert period

ALERT_BLACK_START:
  - Extreme bid (highest possible) on a consumption-reduction packet
  - Only assets with very high bids keep running
  - User-configured "critical asset" priority expressed as high default bids
```

---

### 2.4 Dispatcher

**Purpose:** Executes the FIRM section of the current Plan in real time. Translates planned
power profiles into concrete DispatchCommands for each asset, handles short-term deviations
using auto-follow assets, and manages DeviceSessions. Never acts on FLEXIBLE slots — those
are only resolved when they cross into near horizon on the next plan cycle.

**Cycle:** Periodic, fast (every DispatchCycleTime, e.g. 5 seconds).

#### Owns (creates/modifies)
```
DispatchState               — current dispatch snapshot
DispatchCommand[]           — active commands to assets
DeviceSession[]             — created when packet execution begins, closed on completion

EnergyPacket updates:
  └── PastPowerProfile      — appended each dispatch cycle (actual measurements)
  └── AccumulatedCost_EUR   — updated based on actual power × current rate
  └── AccumulatedCO2_g      — updated based on actual power × current CO2 rate
  └── Status                — SCHEDULED→ACTIVE, ACTIVE→COMPLETED, ACTIVE→PARTIAL_COMPLETED,
                               ACTIVE→PAUSED, PAUSED→ACTIVE, ACTIVE→FAILED
```

#### Reads
```
ActivePlan.TimeSlots[]      — what should happen now (PacketAllocations for current slot)
  └── PacketAllocation.SurplusPower_kW / GridPower_kW — for surplus-aware cost tracking
PlannedRates[]              — current import/export price for cost tracking
Assets[].State              — actual measured power, responsiveness, SoC
Assets[].Profile            — power range, response delay, auto-follow capability
ActivePackets[]             — to check completion conditions
```

#### Triggers received (inputs)
```
DispatchCycleTimer          — every DispatchCycleTime (e.g. 5s)
DISPATCH_SETPOINT event     — direct VTN override (bypasses plan, immediate execution)
```

#### Triggers emitted (outputs)
```
→ DispatchCommand           sent to Asset Controller for execution
→ AssetState updates        measured values written back (via Asset Controller)
→ EnergyPacket.Status changes  (COMPLETED, FAILED, etc.)
```

#### Dispatch Logic (per cycle)
```
1. Read current PlanTimeSlot from ActivePlan.FirmSlots (lookup by current time)
   If current time is beyond FirmBoundary: no firm allocations to execute.
   (This shouldn't happen in practice — the Planner always maintains a firm window
   at least NearHorizonDuration ahead, and replans before the boundary is reached.)
2. For each PacketAllocation in the slot:
   a. Look up target asset
   b. If asset RESPONSIVE: send DispatchCommand with AllocatedPower_kW
   c. If asset DEGRADED: send command but expect partial compliance
   d. If asset UNRESPONSIVE/OFFLINE: skip, treat current ActualPower as fixed
   e. If asset Adjustability = RECOMMENDATION: send DispatchCommand with
      Reason = "recommendation" (not "plan"). Device may ignore it.
      Monitor treats non-compliance from RECOMMENDATION assets as expected
      (does not increment DeviceSession.DeviationCount).
      Planner uses wider capacity margins for RECOMMENDATION assets (treat
      their allocated power as uncertain — include in plan but do not rely on it
      for tight capacity or penalty avoidance decisions).
3. Handle auto-follow assets (battery, flexible loads):
   a. Calculate NetDeviation = Σ(ActualPower) - Σ(PlannedPower) across non-auto-follow assets
   b. Distribute deviation compensation across auto-follow assets proportionally
   c. Respect PowerRange limits
4. For each active DeviceSession:
   a. Record actual power → EnergyPacket.PastPowerProfile
   b. Update cost using PacketAllocation's surplus/grid split:
      surplusFraction = PacketAllocation.SurplusPower / PacketAllocation.AllocatedPower
      gridFraction = PacketAllocation.GridPower / PacketAllocation.AllocatedPower
      AccumulatedCost += ActualPower × (surplusFraction × ExportPrice
                                         + gridFraction × ImportPrice) × dt
      AccumulatedCO2 += ActualPower × gridFraction × CO2Rate × dt  // surplus has zero CO2
   c. Check completion: if PastEnergy ≥ TargetEnergy (or SoC ≥ TargetSoC) → COMPLETED
   d. Check deadline: if now ≥ LatestEnd AND CompletionPolicy = STOP:
      → if FillPercentage = 1.0: COMPLETED
      → if FillPercentage < 1.0: PARTIAL_COMPLETED
      → close DeviceSession, free asset for other packets
   e. Check failure: if Responsiveness = OFFLINE for > threshold → FAILED
5. Update DispatchState with current totals
```
Note: CONTINUE packets past deadline are handled naturally — the implicit post-deadline
tier keeps them in ActivePackets with PostDeadlineComfortBid as their effective bid.
No special Dispatcher logic needed; they just keep executing through normal plan allocation.

#### Direct Override Handling (DISPATCH_SETPOINT from VTN)
```
- Bypasses Planner entirely
- Dispatcher sends DispatchCommand directly to target asset
- Creates synthetic EnergyPacket to track the override
- Triggers PlanTrigger.CAPACITY_CHANGE so Planner re-optimizes around the override
```

---

### 2.5 Asset Controller

**Purpose:** Abstraction layer over physical device communication. Sends setpoints,
reads measurements, tracks responsiveness. This is the only component that knows
about device protocols (Modbus, MQTT, REST, CTA-2045, etc.).

**Cycle:** Driven by Dispatcher (receives commands) + periodic measurement polling.

#### Owns (creates/modifies)
```
AssetState                  — updated from device measurements
  └── ActualPower_kW        — latest measured power
  └── SoC                   — latest state of charge
  └── Temperature_C         — latest temperature (thermal assets)
  └── IsConnected           — physical connectivity status
  └── Responsiveness        — derived from response timing
  └── LastConfirmedResponse — when device last acknowledged a setpoint

SiteMeter                   — updated from grid meter measurements
  └── NetImport_kW          — positive = importing, negative = exporting
  └── CumulativeImport_kWh  — meter reading (import counter)
  └── CumulativeExport_kWh  — meter reading (export counter)
  └── IsOnline              — meter communication status
```

#### Reads
```
DispatchCommand[]           — setpoints to send to devices
AssetProfile                — expected response delay, deviation threshold
```

#### Triggers received (inputs)
```
DispatchCommand             — from Dispatcher, each dispatch cycle
MeasurementPollTimer        — periodic device reading (may be faster than dispatch cycle)
Device event/notification   — some devices push state changes (e.g. EV plugged in)
```

#### Triggers emitted (outputs)
```
→ AssetState updates        written to shared state after each measurement
→ PlanTrigger.ASSET_STATE_CHANGE  when IsConnected or Responsiveness changes significantly
```

#### Responsiveness Detection Logic
```
On each DispatchCommand:
  - Record CommandedPower_kW and timestamp
  - Start response timer (AssetProfile.ResponseDelay_s)

On each measurement:
  - If |ActualPower - CommandedPower| ≤ DeviationThreshold → RESPONSIVE, reset timer
  - If response timer expired and still deviating → DEGRADED
  - If DEGRADED for > 3× ResponseDelay → UNRESPONSIVE
  - If no measurement received for > 10× ResponseDelay → OFFLINE

On device event (EV plug/unplug, etc.):
  - Update IsConnected immediately
  - If newly disconnected: OFFLINE
  - If newly connected: RESPONSIVE (optimistic, will degrade if no response)
```

---

### 2.6 Monitor (Deviation Detector)

**Purpose:** Continuously compares actual system state against the active Plan.
Detects significant deviations and decides whether a replan is needed.
Also watches for penalty threshold approaches and maintains per-asset accounting (AssetLedger).

**Cycle:** Periodic, moderate (every DispatchCycleTime or slightly slower, e.g. 10s).

#### Owns (creates/modifies)
```
PastEnergySum[]             — aggregated actual measurements per timestep (sourced from SiteMeter)
Asset[].Ledger              — per-asset cost/energy accounting (updated each cycle)
SITE_RESIDUAL.State         — derived: SiteMeter.NetImport - Σ(other assets' ActualPower)
```

#### Reads
```
SiteMeter                   — grid connection point meter (actual site import/export)
DispatchState               — current actual vs. planned totals
Assets[].State              — per-asset actual power and responsiveness
Assets[].ActivePackets      — to attribute energy to tracked vs untracked
ActivePlan.TimeSlots[]      — what was planned for current time
ActivePackets[]             — to check per-packet deviation from plan
PenaltyRules[]              — to check if approaching penalty thresholds
DeviceSessions[]            — deviation counts per session
PastRates[]                 — current rate for cost attribution in ledger
```

#### Triggers received (inputs)
```
MonitorCycleTimer           — periodic check
AssetState change           — from Asset Controller (can also be event-driven)
```

#### Triggers emitted (outputs)
```
→ PlanTrigger.DEVICE_DEVIATION      when net deviation exceeds threshold for sustained period
→ PlanTrigger.ASSET_STATE_CHANGE    when device connectivity/responsiveness changes
→ UserNotification                  when penalty threshold approaching
→ UserNotification                  when EnergyPacket.AccumulatedCost approaching WarnThreshold
```

#### Deviation Detection Logic
```
Each monitor cycle:

0. Site residual update:
   SITE_RESIDUAL.State.ActualPower =
     SiteMeter.NetImport_kW - Σ(Asset.State.ActualPower for all non-SITE_RESIDUAL assets)
   (Positive = unmodeled consumption, negative = unmodeled production)

1. Site-level deviation:
   NetDeviation = SiteMeter.NetImport_kW - ActivePlan.current_slot.NetPlannedPower
   If |NetDeviation| > DeviationReplanThreshold_kW for > SustainedDeviationTime:
     → emit PlanTrigger.DEVICE_DEVIATION

2. Per-asset deviation:
   For each asset with an active DeviceSession:
     AssetDeviation = |ActualPower - CommandedPower|
     If AssetDeviation > AssetProfile.DeviationThreshold_kW:
       Increment DeviceSession.DeviationCount
     Else:
       Reset DeviceSession.DeviationCount to 0

3. Penalty proximity and breach check:
   For each PenaltyRule where Active = true:
     If condition = PEAK_DEMAND_EXCEEDED:
       // Compute rolling average over MeasurementWindow (e.g. PT15M)
       windowReadings = SiteMeter.NetImport readings within last MeasurementWindow
       RollingAverage = mean(windowReadings)
       PenaltyRule.RollingAverage = RollingAverage
       PenaltyRule.CurrentPeakValue = max(PenaltyRule.CurrentPeakValue, RollingAverage)
       If RollingAverage > Threshold_kW AND BreachedThisPeriod = false:
         BreachedThisPeriod = true
         BreachTimeStamp = now
         → UserNotification (ALERT: "Peak demand threshold breached. €100 penalty incurred.")
       If RollingAverage > Threshold_kW × 0.9 AND BreachedThisPeriod = false:
         → UserNotification (WARNING: "Approaching peak demand limit")
     If condition = ENERGY_BUDGET_EXCEEDED:
       CurrentTotalUsage = sum(PastEnergySum[]) in current period
       PenaltyRule.CurrentPeakValue = CurrentTotalUsage
       If CurrentTotalUsage > Threshold_kWh AND BreachedThisPeriod = false:
         BreachedThisPeriod = true
         BreachTimeStamp = now
         → UserNotification (ALERT)
       If CurrentTotalUsage > Threshold_kWh × 0.9 AND BreachedThisPeriod = false:
         → UserNotification (WARNING)

4. Cost watch:
   For each ActivePacket with WarnThreshold set (via UserRequest):
     If AccumulatedCost_EUR > WarnThreshold_EUR × 0.9:  → UserNotification (WARNING)
     If AccumulatedCost_EUR > WarnThreshold_EUR:         → UserNotification (ALERT)

5. Tier feasibility check:
   For each ActivePacket:
     If IsOnTrack() = false (can't meet active tier deadline/budget):
       → emit info to Planner (will cause tier fallback on next plan cycle)
       → UserNotification (INFO: "EV charge falling back from tonight to Friday deadline")

5a. LatestStart check:
   For each ActivePacket where Status = PENDING:
     If LatestStart is set AND now > LatestStart:
       → Status = ABANDONED
       → UserNotification (WARNING: "Packet missed latest start time. Abandoned.")

5b. CONTINUE staleness check:
   For each ActivePacket where CompletionPolicy = CONTINUE AND past last DeadlineTier:
     lastProgress = most recent PastPowerProfile entry with power > 0
     If (now - lastProgress) > StaleContinueTimeout:
       → Status = ABANDONED
       → UserNotification (WARNING: "EV charge stalled for [N days] with no progress. Abandoned.
         Remaining energy could not be delivered within bid constraints.")

6. Asset ledger update:
   For each Asset:
     currentRate = PastRates[now]
     energy_kWh = AssetState.ActualPower × dt
     If ActualPower > 0 (consuming):
       Ledger.TotalConsumption_kWh += energy_kWh
       Ledger.TotalImportCost_EUR += energy_kWh × currentRate.ImportPrice.Value
       Ledger.TotalCO2_g += energy_kWh × currentRate.ImportCO2.Value
     If ActualPower < 0 (producing):
       Ledger.TotalProduction_kWh += |energy_kWh|
       Ledger.TotalExportRevenue_EUR += |energy_kWh| × currentRate.ExportPrice.Value
     If asset has active EnergyPackets covering this energy:
       Ledger.TrackedByPackets_kWh += energy_kWh
     Ledger.UntrackedEnergy_kWh = TotalConsumption + TotalProduction - TrackedByPackets

   Note: SITE_RESIDUAL is included in the "For each Asset" loop above.
   Its ActualPower (computed in step 0) is attributed to its ledger as untracked
   consumption. This ensures the sum of all AssetLedger totals ≈ SiteMeter totals.

7. PastEnergySum update:
   PastEnergySum += PowerSnapshot(now, SiteMeter.NetImport_kW)
   (Sourced from SiteMeter, NOT from Σ AssetState — meter is authoritative)
```

---

## 3. Timing Summary

| Component | Cycle | Typical Interval | Priority |
|---|---|---|---|
| Asset Controller (measurement) | Periodic | 1–5s | Highest (real-time) |
| Dispatcher | Periodic | 5s | High (real-time) |
| Monitor | Periodic | 5–10s | High |
| OpenADR Interface (polling) | Periodic | 30–60s | Medium |
| OpenADR Interface (webhook) | Event-driven | — | Medium-High |
| External data source refresh | Periodic | 15–60 min (per source) | Low |
| Asset forecast update | Pre-plan | before each plan cycle | Medium |
| Planner (periodic) | Periodic | 5 min (= PlanTimeStep) | Medium |
| Planner (triggered) | Event-driven | — (throttled by ReplanCooldown) | Medium-High |
| Planner (alert) | Event-driven | — (immediate, skip cooldown) | Highest |
| User Request Manager | Event-driven | — | Low-Medium |
| Report generation | Scheduled | per ReportObligation.DueAt | Medium |
| Heuristic learning | Periodic | daily (e.g. end of day) | Lowest |
| Ledger period rollover | Periodic | monthly (e.g. start of billing period) | Lowest |

---

## 4. Data Flow Chains

The most important end-to-end flows through the system:

### 4.1 Price Event → Optimized Schedule
```
VTN publishes PRICE event
  → OpenADR Interface receives via webhook
  → Translates intervals to RateSnapshot[], stores in PlannedRates
  → Emits PlanTrigger.RATE_CHANGE
  → Planner pre-plan: refreshes external data, updates asset forecasts
  → Planner runs optimization against EffectiveCost = ImportPrice + CO2 × CO2Weight
  → Produces Plan with PacketAllocations per PlanTimeSlot
  → Computes per-packet EstimatedCost/CO2/Completion
  → Updates EnergyPacket.PlannedPowerProfile for each packet
  → Updates PlannedEnergySum
  → User Request Manager picks up estimate changes → notifies user if thresholds approached
  → Dispatcher picks up new plan on next cycle
  → Sends DispatchCommands to Asset Controller
  → Asset Controller sends setpoints to devices
```

### 4.2 User Requests EV Charge
```
User: "Charge EV to 80% by tonight, max €1"
  → User Request Manager validates (EV connected, current SoC)
  → Creates UserRequest
  → Translates to EnergyPacket with:
     - TargetEnergy = (0.8 - 0.3) × 50kWh / 0.95 = 26.3 kWh
     - DeadlineTier 1: tonight 19:00, €1.00, MinCompletion 60%
     - DeadlineTier 2 (implicit): far future, €0.00, MinCompletion 0%
  → Emits PlanTrigger.USER_REQUEST
  → Planner pre-plan: updates PV forecast, checks weather
  → Planner schedules packet into cheapest available slots before 19:00
  → Planner post-plan: EstimatedCost=€0.85, EstimatedCO2=340g, EstimatedCompletion=100%
  → User Request Manager generates notification:
     "EV charge to 80% scheduled. Est. cost: €0.85 (budget: €1.00). 
      Est. CO2: 340g. Expected completion: 18:45."
  → Plan produced, Dispatcher executes
```

### 4.3 Device Deviation → Replan
```
PV drops from 10kW to 3kW (sudden cloud cover)
  → Asset Controller measures new ActualPower, updates AssetState
  → Monitor detects: NetDeviation = +7kW (importing 7kW more than planned)
  → Monitor updates AssetLedger: tracks actual import cost at current rate
  → If sustained > SustainedDeviationTime:
     → Emits PlanTrigger.DEVICE_DEVIATION
  → Meanwhile, Dispatcher's auto-follow logic:
     → Battery absorbs some deviation (increases discharge)
     → Reduces net deviation within seconds
  → Planner pre-plan: updates PV forecast (now predicts lower output near-term)
  → Planner re-optimizes remaining horizon
     → May shift some EnergyPackets to later (wait for PV recovery or cheaper slots)
  → Planner post-plan: recalculates all EstimatedCost/CO2 (may have increased)
  → User Request Manager notifies user if any estimate now exceeds threshold
```

### 4.4 Grid Emergency → Immediate Response
```
VTN publishes ALERT_GRID_EMERGENCY event
  → OpenADR Interface receives, translates
  → Emits PlanTrigger.ALERT (skips cooldown)
  → Planner immediately creates high-value synthetic reduction packet
  → Replans: sheds non-critical loads, pauses EV charging, discharges battery
  → Dispatcher executes immediately
  → OpenADR Interface reports compliance via OPERATING_STATE report
```

### 4.5 Capacity Reservation Request
```
Planner determines: tomorrow 18:00-20:00 needs 15kW import but subscription is 10kW
  → Calculates: 5kW additional capacity needed for 2 hours
  → Notifies OpenADR Interface of flexibility need
  → OpenADR Interface checks OadrCapacityState:
     - CAPACITY_AVAILABLE event shows 5kW available at €0.05/kW
  → Sends capacity reservation request report to VTN
  → VTN responds with CAPACITY_RESERVATION event (granted or denied)
  → OpenADR Interface updates OadrCapacityState.ImportReservation_kW
  → Emits PlanTrigger.CAPACITY_CHANGE
  → Planner re-optimizes with new capacity headroom
```

### 4.6 Report Obligation Fulfillment
```
Event with reportDescriptor arrives (e.g. USAGE report due after last interval)
  → OpenADR Interface creates OadrReportObligation with DueAt
  → When DueAt arrives:
     → OpenADR Interface reads PastEnergySum for the event's interval period
     → Groups by OadrResourceName (from AssetProfile)
     → Formats as OpenADR report JSON (resources → intervals → payloads)
     → POSTs report to VTN
     → Marks obligation as Fulfilled
```

---

## 5. Component Boundaries — What Each Component Must NOT Do

| Component | Must NOT... |
|---|---|
| OpenADR Interface | ...make scheduling decisions. Only translates and triggers. |
| OpenADR Interface | ...talk to devices. Only talks to VTN. |
| User Request Manager | ...optimize or schedule. Only translates user intent to EnergyPackets. |
| User Request Manager | ...talk to devices or VTN. |
| Planner | ...send commands to devices. Only produces a Plan. |
| Planner | ...talk to VTN. Only reads translated internal state. |
| Dispatcher | ...modify the Plan. Only executes it (with auto-follow adjustments). |
| Dispatcher | ...talk to VTN. |
| Asset Controller | ...make energy decisions. Only executes commands and reports measurements. |
| Asset Controller | ...talk to VTN. Only talks to devices. |
| Monitor | ...modify plans or send commands. Only observes and triggers. |

---

## 6. Shared State Access Matrix

Which components read (R) and write (W) which entities:

| Entity | OpenADR IF | User Req Mgr | Planner | Dispatcher | Asset Ctrl | Monitor |
|---|---|---|---|---|---|---|
| SiteMeter | R | | | | W | R |
| OadrProgramConfig | W | | R | | | |
| OadrEventCache | W | | R | | | |
| OadrCapacityState | W | | R | | | |
| OadrReportObligation | W | | | | | |
| PlannedRates | W | | R | R | | |
| PastRates | W | | | | | R |
| ExternalDataSources | | | R/W | | | |
| Assets[].Profile | | R | R | R | | |
| Assets[].State | R | R | R | R | W | R |
| Assets[].Heuristics | | R | R/W | | | |
| Assets[].Forecast | R | | W | | | |
| Assets[].Ledger | | | | | | W |
| EnergyPacket (create) | | W | | | | |
| EnergyPacket (plan) | | | W | | | |
| EnergyPacket (estimates) | | R | W | | | |
| EnergyPacket (execute) | | | | W | | |
| EnergyPacket (read) | R | R | R | R | | R |
| ValueCurve | | W | R/W | | | |
| Plan | | | W | R | | R |
| FlexibilityEnvelope[] | R | | W | | | |
| PlannedEnergySum | R | | W | | | R |
| PastEnergySum | R | | | | | W |
| DispatchState | | | | W | | R |
| DispatchCommand | | | | W | R | |
| DeviceSession | | | | W | | R |
| PenaltyRules | | | R | | | R/W |
| UserRequest | | W | | | | |
| UserNotification | | W | | | | W |

---

*End of Step 2 (Draft 5). Changes from Draft 4:*
- *Updated Planner owns (§2.3): Plan is now two-layer (FirmSlots + FlexibleSlots + FlexibilityEnvelopes). PlannedEnergySum covers FIRM slots only. Estimates combine FIRM actuals + FLEXIBLE envelope estimates.*
- *Reworked Post-Planning Step: FlexibilityEnvelope construction (step 5), envelope-based cost/CO2 estimates with acknowledged uncertainty, user notifications note estimate uncertainty for flexible packets*
- *Updated OpenADR IF VEN→VTN reports (§2.1): USAGE_FORECAST distinguishes FIRM (point) vs FLEXIBLE (range). DOWN_REGULATION_AVAILABLE includes FlexibilityEnvelope totals. IMPORT_CAPACITY_RESERVATION includes flexible demand. Added key point: VTN price signals resolve flexibility into commitment.*
- *Updated Dispatcher purpose and logic (§2.4): executes FIRM slots only. Never acts on FLEXIBLE slots.*
- *Added FlexibilityEnvelope to shared state access matrix (§6): OpenADR IF reads, Planner writes*

*Changes from Drafts 1–4 (preserved):*
- *SiteMeter, SITE_RESIDUAL, CompletionPolicy, PenaltyRule thresholds, Alert bid mechanism, AssetForecast, AssetLedger*

*Proceed to Step 3 (Entity Interaction & Data Flow) after review.*
