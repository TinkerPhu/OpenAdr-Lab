# Step 3 — Entity Interaction & Data Flow

**Scope:** Single-site residential HEMS acting as OpenADR 3.1 VEN.  
**Version:** Draft 4  
**Prerequisite:** Step 1 Entity Model (Draft 6), Step 2 Architecture (Draft 5)

---

## 1. Purpose

Step 2 defined what each component owns and reads. This step traces how data actually
moves through the system *over time* — the lifecycle of each entity, what triggers its
creation, modification, and retirement. It also verifies that every entity has at least
one producer and one consumer, and that no data is written without being read.

---

## 2. Entity Lifecycle Summary

For every entity in the model: who creates it, who mutates it, who reads it, and when
it gets retired.

### 2.1 Configuration Entities (long-lived, rarely change)

| Entity | Created by | Mutated by | Read by | Retired when |
|---|---|---|---|---|
| VenController | System startup | Admin config | All components | Never (singleton) |
| AssetProfile (incl. ThermalModelParams, MinSoC) | Admin config | Admin config | Planner, Dispatcher, Asset Ctrl, User Req Mgr, Monitor | Asset physically removed |
| OadrProgramConfig | OpenADR IF (program discovery) | OpenADR IF (program update) | Planner, OpenADR IF | Program ended / unenrolled |
| PenaltyRule (incl. MeasurementWindow) | Admin config / OpenADR IF | Monitor (RollingAverage, BreachedThisPeriod) | Planner, Monitor | Rule deactivated / expired |
| ExternalDataSource | Admin config | Planner (refresh cycle) | Planner (pre-plan step), Asset.UpdateForecast() | Source removed from config |
| RateHeuristic | System startup (defaults) | Planner (daily UpdateHeuristics from PastRates) | Planner (Phase 1 StaleRatePolicy) | Never (continuously updated) |

### 2.2 VTN-Driven Entities (event-driven, hours to days lifecycle)

| Entity | Created by | Mutated by | Read by | Retired when |
|---|---|---|---|---|
| OadrEventCache | OpenADR IF (event received) | — (immutable after creation) | Planner (for context) | Event period elapsed + retention |
| RateSnapshot (Planned) | OpenADR IF (from event intervals) | — (immutable) | Planner, Dispatcher | TimeStamp passes → moved to PastRates |
| RateSnapshot (Past) | OpenADR IF (time transition) | — (immutable) | Monitor (ledger), OpenADR IF (reports), RateHeuristic (learning) | Retention window (e.g. 90 days) |
| OadrCapacityState | OpenADR IF (subscription event) | OpenADR IF (reservation granted/denied) | Planner | Continuously overwritten |
| OadrReportObligation | OpenADR IF (from reportDescriptor) | OpenADR IF (marks Fulfilled / cancelled) | OpenADR IF | Fulfilled + retention |

### 2.3 Asset-Driven Entities (continuous, seconds to minutes lifecycle)

| Entity | Created by | Mutated by | Read by | Retired when |
|---|---|---|---|---|
| SiteMeter | Asset Controller | Asset Controller (each measurement) | Monitor, OpenADR IF | — (continuously overwritten, latest only) |
| AssetState | Asset Controller | Asset Controller (each measurement) | All components | Overwritten each cycle (latest only) |
| AssetState (SITE_RESIDUAL) | Monitor (derived) | Monitor (each cycle: SiteMeter - Σ others) | Planner (via forecast) | Overwritten each cycle |
| AssetHeuristics | Admin default / Planner (UpdateHeuristics) | Planner (daily learning) | Planner (pre-plan), User Req Mgr | Asset removed |
| AssetForecast (incl. AvailabilityWindows) | Planner (pre-plan UpdateForecast) | Planner (each pre-plan step) | Planner (Phase 2 eligibility), OpenADR IF (USAGE_FORECAST) | Overwritten each plan cycle |
| AssetFlexibility | Computed on demand | — (not stored) | Planner, VenController methods | — (transient) |
| AssetLedger | Monitor (period start) | Monitor (each cycle) | User (reporting) | Period ends → archived, new one created |

### 2.4 Planning Entities (plan-cycle lifecycle, ~5 minutes)

| Entity | Created by | Mutated by | Read by | Retired when |
|---|---|---|---|---|
| Plan | Planner | — (immutable after creation) | Dispatcher, Monitor, OpenADR IF | Replaced by next Plan → PlanHistory |
| PlanTimeSlot[] (FIRM, incl. SurplusAvailable, GridEffectiveCost, RateEstimated) | Planner | — | Dispatcher, Monitor | With parent Plan |
| PlanTimeSlot[] (FLEXIBLE, incl. GridEffectiveCost, RateEstimated) | Planner | — | OpenADR IF (flexibility reports) | With parent Plan |
| PacketAllocation[] (incl. SurplusPower, GridPower) | Planner (within FIRM PlanTimeSlot) | — | Dispatcher (cost tracking with surplus/grid split) | With parent PlanTimeSlot |
| FlexibilityEnvelope[] | Planner (within Plan) | — | OpenADR IF (VTN reports), User Req Mgr (estimates) | With parent Plan |
| PlanWarning[] | Planner | — | User Req Mgr (→ notifications) | With parent Plan |
| PlannedEnergySum | Planner | — (rewritten each plan) | OpenADR IF, Monitor | Overwritten each plan cycle (FIRM slots only) |

### 2.5 Scheduling Entities (hours to days lifecycle)

| Entity | Created by | Mutated by | Read by | Retired when |
|---|---|---|---|---|
| EnergyPacket | User Req Mgr (or Planner for alert packets) | Planner (plan fields, thermal TargetEnergy), Dispatcher (execution), Monitor (ABANDONED) | All components | Status → terminal → CompletedPackets |
| ValueCurve | User Req Mgr | Planner (ActiveTierIndex) | Planner | With parent EnergyPacket |
| DeadlineTier[] | User Req Mgr | — (immutable) | Planner | With parent ValueCurve |
| ComfortRate[] | User Req Mgr | — (immutable) | Planner | With parent ValueCurve |
| UserRequest | User Req Mgr | User Req Mgr (user modifies) | User Req Mgr | Linked packets all terminal |

### 2.6 Execution Entities (seconds to hours lifecycle)

| Entity | Created by | Mutated by | Read by | Retired when |
|---|---|---|---|---|
| DispatchState | Dispatcher | Dispatcher (each cycle) | Monitor | Overwritten each dispatch cycle |
| DispatchCommand | Dispatcher | — (new each cycle) | Asset Controller | Next dispatch cycle |
| DeviceSession | Dispatcher (packet starts executing) | Dispatcher (each cycle) | Monitor | Packet completes/fails |
| PastEnergySum | Monitor | Monitor (each cycle) | OpenADR IF (reports), Monitor (penalty checks) | Retention window |

### 2.7 User-Facing Entities (event-driven lifecycle)

| Entity | Created by | Mutated by | Read by | Retired when |
|---|---|---|---|---|
| UserNotification | User Req Mgr / Monitor | — (immutable) | User (UI) | User dismisses / retention |

---

## 3. Data Flow Through Time — One Complete Cycle

This section traces a single "heartbeat" of the system — what happens in one
PlanTimeStep (5 minutes), assuming the system is in steady state with active packets.

### 3.1 Fast Loop: Dispatch Cycle (every 5 seconds)

```
                ┌─────────────────────────────────────────────┐
                │            DISPATCH CYCLE (5s)               │
                │                                             │
  ┌─────────┐   │  ┌──────────┐   ┌────────────┐             │
  │ Devices │◄──┼──│  Asset   │──►│  AssetState │             │
  │         │──►┼──│Controller│   │  (updated)  │             │
  └─────────┘   │  └──────────┘   └──────┬─────┘             │
                │                        │                     │
                │                        ▼                     │
                │  ┌──────────┐   ┌────────────┐              │
                │  │ActivePlan│──►│ Dispatcher  │              │
                │  │.current  │   │             │              │
                │  │ slot     │   └──────┬─────┘              │
                │  └──────────┘          │                     │
                │                        ├── DispatchCommand[]  │
                │                        ├── DeviceSession update│
                │                        ├── EnergyPacket.Past* │
                │                        └── EnergyPacket.Status│
                │                                              │
                │  ┌──────────┐   ┌────────────┐              │
                │  │Dispatch  │──►│  Monitor    │──► triggers? │
                │  │State     │   └────────────┘              │
                │  └──────────┘        │                       │
                │                      ├── PastEnergySum update │
                │                      ├── AssetLedger update   │
                │                      └── UserNotification?    │
                └─────────────────────────────────────────────┘
```

**Step by step:**

```
t=0.000s  Asset Controller polls all devices + grid meter
          → writes AssetState.ActualPower, SoC, Temperature, IsConnected for each asset
          → writes SiteMeter.NetImport_kW, CumulativeImport/Export
          → if IsConnected or Responsiveness changed significantly:
            emits PlanTrigger.ASSET_STATE_CHANGE

t=0.050s  Dispatcher reads current PlanTimeSlot from ActivePlan
          → for each PacketAllocation in slot:
            computes DispatchCommand for target asset
          → for auto-follow assets:
            NetDeviation = Σ(ActualPower) - Σ(PlannedPower)
            distributes compensation across auto-follow assets
          → writes DispatchCommand[] to Asset Controller
          → for each active DeviceSession:
            appends to EnergyPacket.PastPowerProfile
            updates AccumulatedCost using PacketAllocation surplus/grid split:
              surplusFraction × ExportPrice + gridFraction × ImportPrice
            updates AccumulatedCO2 += ActualPower × gridFraction × CO2Rate × dt
            checks completion → may set Status = COMPLETED
          → writes DispatchState

t=0.100s  Monitor reads SiteMeter + AssetState[]
          → computes SITE_RESIDUAL.State.ActualPower =
              SiteMeter.NetImport - Σ(other assets' ActualPower)
          → computes NetDeviation = SiteMeter.NetImport - planned net power
          → if |NetDeviation| > threshold for sustained period:
            emits PlanTrigger.DEVICE_DEVIATION
          → updates PastEnergySum from SiteMeter.NetImport (authoritative)
          → updates AssetLedger for each asset (including SITE_RESIDUAL):
            TotalConsumption, TotalImportCost, TotalCO2, TrackedByPackets
          → checks penalty proximity:
            computes RollingAverage over MeasurementWindow for each PenaltyRule
            compares against threshold (breach check)
          → checks per-packet cost warnings
          → checks tier feasibility (IsOnTrack)
          → checks LatestStart: PENDING packets past LatestStart → ABANDONED
          → checks StaleContinue: CONTINUE packets with no progress for StaleContinueTimeout → ABANDONED
```

This loop runs 60 times per PlanTimeStep (5min ÷ 5s).

### 3.2 Medium Loop: Plan Cycle (every 5 minutes or triggered)

```
                ┌─────────────────────────────────────────────┐
                │            PLAN CYCLE (5min)                  │
                │                                             │
  ┌───────────┐ │  ┌──────────────────┐                       │
  │ External  │─┼─►│ Pre-Plan:        │                       │
  │ Data APIs │ │  │ refresh sources   │                       │
  └───────────┘ │  │ update forecasts  │                       │
                │  └────────┬─────────┘                       │
                │           │                                  │
                │           ▼                                  │
                │  ┌──────────────────┐                       │
                │  │ Inputs:           │                       │
                │  │ · PlannedRates[]  │                       │
                │  │ · ActivePackets[] │                       │
                │  │ · Assets[].State  │                       │
                │  │ · Assets[].Forecast│                      │
                │  │ · CapacityState   │                       │
                │  │ · PenaltyRules[]  │                       │
                │  │ · AutoFollowHeadroom│                     │
                │  └────────┬─────────┘                       │
                │           │                                  │
                │           ▼                                  │
                │  ┌──────────────────┐                       │
                │  │ OPTIMIZE:         │                       │
                │  │ for each slot:    │                       │
                │  │   EffectiveCost = │                       │
                │  │   Price + CO2×W   │                       │
                │  │   rank packets by │                       │
                │  │   MarginalValue   │                       │
                │  │   allocate power  │                       │
                │  └────────┬─────────┘                       │
                │           │                                  │
                │           ▼                                  │
                │  ┌──────────────────┐                       │
                │  │ Post-Plan:        │                       │
                │  │ · Estimated Cost  │──► User Req Mgr      │
                │  │ · Estimated CO2   │    → notifications    │
                │  │ · Tier feasibility│                       │
                │  │ · PlanWarnings    │                       │
                │  └────────┬─────────┘                       │
                │           │                                  │
                │           ▼                                  │
                │  ┌──────────────────┐                       │
                │  │ Outputs:          │                       │
                │  │ · Plan (new)      │──► Dispatcher         │
                │  │ · PlannedEnergySum│──► OpenADR IF         │
                │  │ · EnergyPacket.*  │                       │
                │  │ · PlanWarnings    │──► User Req Mgr       │
                │  └──────────────────┘                       │
                └─────────────────────────────────────────────┘
```

**Step by step:**

```
t=0.0s    Trigger arrives (PERIODIC, RATE_CHANGE, DEVICE_DEVIATION, etc.)
          → check ReplanCooldown (skip for ALERT triggers)

t=0.1s    Pre-Plan: External Data Refresh
          → for each ExternalDataSource where (now - LastFetch > PollInterval):
            fetch new data, update CachedData, set FetchStatus
            if fetch fails: FetchStatus=STALE, add PlanWarning

t=0.5s    Pre-Plan: Asset Forecast Update
          → for each Asset: call UpdateForecast()
            PV:           irradiation × panel specs → production profile
            Heat pump:    outdoor temp × thermal model → consumption profile
            Stove:        heuristic DaytimeProfile → consumption profile
            EV:           heuristic connection pattern → availability + AvailabilityWindows
            Site residual: heuristic DaytimeProfile → unmodeled consumption profile
            Battery:      skip (fully controllable)
          → write AssetForecast (incl. AvailabilityWindows) for each

t=0.7s    Pre-Plan: Thermal TargetEnergy Recomputation
          → for each packet P on a thermal asset with ThermalModelParams:
            recompute P.TargetEnergy_kWh from current temperature, outdoor forecast,
            insulation parameters, and efficiency. (See Step 1 §3.1.1.)

t=1.0s    Build Planning Grid
          → create PlanTimeSlot[] from now to EndTime
          → populate each slot with:
            · rates from PlannedRates[] (ImportPrice, ExportPrice, CO2)
            · if rate missing: apply StaleRatePolicy (LAST_KNOWN / HEURISTIC_FORECAST /
              DEFER_TO_FLEXIBLE / SAFE_AVERAGE). Set RateEstimated = true.
            · GridEffectiveCost = ImportPrice + (CO2Rate × CO2Weight) for all slots
            · capacity limits from CapacityState + RateSnapshot limits
            · baseline: uncontrollable load/production from AssetForecast[]
            · SurplusAvailable_kW = max(0, -BaselineLoad) for each slot

t=2.0s    Optimization (detail in Step 4)
          → classify slots: FIRM (≤ FirmBoundary) vs FLEXIBLE (> FirmBoundary)
          → FIRM slots: greedy allocation
            for each slot, for each active packet:
              compute CalcCache (effective cost, marginal comfort value, time pressure)
              rank packets by EffectivePriority
              allocate power top-down until asset limits / capacity limits exhausted
            handle storage strategy: charge slots vs discharge slots
            handle PV surplus: export vs store decision
            write PacketAllocation[] into each FIRM PlanTimeSlot
          → FLEXIBLE slots: no allocation — build FlexibilityEnvelopes instead
            for each packet with UndeliveredEnergy beyond firm allocations:
              compute envelope: energy needed, power range, time window, rate range
          → write PlannedPowerProfile into each EnergyPacket (FIRM allocations only)

t=3.0s    Post-Plan: Estimate Computation
          → for each EnergyPacket:
            FirmCost = Σ(CostInSlot) for FIRM allocations
            FlexCost = FlexibilityEnvelope.EstimatedCost (avg rate in eligible window)
            EstimatedCost = AccumulatedCost + FirmCost + FlexCost
            EstimatedCO2 = AccumulatedCO2 + FirmCO2 + FlexCO2
            EstimatedCompletion = (PastEnergy + FirmEnergy + FlexEnergy) / TargetEnergy
          → tier feasibility check:
            if EstimatedCost > ActiveTier.MaxTotalCost → fall back
            if EstimatedCompletion < ActiveTier.MinCompletion → fall back
          → budget warnings:
            if EstimatedCost > WarnThreshold → PlanWarning

t=3.5s    Publish Outputs
          → write new Plan with FirmSlots + FlexibleSlots + Envelopes (old plan → PlanHistory)
          → write PlannedEnergySum (FIRM slots only — authoritative for Dispatcher)
          → FlexibilityEnvelopes available for OpenADR IF (flexibility reports to VTN)
          → PlanWarnings available for User Req Mgr

t=4.0s    User Req Mgr processes PlanWarnings
          → for new packets: generate initial estimate notification
          → for tier fallbacks: generate degradation notification
          → for budget warnings: generate cost alert
          → write UserNotifications
```

### 3.3 Slow Loop: OpenADR Event/Report Cycle (event-driven + scheduled)

```
  ┌─────────┐         ┌──────────────┐         ┌──────────────┐
  │   VTN   │◄───────►│  OpenADR IF  │────────►│  Internal    │
  │         │  HTTP    │              │ writes   │  State       │
  └─────────┘         └──────────────┘         └──────────────┘
       │                     │
       │ events in           │ reports out
       │                     │
       ▼                     ▼
  ┌─────────────────────────────────────────────────────────┐
  │                                                         │
  │  INBOUND (VTN → VEN):                                   │
  │                                                         │
  │  PRICE/GHG/EXPORT_PRICE event arrives                   │
  │    → parse intervals                                    │
  │    → for each interval:                                 │
  │        create RateSnapshot with ImportPrice, ExportPrice,│
  │        ImportCO2, capacity limits                        │
  │    → append to PlannedRates[]                           │
  │    → create OadrEventCache                              │
  │    → extract OadrReportObligations from reportDescriptors│
  │    → emit PlanTrigger.RATE_CHANGE                       │
  │                                                         │
  │  CAPACITY_SUBSCRIPTION event arrives                    │
  │    → update CapacityState.ImportSubscription_kW          │
  │    → emit PlanTrigger.CAPACITY_CHANGE                   │
  │                                                         │
  │  CAPACITY_RESERVATION response arrives                  │
  │    → update CapacityState.ImportReservation_kW           │
  │    → emit PlanTrigger.CAPACITY_CHANGE                   │
  │                                                         │
  │  ALERT_* event arrives                                  │
  │    → store in EventCache                                │
  │    → emit PlanTrigger.ALERT (skip cooldown)             │
  │                                                         │
  │  DISPATCH_SETPOINT event arrives                        │
  │    → bypass Planner                                     │
  │    → send directly to Dispatcher as override            │
  │                                                         │
  └─────────────────────────────────────────────────────────┘

  ┌─────────────────────────────────────────────────────────┐
  │                                                         │
  │  OUTBOUND (VEN → VTN):                                  │
  │                                                         │
  │  ReportObligation.DueAt arrives                         │
  │    → determine report type (USAGE, DEMAND, FORECAST...) │
  │    → gather source data:                                │
  │        USAGE:            PastEnergySum per resource      │
  │        DEMAND:           AssetState.ActualPower          │
  │        USAGE_FORECAST:   AssetForecast.Profile           │
  │        STORAGE_*:        AssetState.SoC, Profile.Power   │
  │        OPERATING_STATE:  DeviceResponsiveness            │
  │        CAPACITY_REQ:     GetImportFlexibility()          │
  │    → format as OpenADR report JSON                      │
  │    → POST to VTN                                        │
  │    → mark obligation Fulfilled                          │
  │                                                         │
  │  Flexibility changes significantly after replan          │
  │    → compute new capacity request if needed             │
  │    → POST capacity reservation request to VTN           │
  │                                                         │
  └─────────────────────────────────────────────────────────┘
```

### 3.4 Rare Loops

```
Heuristic Learning (daily, e.g. 23:55):
  → Planner calls UpdateHeuristics() on each asset with Heuristics
  → reads past 24h of AssetState measurements from PastEnergySum
  → updates DaytimeProfile, WeekdayWeights, SeasonalFactor
  → writes AssetHeuristics.LastUpdated

Ledger Period Rollover (monthly):
  → Monitor detects PeriodEnd reached
  → archives current AssetLedger
  → creates new AssetLedger with PeriodStart = now, all counters = 0

Plan History Pruning (periodic):
  → system removes PlanHistory entries older than retention window
  → same for CompletedPackets, PastRates, PastEnergySum
```

---

## 4. EnergyPacket Lifecycle — Complete State Machine

The EnergyPacket is the most complex entity. Here is its full lifecycle with
field mutations at each transition:

```
                 User creates request
                        │
                        ▼
              ┌──────────────────┐
              │     PENDING      │
              │                  │
              │ Created by:      │
              │   User Req Mgr   │
              │                  │
              │ Fields set:      │
              │   PacketID       │
              │   AssetID        │
              │   EarliestStart  │
              │   LatestEnd      │
              │   TargetEnergy   │
              │   ValueCurve     │
              │   RequestMode    │
              │   CompletionPolicy│
              │   PostDeadline   │
              │   ComfortBid     │
              └────────┬─────────┘
                       │ Planner assigns
                       │ to time slots
                       ▼
              ┌──────────────────┐
              │    SCHEDULED     │
              │                  │
              │ Fields set by    │
              │ Planner:         │
              │   PlannedPower   │
              │   Profile        │
              │   ActiveTierIdx  │
              │   EstimatedCost  │
              │   EstimatedCO2   │
              │   EstimatedCompl │
              └───┬──────────┬───┘
                  │          │
    EarliestStart │          │ All tiers infeasible
    reached +     │          │ (Planner)
    dispatch      │          │
    begins        │          ▼
                  │  ┌──────────────────┐
                  │  │    ABANDONED     │
                  │  │                  │
                  │  │ Terminal state.  │
                  │  │ → CompletedPkts  │
                  │  └──────────────────┘
                  │
                  ▼
              ┌──────────────────┐
              │     ACTIVE       │◄──────────── PAUSED
              │                  │              (resume)
              │ Fields updated   │
              │ by Dispatcher    │
              │ each cycle:      │──────────►┌──────────────────┐
              │   PastPower      │  pause    │     PAUSED       │
              │   Profile        │  (VTN,    │                  │
              │   AccumCost      │   user,   │ Temporary halt.  │
              │   AccumCO2       │   conflict│ Device stays     │
              │                  │   )       │ allocated.       │
              │ DeviceSession    │           └──────────────────┘
              │ created and      │
              │ maintained       │
              └─┬──────┬─────┬──┘
                │      │     │
   PastEnergy ≥ │      │     │ Device fails /
   TargetEnergy │      │     │ asset goes OFFLINE
   (Dispatcher) │      │     │ (Dispatcher)
                │      │     │
                ▼      │     ▼
  ┌──────────────┐  │  ┌──────────────────┐
  │  COMPLETED   │  │  │     FAILED       │
  │              │  │  │                  │
  │ Terminal.    │  │  │ Terminal.        │
  │ Fill = 100%  │  │  │ DeviceSession    │
  │ DeviceSession│  │  │ closed.          │
  │ closed.      │  │  │ → Completed      │
  │ → Completed  │  │  │   Packets        │
  │   Packets    │  │  └──────────────────┘
  └──────────────┘  │
                    │  now ≥ LatestEnd
                    │  AND CompletionPolicy = STOP
                    │  AND FillPercentage < 1.0
                    │  (Dispatcher)
                    ▼
          ┌───────────────────────┐
          │  PARTIAL_COMPLETED    │
          │                       │
          │ Terminal.             │
          │ Fill < 100% at hard   │
          │ deadline. Asset freed │
          │ for other packets.    │
          │ DeviceSession closed. │
          │ → CompletedPackets    │
          └───────────────────────┘
```

**CompletionPolicy determines what happens at LatestEnd:**
- **STOP**: Dispatcher checks `now ≥ LatestEnd`. If fill < 100% → PARTIAL_COMPLETED.
  Asset is freed immediately. E.g. battery charge stops, discharge packet can begin.
- **CONTINUE**: LatestEnd passes silently. Packet enters the post-deadline implicit tier
  with PostDeadlineComfortBid as its effective bid. It keeps competing for energy through
  the normal optimization. A washing machine mid-cycle bids high → stays running.
  An EV top-up bids low → only gets free solar energy.
  Eventually: fills to 100% → COMPLETED, or user cancels → ABANDONED.

**The comfort bid is a priority mechanism, not a price.**
A washing machine bidding €5/kWh competes against a grid emergency bidding €5/kWh.
Both enter the same optimization. The Planner sheds the lower-bidding packets first.
The actual cost paid is always the import rate (e.g. €0.30/kWh).

**Field mutation summary by component:**

| Field | User Req Mgr | Planner | Dispatcher |
|---|---|---|---|
| PacketID | CREATE | | |
| AssetID | CREATE | | |
| Status | set PENDING | set SCHEDULED/ABANDONED | set ACTIVE/PAUSED/COMPLETED/PARTIAL_COMPLETED/FAILED |
| EarliestStart | CREATE | | |
| LatestEnd | CREATE | | |
| TargetEnergy_kWh | CREATE | UPDATE (thermal only) | |
| TargetSoC | CREATE | | |
| ValueCurve | CREATE | write ActiveTierIndex | |
| RequestMode | CREATE | | |
| CompletionPolicy | CREATE | | |
| PostDeadlineComfortBid | CREATE | | |
| PlannedPowerProfile | | REWRITE each cycle | |
| PastPowerProfile | | | APPEND each cycle |
| AccumulatedCost_EUR | | | UPDATE each cycle |
| AccumulatedCO2_g | | | UPDATE each cycle |
| EstimatedCost_EUR | | WRITE each cycle | |
| EstimatedCO2_g | | WRITE each cycle | |
| EstimatedCompletion | | WRITE each cycle | |
| LastEstimateAt | | WRITE each cycle | |

---

## 5. RateSnapshot Flow — From VTN to Decision

Tracing a single price signal through the entire system:

```
VTN publishes event:
  { payloadType: "PRICE", values: [0.25], units: "KWH", currency: "EUR" }
  { payloadType: "GHG", values: [380] }
  { payloadType: "EXPORT_PRICE", values: [0.08], units: "KWH", currency: "EUR" }
  for interval: 14:00-15:00

     │
     ▼ OpenADR IF translates

RateSnapshot created:
  TimeStamp:      14:00
  ImportPrice:    { Value: 0.25, Type: PER_KWH, Unit: EUR }
  ExportPrice:    { Value: 0.08, Type: PER_KWH, Unit: EUR }
  ImportCO2:      { Value: 380,  Type: PER_KWH, Unit: g_CO2_eq }
  ImportCapacityLimit: 10.0  (from separate CAPACITY_LIMIT event)
  ExportCapacityLimit: 6.0

     │
     ▼ Planner reads at plan time

PlanTimeSlot [14:00-14:05] populated:
  ImportPrice_per_kWh:  0.25
  ExportPrice_per_kWh:  0.08
  CO2Rate_gPerKWh:      380
  ImportCapacityLimit:   10.0  (subscription + reservation + event limit)
  ExportCapacityLimit:   6.0

     │
     ▼ Planner computes EffectiveCost

  EffectiveCost = 0.25 + (380 × 0.0001)  = 0.288 €/kWh
  (assuming CO2Weight = 0.0001 €/g, i.e. €100/tonne)

     │
     ▼ Planner compares with packet MarginalValue

  EV packet: ComfortRate at 40% fill → MaxMarginalPrice = 0.30 €/kWh
  0.288 < 0.30 → allocate power to EV in this slot

     │
     ▼ PacketAllocation written

  PacketID:           "ev-charge-001"
  AllocatedPower_kW:  7.0
  MarginalValue:      0.30
  CostInSlot_EUR:     7.0 × 0.25 × (5/60) = 0.146 €
  CO2InSlot_g:        7.0 × 380 × (5/60) = 221.7 g

     │
     ▼ Post-plan: accumulated into EnergyPacket

  EstimatedCost_EUR += 0.146
  EstimatedCO2_g += 221.7

     │
     ▼ Dispatcher executes at 14:02

  DispatchCommand: { AssetID: "ev", CommandedPower: 7.0, SourcePacketID: "ev-charge-001" }

     │
     ▼ Asset Controller sends to EV charger, measures actual

  AssetState: ActualPower = 6.8 kW (slight deviation)

     │
     ▼ Dispatcher records actual

  EnergyPacket.PastPowerProfile += { TimeStamp: 14:02, Power: 6.8, Cumulative: ... }
  AccumulatedCost_EUR += 6.8 × 0.25 × (5/60) = 0.142 €
  AccumulatedCO2_g += 6.8 × 380 × (5/60) = 215.3 g

     │
     ▼ Monitor records in PastEnergySum and AssetLedger

  PastEnergySum += { TimeStamp: 14:02, Power: +6.8 }  (net site import)
  AssetLedger["ev"].TotalConsumption += 6.8 × (5/60) = 0.567 kWh
  AssetLedger["ev"].TotalImportCost += 0.142 €
  AssetLedger["ev"].TotalCO2 += 215.3 g
  AssetLedger["ev"].TrackedByPackets += 0.567 kWh

     │
     ▼ Later: OpenADR IF sends USAGE report to VTN

  report resource "ev": interval 14:00, USAGE = 0.567 kWh
```

---

## 6. Cross-Cutting Flow: Capacity Reservation

Traces how a capacity need detected by the Planner flows through to VTN and back:

```
Planner detects: tomorrow 18:00-20:00 slot needs 15kW import

     │
     ▼ Planner reads

  CapacityState.ImportSubscription = 10kW
  CapacityState.ImportReservation = 0kW (none granted)
  Available = 10kW, Need = 15kW, Shortfall = 5kW

     │
     ▼ Planner sets PlanWarning

  "Insufficient import capacity 18:00-20:00. Need 5kW additional."

     │
     ▼ OpenADR IF reads PlannedEnergySum, detects capacity shortfall

  Checks CAPACITY_AVAILABLE event: 5kW available at €0.05/kW
  Decision: cost of reservation (€0.05 × 5 × 2h) = €0.50
  vs. cost of rescheduling packets to avoid the need
  (this decision is informed by Planner's alternatives)

     │
     ▼ OpenADR IF sends capacity reservation request report

  POST report to VTN:
  { payloadType: "IMPORT_CAPACITY_RESERVATION",
    values: [15],  // requesting 15kW total (sub 10 + extra 5)
    interval: 18:00-20:00 }

     │
     ▼ VTN responds with CAPACITY_RESERVATION event

  { payloadType: "IMPORT_CAPACITY_RESERVATION",
    values: [15],  // granted
    payloadType: "IMPORT_CAPACITY_RESERVATION_FEE",
    values: [0.05] }

     │
     ▼ OpenADR IF processes response

  CapacityState.ImportReservation = 5kW (additional)
  Emits PlanTrigger.CAPACITY_CHANGE

     │
     ▼ Planner re-optimizes

  Slot 18:00-20:00 now has 15kW capacity available
  Schedules EV + heater in that window
  Reservation cost (€0.50) added to plan cost
```

---

## 7. Cross-Cutting Flow: PV Surplus Decision

Traces how PV production interacts with storage, export, and self-consumption:

```
Plan cycle at 12:00. Sunny day. PV forecast = 9kW production.

     │
     ▼ Planner builds slot for 12:00-12:05

  AssetForecast["pv"].Profile[12:00] = -9.0 kW  (producing)
  Baseline load from forecasts:
    Heat pump:   2.0 kW  (maintaining temperature)
    Stove:       0.0 kW  (not expected now)
    Standby:     0.3 kW  (various devices)
  Net baseline: 2.3 kW consumption
  PV surplus = 9.0 - 2.3 = 6.7 kW available

     │
     ▼ Planner decides what to do with 6.7 kW surplus

  Option A: Export now at ExportPrice = €0.08/kWh
    Revenue = 6.7 × 0.08 × (5/60) = €0.045

  Option B: Store in battery for later self-consumption
    Future slot 19:00: ImportPrice = €0.30/kWh, CO2 = 450g/kWh
    Displaced cost = 6.7 × 0.30 × (5/60) × 0.92 (round-trip eff) = €0.154
    Charge cost = 6.7 × 0.08 × (5/60) = €0.045 (opportunity cost: forgone export)
    Net value = 0.154 - 0.045 = €0.109

  Option C: Charge EV (active packet, currently at 45% fill)
    EV packet MarginalValue at 45% fill = €0.25/kWh
    EffectiveCost of PV self-consumption = ExportPrice = €0.08/kWh (opportunity cost)
    Value = saves (ImportPrice - ExportPrice) = €0.17/kWh vs future grid import

     │
     ▼ Surplus-aware EffectiveCost sort (Phase 3) allocates highest value first

  1. EV gets 6.7 kW from PV surplus (EffectiveCost = €0.08)
     → cheaper than any grid import slot → greedy sort puts this first
     EV needs 7kW but only 6.7 kW surplus → remaining 0.3 kW from grid
  
  2. If surplus remained: battery charges (store for 19:00)
  3. If surplus still remained: export at €0.08/kWh

     │
     ▼ PacketAllocations

  EV:  AllocatedPower = 7.0 kW
       SurplusPower = 6.7 kW, GridPower = 0.3 kW
       CostInSlot = 6.7×0.08×(5/60) + 0.3×0.25×(5/60) = €0.045 + €0.004 = €0.049
       CO2InSlot = 0.3 × 380 × (5/60) = 19.0 g    (only the grid portion has CO2)
```

---

## 8. Producer/Consumer Verification

Every entity must have at least one producer (W) and one consumer (R).
Entities with no consumer are dead data. Entities with no producer are undefined.

| Entity | Producer(s) | Consumer(s) | Status |
|---|---|---|---|
| VenController | System startup | All | ✓ |
| SiteMeter | Asset Controller | Monitor, OpenADR IF | ✓ |
| AssetProfile | Admin config | Planner, Dispatcher, Asset Ctrl, User Req Mgr, Monitor | ✓ |
| AssetState | Asset Controller | All components | ✓ |
| AssetState (SITE_RESIDUAL) | Monitor (derived from SiteMeter - Σ assets) | Planner (via forecast), Monitor (ledger) | ✓ |
| AssetHeuristics | Planner (UpdateHeuristics) | Planner (pre-plan), User Req Mgr (implicit packets) | ✓ |
| AssetForecast | Planner (UpdateForecast) | Planner (optimization), OpenADR IF (USAGE_FORECAST) | ✓ |
| AssetFlexibility | Computed on demand | Planner, VenController methods (HasAutoFollow, flexibility) | ✓ |
| AssetLedger | Monitor | User (reporting) | ✓ |
| OadrProgramConfig | OpenADR IF | Planner, OpenADR IF | ✓ |
| OadrEventCache | OpenADR IF | Planner (context) | ✓ |
| OadrCapacityState | OpenADR IF | Planner | ✓ |
| OadrReportObligation | OpenADR IF | OpenADR IF (self — tracks fulfillment) | ✓ |
| OadrCapacityRequest | OpenADR IF | OpenADR IF (sends to VTN) | ✓ |
| RateSnapshot (Planned) | OpenADR IF | Planner, Dispatcher | ✓ |
| RateSnapshot (Past) | OpenADR IF (time transition) | Monitor (ledger), OpenADR IF (reports) | ✓ |
| ExternalDataSource | Admin config / Planner refresh | Planner (pre-plan), Asset.UpdateForecast() | ✓ |
| EnergyPacket | User Req Mgr | Planner, Dispatcher, Monitor, OpenADR IF | ✓ |
| ValueCurve | User Req Mgr | Planner | ✓ |
| ComfortRate | User Req Mgr | Planner | ✓ |
| DeadlineTier | User Req Mgr | Planner | ✓ |
| Plan | Planner | Dispatcher (FIRM), OpenADR IF (envelopes), Monitor | ✓ |
| PlanTimeSlot (FIRM) | Planner | Dispatcher, Monitor | ✓ |
| PlanTimeSlot (FLEXIBLE) | Planner | OpenADR IF (flexibility reports) | ✓ |
| PacketAllocation | Planner | Dispatcher | ✓ |
| FlexibilityEnvelope | Planner | OpenADR IF (VTN reports), User Req Mgr (estimates) | ✓ |
| PlanWarning | Planner | User Req Mgr | ✓ |
| PlannedEnergySum | Planner (FIRM slots only) | OpenADR IF, Monitor | ✓ |
| PastEnergySum | Monitor (from SiteMeter, authoritative) | OpenADR IF (USAGE reports), Monitor (penalty checks) | ✓ |
| DispatchState | Dispatcher | Monitor | ✓ |
| DispatchCommand | Dispatcher | Asset Controller | ✓ |
| DeviceSession | Dispatcher | Monitor | ✓ |
| PenaltyRule | Admin config | Planner, Monitor | ✓ |
| PenaltyThreshold | Admin config (within PenaltyRule) | Planner, Monitor | ✓ |
| UserRequest | User Req Mgr | User Req Mgr (self — tracking) | ✓ |
| UserNotification | User Req Mgr / Monitor | User (UI) | ✓ |
| PowerRange | Admin config (within AssetProfile) | Planner, Dispatcher | ✓ |
| EnergySnapshot | Planner (planned), Dispatcher (past) | Planner, Dispatcher, Monitor | ✓ |
| PowerSnapshot | Various (PlannedEnergySum, PastEnergySum, forecasts) | Various | ✓ |

**Result: All entities have at least one producer and one consumer. No orphaned data.**

---

## 9. Concurrency Notes

Three components write to EnergyPacket, touching different fields:

| Component | Fields Written | Cycle |
|---|---|---|
| User Req Mgr | create all fields, modify ValueCurve | Event-driven (rare) |
| Planner | PlannedPowerProfile, ActiveTierIndex, Estimated*, TargetEnergy (thermal only), Status(SCHEDULED/ABANDONED) | Every 5 min |
| Dispatcher | PastPowerProfile, AccumulatedCost/CO2, Status(ACTIVE/COMPLETED/FAILED) | Every 5 sec |
| Monitor | Status(ABANDONED: LatestStart missed, StaleContinue timeout) | Every 5-10 sec |

**No field is written by more than one component** except Status, which has strictly
ordered transitions — each state can only be reached from specific predecessor states
owned by specific components.

The Status field has a clear ownership chain:
```
PENDING → SCHEDULED          (Planner only)
SCHEDULED → ACTIVE           (Dispatcher only)
ACTIVE → PAUSED              (Dispatcher only)
PAUSED → ACTIVE              (Dispatcher only)
ACTIVE → COMPLETED           (Dispatcher only — FillPercentage = 1.0)
ACTIVE → PARTIAL_COMPLETED   (Dispatcher only — LatestEnd reached, CompletionPolicy = STOP, fill < 1.0)
ACTIVE → FAILED              (Dispatcher only)
SCHEDULED → ABANDONED        (Planner only — tier infeasible)
PENDING → ABANDONED          (Planner — infeasible, OR Monitor — LatestStart missed)
ACTIVE → ABANDONED           (Monitor only — StaleContinue timeout, no progress for StaleContinueTimeout)
```

No ambiguity, no races.

---

## 10. Data Retention Summary

How long each data type is kept:

| Data | Active Lifecycle | Retention After | Purpose |
|---|---|---|---|
| SiteMeter | Overwritten each cycle | Latest only | Grid meter reading |
| AssetState | Overwritten each cycle | Latest only | Real-time monitoring |
| AssetForecast | Overwritten each plan cycle | Latest only | Planning input |
| DispatchState | Overwritten each cycle | Latest only | Monitor reads |
| DispatchCommand | Overwritten each cycle | Latest only | Asset Controller reads |
| PlannedEnergySum | Overwritten each plan | Latest only | OpenADR IF, Monitor |
| ActivePlan | Until replaced | PlanHistory (24-48h) | Diagnostics |
| ActivePackets | Until terminal | CompletedPackets (30 days) | Reporting, user history |
| PastEnergySum | Continuous | 90 days | OpenADR USAGE reports, penalty checks |
| PastRates | Continuous | 90 days | Ledger cost attribution, reporting |
| OadrEventCache | Until event period ends | 30 days | Audit trail |
| OadrReportObligation | Until fulfilled | 30 days | Audit trail |
| AssetLedger | Current billing period | Archived indefinitely | User cost history |
| UserNotification | Until dismissed | 30 days | User review |
| AssetHeuristics | Permanent (continually updated) | — | Learning |

---

*End of Step 3 (Draft 4). Changes from Draft 3:*
- *Updated medium loop optimization step (§3.2): two-layer plan. FIRM slots get greedy allocation. FLEXIBLE slots produce FlexibilityEnvelopes. Post-plan estimates combine FIRM actuals + FLEXIBLE averages. Publish step outputs envelopes to OpenADR IF.*
- *Updated entity lifecycle (§2.4): PlanTimeSlot split into FIRM (→ Dispatcher, Monitor) and FLEXIBLE (→ OpenADR IF). Added FlexibilityEnvelope[] lifecycle: Planner creates, OpenADR IF and User Req Mgr consume. PlannedEnergySum noted as FIRM-only.*
- *Updated producer/consumer verification (§8): FlexibilityEnvelope verified. PlanTimeSlot(FLEXIBLE) consumers verified. Plan consumers expanded to include OpenADR IF for envelopes.*

*Changes from Drafts 1–3 (preserved):*
- *SiteMeter, SITE_RESIDUAL, PARTIAL_COMPLETED, CompletionPolicy, bid-as-priority*

*Proceed to Step 4 (Planning Algorithm) after review.*
