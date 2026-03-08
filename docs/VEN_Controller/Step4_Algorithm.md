# Step 4 — Planning Algorithm

**Scope:** Single-site residential HEMS acting as OpenADR 3.1 VEN.  
**Version:** Draft 4  
**Prerequisite:** Step 1 Entity Model (Draft 6), Step 2 Architecture (Draft 5), Step 3 Data Flow (Draft 4)

---

## 1. Purpose

Step 2 defined *what* the Planner does. This step defines *how*: the algorithm that
takes rates, packets, assets, forecasts, and constraints and produces an optimal Plan.

The algorithm is a priority-based greedy scheduler, not a full mathematical optimizer
(LP/MILP). This is a deliberate choice for residential scale — planning horizons are
short (24–48h), asset count is small (3–15), and replanning is frequent (every 5 min).
A greedy approach with good heuristics produces near-optimal results and runs in
milliseconds, which matters when replanning is triggered by real-time events.

---

## 2. Algorithm Overview

```
┌──────────────────────────────────────────────────────────────┐
│                   PLANNING ALGORITHM                          │
│                                                              │
│  Phase 1: PREPARE                                            │
│    Build planning grid (slots × rates × limits)              │
│    Classify slots: FIRM vs FLEXIBLE                          │
│    Populate baseline from forecasts                          │
│    Classify assets and packets                               │
│                                                              │
│  Phase 2: SCORE (FIRM slots only)                            │
│    For each (packet, FIRM slot) pair:                        │
│      Compute CalcCache: slot cost, comfort bid, time         │
│      pressure, eligibility                                   │
│                                                              │
│  Phase 3: ALLOCATE CONSUMPTION (FIRM slots only)             │
│    Sort eligible (packet, slot) pairs by EffectivePriority   │
│    Greedy allocation respecting hard constraints             │
│                                                              │
│  Phase 4: ALLOCATE STORAGE (FIRM slots only)                 │
│    Identify charge/discharge opportunities                   │
│    Apply round-trip efficiency test                          │
│                                                              │
│  Phase 5: ALLOCATE RESIDUAL PV SURPLUS (FIRM slots only)       │
│    Export unclaimed surplus, handle curtailment                 │
│                                                              │
│  Phase 6: PENALTY CHECK (FIRM slots only)                    │
│    Evaluate discrete penalty thresholds                      │
│    Reschedule if avoidance is cheaper than breach            │
│                                                              │
│  Phase 7: BUILD FLEXIBILITY ENVELOPES (far horizon)          │
│    For each packet with unallocated energy:                  │
│      Characterize flexible demand window                     │
│      Compute rate range, budget remaining, estimates         │
│                                                              │
│  Phase 8: FINALIZE                                           │
│    Write FIRM PacketAllocations                              │
│    Write FlexibilityEnvelopes                                │
│    Compute slot summaries and estimates                      │
│    Detect warnings                                           │
│                                                              │
└──────────────────────────────────────────────────────────────┘
```

---

## 3. CalcCache — The Transient Scoring Structure

CalcCache is built per-packet-per-slot during Phase 2, used for ranking in Phase 3,
then discarded. The only surviving output is `PacketAllocation.MarginalValue`.

```
CalcCache
    PacketID:           string
    SlotIndex:          int

    // --- Slot Conditions ---
    EffectiveCost:      float       // surplus-aware cost for this packet in this slot (see §6)
                                    // Pure grid import: ImportPrice + (ImportCO2 × CO2Weight)
                                    // Pure PV self-consumption: ExportPrice (opportunity cost)
                                    // Blended: weighted average of the above
    EffectiveRevenue:   float       // ExportPrice (for export/discharge decisions)

    // --- Surplus Breakdown ---
    SurplusForPacket_kW: float      // kW from PV surplus available to this packet in this slot
    ImportForPacket_kW:  float      // kW from grid import needed for this packet in this slot
                                    // SurplusForPacket + ImportForPacket = MaxPower (tentative)

    // --- Packet State at This Slot ---
    ProjectedFill:      float       // projected FillPercentage when this slot begins
                                    // = (PastEnergy + planned energy in prior slots) / TargetEnergy
    EnergyToAllocate:   float       // energy still needing slot assignment at this point in the scoring loop
    SlotsUntilDeadline: int         // how many FIRM slots remain before active tier deadline

    // --- Value Computation ---
    ComfortBid:         float       // interpolated from ComfortRate[] at ProjectedFill
    TimePressure:       float       // urgency factor (see §4)
    MarginalValue:      float       // = ComfortBid × TimePressure (the effective priority)

    // --- Eligibility ---
    Eligible:           bool        // passes all hard constraints for this slot
    SkipReason:         string?     // why ineligible (for diagnostics)

    // --- Budget Check ---
    CostIfAllocated:    float       // EffectiveCost × power × dt (what this slot would actually cost)
    BudgetRemaining:    float       // ActiveTier.MaxTotalCost - AccumulatedCost - planned cost so far
    WithinComfortBid:   bool        // EffectiveCost ≤ ComfortBid (fill-based cost ceiling)
    WithinMarginalRate: bool        // EffectiveCost ≤ ActiveTier.MaxMarginalRate (tier ceiling)
    WithinBudget:       bool        // CostIfAllocated ≤ BudgetRemaining
```

---

## 4. MarginalValue Computation

MarginalValue is the single number that determines allocation priority. Higher value
wins when packets compete for the same slot or capacity.

### 4.1 ComfortBid: What the User Values

Interpolated from the packet's ComfortRate[] at current ProjectedFill:

```
Given ComfortRate[] = [
    { FillPercentage: 0.0, MaxMarginalPrice: 0.40 },
    { FillPercentage: 0.5, MaxMarginalPrice: 0.30 },
    { FillPercentage: 0.8, MaxMarginalPrice: 0.15 },
    { FillPercentage: 1.0, MaxMarginalPrice: 0.05 },
]

At ProjectedFill = 0.65:
  → between 0.5 (0.30) and 0.8 (0.15)
  → linear interpolation: 0.30 + (0.65 - 0.5) / (0.8 - 0.5) × (0.15 - 0.30)
  → ComfortBid = 0.225 €/kWh

For post-deadline CONTINUE packets:
  → ComfortBid = PostDeadlineComfortBid (flat, no fill-based curve)
```

This means: at the start (empty battery), the user bids high — they really want energy.
As fill increases, the bid drops — each additional kWh is less valuable. At 100%, the
bid is very low — only take energy if it's nearly free.

### 4.2 TimePressure: How Urgent Is This Slot

TimePressure increases as the deadline approaches and the packet is behind schedule.
It amplifies the ComfortBid to push urgent packets ahead of comfortable ones.

```
TimeSlack = FirmSlotsUntilDeadline - SlotsNeededToComplete
  where SlotsNeededToComplete = EnergyToAllocate / (MaxPower × StepSize)
  and FirmSlotsUntilDeadline counts only FIRM slots before active tier deadline

If TimeSlack ≤ 0:
  TimePressure = 3.0       // critical: not enough slots even at max power
If TimeSlack = 1:
  TimePressure = 2.0       // tight: exactly one slot margin
If TimeSlack ≤ 3:
  TimePressure = 1.5       // pressure: a few slots margin
Else:
  TimePressure = 1.0       // comfortable: plenty of time

For post-deadline CONTINUE packets:
  TimePressure = 1.0       // no deadline pressure (bid alone determines priority)

For STOP packets past LatestEnd:
  Not in ActivePackets (already PARTIAL_COMPLETED). Not scored.
```

### 4.3 MarginalValue: The Final Priority

```
MarginalValue = ComfortBid × TimePressure
```

Examples:
- EV at 20% fill, 2 hours to deadline, 3 hours needed → ComfortBid 0.35 × TimePressure 3.0 = **1.05**
- EV at 70% fill, 8 hours to deadline → ComfortBid 0.18 × TimePressure 1.0 = **0.18**
- Washing machine past deadline, CONTINUE, bid €5.00 → 5.00 × 1.0 = **5.00**
- Grid emergency synthetic packet → bid €5.00 × TimePressure 3.0 = **15.00**

### 4.4 Eligibility: ComfortBid as Both Priority and Cost Ceiling

The ComfortBid serves two roles simultaneously:
1. **Priority**: higher bid wins when packets compete for scarce capacity in a slot.
2. **Cost ceiling**: the user won't pay more per kWh than the bid expresses.

This means a slot is only eligible if its EffectiveCost does not exceed the bid:

```
Eligibility for slot (all must pass):
  EffectiveCost ≤ ComfortBid                    // "do I value this energy enough for this slot?"
  EffectiveCost ≤ ActiveTier.MaxMarginalRate     // tier-level ceiling (coarse filter)
  CostIfAllocated + planned costs ≤ MaxTotalCost // "can I afford this slot?"
  CO2Rate ≤ ComfortRate.MaxMarginalCO2           // "is this slot clean enough?"

If eligible: compete on MarginalValue for scarce capacity.
If not eligible: skip this slot entirely.
```

The ComfortBid gate and the tier MaxMarginalRate gate are complementary:
- MaxMarginalRate is the user's coarse budget control ("never pay more than €0.30/kWh")
- ComfortBid is fill-dependent ("at 80% full, I only value the next kWh at €0.15")
- The effective ceiling is min(ComfortBid, MaxMarginalRate)

At low fill (empty battery), ComfortBid is high → many slots qualify → high priority too.
At high fill (nearly full), ComfortBid drops → expensive slots become ineligible AND
priority drops. The value curve naturally tightens both eligibility and priority as
the packet fills up.

Example: EV at 80% fill, ComfortBid = €0.15/kWh
  Slot at 14:00: EffectiveCost = €0.235 → 0.235 > 0.15 → ineligible (too expensive for this fill level)
  Slot at 22:00: EffectiveCost = €0.148 → 0.148 < 0.15 → eligible, and MarginalValue = 0.15 × 1.0 = 0.15

This ensures the EV naturally gravitates to cheap off-peak slots as it fills up,
without any special scheduling logic. The value curve does all the work.

---

## 5. Phase 1: Prepare

```
Input:
  PlannedRates[]           — rate landscape over horizon
  ActivePackets[]          — all non-terminal packets
  Assets[]                 — with profiles, states, forecasts
  CapacityState            — subscription + reservation limits
  PenaltyRules[]           — with BreachedThisPeriod state

Output:
  PlanningGrid: PlanTimeSlot[] with external conditions and Commitment populated
  BaselineLoad: per-slot net uncontrollable load from forecasts
  PacketSet: classified packets ready for scoring
  FirmBoundary: TimeStamp dividing FIRM from FLEXIBLE slots

Steps:

1. Build horizon:
   StartTime = now (truncated to StepSize)
   EndTime = max(StartTime + MinPlanTime, max(packet.LatestEnd for STOP packets),
                 max(packet.LatestEnd + ContinueHorizonExtension for CONTINUE packets))
   Create PlanTimeSlot[] array

2. Classify slots (FIRM vs FLEXIBLE):
   GlobalFirmBoundary = now + NearHorizonDuration

   For each slot:
     If slot.TimeStamp < GlobalFirmBoundary:
       slot.Commitment = FIRM              // within global near-horizon window

     Else:
       // Check if any packet forces this slot to be FIRM (urgency-based)
       forceFirm = false
       For each packet P eligible for this slot:
         If TimePressure(P, slot) ≥ 2.0:   // packet is running out of time
           forceFirm = true
           break
       If forceFirm:
         slot.Commitment = FIRM
       Else:
         slot.Commitment = FLEXIBLE

   FirmBoundary = max TimeStamp of all FIRM slots

   // Early firm-up heuristic: if all FLEXIBLE slots for a packet have
   // identical GridEffectiveCost (flat rate, no VTN variation), there's no value
   // in holding flexibility. Convert those slots to FIRM for that packet.
   // This is evaluated per-packet in Phase 2/3 rather than globally here.

3. Populate slot conditions:
   For each slot:
     Look up RateSnapshot for this time → ImportPrice, ExportPrice, CO2
     If RateSnapshot exists:
       Use directly.
     Else (rate data missing — VTN hasn't published, or connection lost):
       Switch(StaleRatePolicy):
         LAST_KNOWN:          slot rates = last known RateSnapshot values
         HEURISTIC_FORECAST:  slot rates = RateHeuristic.predict(slot.TimeOfDay, DayOfWeek)
         DEFER_TO_FLEXIBLE:   slot.Commitment = FLEXIBLE (override, regardless of near-horizon)
                              slot rates = last known (for envelope estimation only)
         SAFE_AVERAGE:        slot rates = historical 80th percentile for this time of day
       slot.RateEstimated = true  // flagged for PlanWarning generation
     
     // Compute slot-level GridEffectiveCost for ALL slots (FIRM and FLEXIBLE):
     slot.GridEffectiveCost = slot.ImportPrice + (slot.CO2Rate × CO2Weight)
     // This is the cost assuming pure grid import. Used for:
     //   - FLEXIBLE slot scoring (early firm-up variance, Phase 7 envelopes)
     //   - Phase 4 storage profitability comparison
     //   - Fallback when per-packet surplus-aware cost is not available
     // Phase 2 computes per-packet surplus-aware EffectiveCost for FIRM slots only.
     
     Look up capacity limits (min of subscription + reservation, event limits)

4. Populate baseline:
   For each asset with AssetForecast.Source ≠ NONE:
     For each slot:
       slot.BaselineLoad += AssetForecast.Profile[slot.time]
   Uncontrollable consumption is positive, uncontrollable production is negative.
   This includes:
     - PV forecast (negative: production)
     - Cooking stove forecast (positive: heuristic consumption)
     - Heat pump forecast (positive: physical model)
     - SITE_RESIDUAL forecast (positive: learned unmodeled consumption)
   The SITE_RESIDUAL ensures the baseline accounts for real site consumption
   even when most devices are unmodeled (fridge, lights, router, etc.).
   On a fresh install, SITE_RESIDUAL defaults to a flat configurable value
   (e.g. 0.5 kW) that improves over the first few days as heuristics learn.

   After computing baseline:
     slot.SurplusAvailable_kW = max(0, -slot.BaselineLoad)  // kW of PV surplus above fixed loads
     slot.ImportNeeded_kW = max(0, slot.BaselineLoad)        // kW already committed to grid import
   
   SurplusAvailable is a shared pool: multiple packets claiming surplus in the
   same slot must compete for it (tracked in Phase 3 as slotRemainingSurplus).

4a. Thermal TargetEnergy recomputation:
   For each packet P on a thermal asset with ThermalModelParams:
     P.TargetEnergy_kWh = ThermalModelParams.compute(
       currentTemp = Asset.State.Temperature_C,
       targetTemp = from P's UserRequest.TargetTemperature_C,
       outdoorForecast = ExternalDataSource (weather) over planning horizon,
       efficiency = Asset.Profile.Efficiency
     )
   This ensures thermal packets track changing conditions (outdoor temp, heat loss).

5. Classify packets:
   ConsumptionPackets: packets on consuming assets (EV, heater, washing machine)
   StoragePackets:     packets on bidirectional assets (battery charge/discharge)
   ExportPackets:      synthetic packets for PV surplus export decisions

6. Classify assets:
   Controllable: assets with PowerAdjustability ≠ NONE and Responsiveness = RESPONSIVE
   AutoFollow:   controllable assets with AutoFollow = true
   FixedLoad:    everything else (included in baseline)
   
7. Asset availability per slot:
   For STOP packets: asset is blocked for this packet until LatestEnd,
     then freed for other packets in subsequent slots.
   For assets shared by multiple packets: track per-slot asset commitment.
```

### 5.1 When to Hold Flexibility vs Firm Up Early

The default is: FLEXIBLE slots stay flexible, reported to VTN as available demand.
But holding flexibility has a cost — we miss the opportunity to lock in known-cheap slots.
The heuristic:

```
Hold flexibility (keep FLEXIBLE) when:
  - Rate data is incomplete for the window (VTN hasn't published prices yet)
  - Rate variation exists in the window (some slots significantly cheaper than others)
  - Packet has sufficient TimeSlack (no urgency to commit)

Firm up early (treat FLEXIBLE as FIRM) when:
  - All FLEXIBLE slots for a packet have similar GridEffectiveCost (nothing to gain by waiting)
  - TimeSlack is low (must commit to ensure completion)
  - CompletionPolicy = STOP and deadline is approaching (can't risk late commitment)

Implementation: during Phase 2 scoring, if a packet's eligible FLEXIBLE slots have
rate variance < threshold (e.g. < 10% coefficient of variation), treat them as FIRM
for that packet. This is a per-packet decision, not a global slot reclassification.
```

---

## 6. Phase 2: Score

Build CalcCache for every eligible (packet, FIRM slot) pair.
FLEXIBLE slots are NOT scored — they are handled in Phase 7 (FlexibilityEnvelopes).

```
// Initialize surplus tracking for tentative forward projection
surplusTentativelyClaimed[S] = 0    (for each FIRM slot S)

For each packet P in ActivePackets:
  ProjectedFill = P.FillPercentage()    // start with current actual fill
  PlannedCostSoFar = P.AccumulatedCost  // start with actual accumulated cost
  EnergyToAllocate = P.UndeliveredEnergy()  // total energy needing slot assignments

  // Check for early firm-up: if this packet's eligible FLEXIBLE slots all have
  // similar GridEffectiveCost (variance < threshold), treat them as FIRM for this packet.
  eligibleFlexSlots = [S for S in FlexibleSlots where P is eligible for S]
  If eligibleFlexSlots not empty:
    rateVariance = coefficientOfVariation(S.GridEffectiveCost for S in eligibleFlexSlots)
    If rateVariance < 0.10:   // < 10% variation → no value in waiting
      earlyFirmSlots = eligibleFlexSlots  // treat as FIRM for this packet only
    Else:
      earlyFirmSlots = []

  scoringSlots = FirmSlots + earlyFirmSlots  // sorted chronologically

  For each slot S in scoringSlots (chronological order):

    // --- Eligibility gates ---

    If S.TimeStamp < P.EarliestStart:
      Skip (not yet eligible)

    If P.LatestStart is set AND now > P.LatestStart AND P.Status = PENDING:
      Skip (missed latest start — Monitor will ABANDON this packet)

    If P.CompletionPolicy = STOP AND S.TimeStamp ≥ P.LatestEnd:
      Skip (past hard deadline)

    If EnergyToAllocate ≤ 0:
      Skip (already fully planned in FIRM slots)

    Asset = Assets[P.AssetID]
    If Asset.State.IsConnected = false:
      Skip (device not available)
    If Asset.State.Responsiveness = UNRESPONSIVE or OFFLINE:
      Skip (device not controllable)
    If Asset.Forecast.AvailabilityWindows is not null:
      If S.TimeStamp is not within any AvailabilityWindow:
        Skip (asset forecast to be unavailable — e.g. EV expected to disconnect)
      // Replan on ASSET_STATE_CHANGE corrects stale availability forecasts.

    // --- Determine effective bid ---

    If P is past LatestEnd (CONTINUE policy, post-deadline):
      ComfortBid = P.PostDeadlineComfortBid
      TimePressure = 1.0
    Else:
      ComfortBid = interpolate(P.ValueCurve.ComfortRates, ProjectedFill)
      TimePressure = computeTimePressure(P, S)

    MarginalValue = ComfortBid × TimePressure

    // --- Budget and rate gates ---

    // Surplus-aware EffectiveCost:
    // PV surplus in this slot is not free — it has an opportunity cost (forgone export revenue).
    // The site either imports from grid or self-consumes PV surplus. Never both simultaneously
    // at the system level, but a single packet may use a mix if surplus < MaxPower.

    MaxPower = min(Asset.Profile.PowerRange.MaxPower, available capacity in slot)
    SurplusForPacket = min(MaxPower, S.SurplusAvailable - surplusTentativelyClaimed[S])
    ImportForPacket = MaxPower - SurplusForPacket

    If ImportForPacket = 0:
      // Pure self-consumption: cost = export opportunity cost (forgone revenue)
      EffectiveCost = S.ExportPrice + (0 × CO2Weight)    // PV is zero-carbon
    Elif SurplusForPacket = 0:
      // Pure grid import
      EffectiveCost = S.ImportPrice + (S.CO2Rate × CO2Weight)
    Else:
      // Blended: part surplus, part grid
      EffectiveCost = (SurplusForPacket × S.ExportPrice
                       + ImportForPacket × (S.ImportPrice + S.CO2Rate × CO2Weight)) / MaxPower

    ActiveTier = P.ValueCurve.DeadlineTiers[P.ValueCurve.ActiveTierIndex]

    WithinComfortBid = EffectiveCost ≤ ComfortBid      // fill-based cost ceiling
    WithinMarginalRate = EffectiveCost ≤ ActiveTier.MaxMarginalRate  // tier-level ceiling
    WithinCO2 = S.CO2Rate ≤ interpolatedMaxMarginalCO2(P, ProjectedFill)

    CostIfAllocated = (SurplusForPacket × S.ExportPrice
                       + ImportForPacket × S.ImportPrice) × S.Duration
    WithinBudget = (PlannedCostSoFar + CostIfAllocated) ≤ ActiveTier.MaxTotalCost

    Eligible = WithinComfortBid AND WithinMarginalRate AND WithinBudget AND WithinCO2

    // --- Record CalcCache ---

    cache[P, S] = CalcCache {
      EffectiveCost, MarginalValue, ProjectedFill,
      EnergyToAllocate, Eligible, ComfortBid, TimePressure,
      SurplusForPacket, ImportForPacket,
      CostIfAllocated, BudgetRemaining, ...
    }

    // --- Project forward for next slot ---

    If Eligible:
      // Tentatively assume this slot will be used (greedy forward projection)
      tentativeEnergy = MaxPower × S.Duration
      ProjectedFill += tentativeEnergy / P.TargetEnergy
      PlannedCostSoFar += CostIfAllocated
      EnergyToAllocate -= tentativeEnergy
      surplusTentativelyClaimed[S] += SurplusForPacket  // reduce surplus for next packet scoring
```

Note: the forward projection in scoring is tentative. Phase 3 allocation may differ
because capacity is shared across packets. But it gives a reasonable fill trajectory
for ComfortBid interpolation and TimePressure calculation.

**Known approximation (surplus consistency):** Phase 2 tentatively claims surplus via
`surplusTentativelyClaimed` in packet iteration order (arbitrary). Phase 3 allocates in
EffectiveCost × MarginalValue sort order (different). A packet scored with surplus in
Phase 2 might not get it in Phase 3 if a higher-priority packet claims it first. This
means some CalcCache.EffectiveCost entries for surplus slots may be slightly wrong.
Acceptable for residential scale (3–10 packets rarely compete for surplus in the same
slot). Phase 3's `slotRemainingSurplus` tracking is authoritative.

---

## 7. Phase 3: Allocate Consumption

The core scheduling loop. Greedy allocation in priority order. Operates on FIRM slots only.

```
// Collect all eligible (packet, FIRM slot) pairs
candidates = [ (P, S, cache[P,S]) for all P,S where cache[P,S].Eligible = true ]

// Sort by: cheapest effective cost first, then highest MarginalValue within each cost level
sort candidates by:
  1. cache[P,S].EffectiveCost ascending   // per-packet surplus-aware cost from CalcCache
  2. cache[P,S].MarginalValue descending  // within similar cost, highest priority first

// Track remaining capacity and energy needs
slotRemainingCapacity[S] = S.ImportCapacityLimit - S.BaselineLoad  (for each FIRM slot)
slotRemainingSurplus[S] = S.SurplusAvailable_kW                    (for each FIRM slot)
packetEnergyToAllocate[P] = P.UndeliveredEnergy()                  (for each packet)
packetPlannedCost[P] = P.AccumulatedCost                           (for each packet)
assetSlotCommitment[A,S] = 0                                       (for each asset × slot)

For each (P, S, cache) in sorted candidates:

  If packetEnergyToAllocate[P] ≤ 0:
    Continue (packet fully scheduled)

  Asset = Assets[P.AssetID]

  // --- Determine allocatable power ---
  
  maxAssetPower = Asset.Profile.PowerRange.MaxPower
  If Asset.Profile.Adjustability = STEPPED:
    maxAssetPower = largest step ≤ maxAssetPower
  
  // Respect asset sharing: if another packet already uses this asset in this slot
  availableAssetPower = maxAssetPower - assetSlotCommitment[Asset, S]
  If availableAssetPower ≤ 0:
    Continue (asset fully committed in this slot)

  // Respect site import capacity (surplus portion doesn't count against import limit)
  surplusUsed = min(availableAssetPower, slotRemainingSurplus[S])
  gridNeeded = availableAssetPower - surplusUsed
  allocatablePower = surplusUsed + min(gridNeeded, slotRemainingCapacity[S])
  If allocatablePower ≤ 0:
    Continue (no capacity available)

  // Recompute surplus/grid split for actual allocatable power
  surplusUsed = min(allocatablePower, slotRemainingSurplus[S])
  gridUsed = allocatablePower - surplusUsed

  // Respect packet's remaining energy need
  energyInSlot = allocatablePower × S.Duration_hours
  If energyInSlot > packetEnergyToAllocate[P]:
    // Don't over-allocate. Reduce power to match remaining need.
    allocatablePower = packetEnergyToAllocate[P] / S.Duration_hours
    surplusUsed = min(allocatablePower, slotRemainingSurplus[S])
    gridUsed = allocatablePower - surplusUsed

  // --- Final budget check with actual allocation ---

  actualCost = (surplusUsed × S.ExportPrice + gridUsed × S.ImportPrice) × S.Duration_hours
  ActiveTier = P.ValueCurve.DeadlineTiers[P.ActiveTierIndex]
  If (packetPlannedCost[P] + actualCost) > ActiveTier.MaxTotalCost:
    // Would bust budget. Reduce power to fit within budget.
    // First consume surplus (cheapest), then grid import with remaining budget.
    remainingBudget = ActiveTier.MaxTotalCost - packetPlannedCost[P]
    surplusCost = surplusUsed × S.ExportPrice × S.Duration_hours
    If surplusCost > remainingBudget:
      surplusUsed = remainingBudget / (S.ExportPrice × S.Duration_hours)
      gridUsed = 0
    Else:
      gridBudget = remainingBudget - surplusCost
      gridUsed = min(gridUsed, gridBudget / (S.ImportPrice × S.Duration_hours))
    allocatablePower = surplusUsed + gridUsed
    If allocatablePower ≤ 0:
      Continue
    actualCost = (surplusUsed × S.ExportPrice + gridUsed × S.ImportPrice) × S.Duration_hours

  // --- Commit allocation ---

  actualEnergy = allocatablePower × S.Duration_hours
  actualCO2 = gridUsed × S.CO2Rate × S.Duration_hours  // only grid import has CO2

  Record PacketAllocation {
    PacketID: P.PacketID,
    AssetID: P.AssetID,
    AllocatedPower_kW: allocatablePower,
    SurplusPower_kW: surplusUsed,            // how much came from PV surplus
    GridPower_kW: gridUsed,                  // how much came from grid import
    MarginalValue: cache.MarginalValue,
    CostInSlot_EUR: actualCost,
    CO2InSlot_g: actualCO2
  }

  // Update tracking
  slotRemainingCapacity[S] -= gridUsed       // only grid import counts against capacity
  slotRemainingSurplus[S] -= surplusUsed     // surplus pool depleted
  packetEnergyToAllocate[P] -= actualEnergy
  packetPlannedCost[P] += actualCost
  assetSlotCommitment[Asset, S] += allocatablePower
```

### 7.1 Sort Order Rationale

The sort — cheapest slot first, then highest MarginalValue — produces the correct result
because:

- Filling cheap slots first minimizes total cost.
- Within a cheap slot that has limited capacity, the highest-priority packet wins.
- An urgent EV (high MarginalValue) that can't fit in slot #1 (full) will get slot #2
  instead. A low-priority top-up that could have used slot #2 gets pushed to slot #3.
- The net effect is: high-priority packets get good slots, low-priority packets get
  whatever's left — which is exactly what the user's bids express.

### 7.2 STEPPED Asset Handling

For assets with PowerAdjustability = STEPPED (e.g. heater with 0/3/6 kW):

```
The algorithm uses the largest feasible step:
  availableSteps = Asset.Profile.PowerRange.PowerSteps
  feasibleSteps = [s for s in availableSteps if s ≤ availableCapacity AND s ≤ needed]
  allocatablePower = max(feasibleSteps) if feasibleSteps not empty, else 0

This means STEPPED assets sometimes under-utilize a slot (want 4 kW but can
only do 3 kW) or skip a slot (need 1 kW but minimum step is 3 kW). The
remaining capacity can be used by other packets in the same slot.
```

---

## 8. Phase 4: Allocate Storage

Storage assets (battery, V2G-capable EV) need special treatment because they can both
consume and produce. The algorithm identifies profitable charge/discharge cycles.

Note: Phase 4 runs AFTER Phase 3. It uses `slotRemainingSurplus[S]` (the post-Phase-3
surplus pool), not the original `S.SurplusAvailable_kW`. This ensures consumption packets
get first claim on surplus, and batteries only charge from unclaimed remainder.

Note: battery charge and discharge packets are SEPARATE packets with sequential deadlines.
Charge packet: CompletionPolicy = STOP at discharge start time. If charge doesn't complete,
the discharge packet uses actual SoC at start (not planned SoC). The discharge packet's
TargetEnergy is NOT adjusted — it delivers what it can from available SoC.

### 8.1 Opportunity Identification

```
For each storage asset A:
  If A has no active charge packet AND no active discharge packet:
    Continue (storage not in use)

  If A has a charge packet but the Planner determines better use:
    Storage can shift energy across time — this is the core value of batteries.

  SortedSlots = all FIRM slots sorted by GridEffectiveCost ascending
  CheapSlots = slots in lower quartile of GridEffectiveCost (good for charging)
  ExpensiveSlots = slots in upper quartile (good for discharging)

  For each CheapSlot:
    // Charge cost depends on energy source (post-Phase-3 surplus pool):
    // If PV surplus remains after consumption packets: cost = ExportPrice
    // If no surplus: cost = ImportPrice + CO2
    If slotRemainingSurplus[CheapSlot] > 0:
      ChargeCost = CheapSlot.ExportPrice  // forgone export revenue
    Else:
      ChargeCost = CheapSlot.ImportPrice + (CheapSlot.CO2Rate × CO2Weight)
    ChargeOpportunityCost = ChargeCost / A.Profile.Efficiency
    // Divide by efficiency because 1 kWh stored requires 1/η kWh charged

  For each ExpensiveSlot:
    DischargeValue = ExpensiveSlot.GridEffectiveCost
    // Discharge displaces grid import at the slot-level cost (no per-packet context)

  // A charge/discharge cycle is profitable if:
  //   DischargeValue > ChargeOpportunityCost
  //   i.e. the expensive slot is worth more than the cheap slot adjusted for losses

  ProfitablePairs = [
    (cheap, expensive) where
    expensive.GridEffectiveCost > cheap.GridEffectiveCost / A.Profile.Efficiency
  ]
```

### 8.2 Storage Allocation

```
For each ProfitablePair (chargeSlot, dischargeSlot):
  // Determine power constrained by asset limits, SoC bounds, and capacity
  chargePower = min(A.MaxChargePower, slotRemainingCapacity[chargeSlot])
  dischargePower = min(A.MaxDischargePower, slotRemainingCapacity[dischargeSlot])

  // Respect SoC limits
  projectedSoC_after_charge = currentSoC + (chargePower × chargeSlot.Duration) / A.MaxCapacity
  If projectedSoC_after_charge > 1.0:
    Reduce chargePower

  projectedSoC_after_discharge = projectedSoC - (dischargePower × dischargeSlot.Duration) / A.MaxCapacity
  If projectedSoC_after_discharge < A.MinSoC:   // MinSoC may be > 0 (e.g. 10% reserve)
    Reduce dischargePower

  // Record charge allocation (positive power = consuming)
  Record PacketAllocation in chargeSlot for battery charge packet
  
  // Update surplus pool if charging from surplus
  If slotRemainingSurplus[chargeSlot] > 0:
    surplusUsedForCharge = min(chargePower, slotRemainingSurplus[chargeSlot])
    slotRemainingSurplus[chargeSlot] -= surplusUsedForCharge
  
  // Record discharge allocation:
  // If there's a consumption packet that can use the energy → direct self-consumption
  // If not → export to grid at ExportPrice
```

### 8.3 Interaction with Consumption Packets

Storage interacts with Phase 3 results:

```
After Phase 3, some consumption packets may be allocated to expensive slots
because cheaper slots were full. Storage can help:

1. Battery charges in a cheap slot (low EffectiveCost)
2. Battery discharges in the expensive slot, displacing grid import
3. The consumption packet effectively gets energy at the cheaper rate
   (minus efficiency losses)

This is detected as:
  If a consumption packet is allocated to an expensive slot AND
  the battery has uncharged capacity in a cheaper slot AND
  the round-trip is profitable:
    → Reallocate: charge battery in cheap slot, discharge in expensive slot,
      feed consumption packet from battery instead of grid.
    → Net saving = (ExpensiveCost - CheapCost/Efficiency) × Energy
```

---

## 9. Phase 5: Allocate Residual PV Surplus

After Phases 3 and 4, some slots may still have unclaimed PV surplus (surplus not
consumed by any packet or stored in battery). This phase handles the residual.

Note: self-consumption decisions are no longer made here — they are handled by
Phase 3's surplus-aware EffectiveCost. A packet consuming PV surplus "pays" the
ExportPrice (opportunity cost), and the greedy sort automatically prefers surplus
slots over grid-import slots when ExportPrice < ImportPrice.

```
For each FIRM slot S where slotRemainingSurplus[S] > 0:
  // After Phase 3 and Phase 4, this is surplus that no packet or battery claimed.
  // The only option is to export it.

  ExportPower = slotRemainingSurplus[S]

  // Check export capacity limit
  If ExportPower > S.ExportCapacityLimit:
    ExportPower = S.ExportCapacityLimit
    CurtailedPower = slotRemainingSurplus[S] - ExportPower
    // PV must be curtailed (if CROPPABLE) or wasted
    PlanWarning INFO: "PV surplus exceeds export limit in slot HH:MM. Curtailing X kW."

  S.PlannedExport_kW += ExportPower
  ExportRevenue = ExportPower × S.ExportPrice × S.Duration_hours

  // Track for reporting
  Record export allocation for PV asset in this slot.
```

Why self-consumption is handled correctly by Phase 3:
```
Example: slot at 12:00, PV surplus 3.5kW, ExportPrice €0.08, ImportPrice €0.22.

  Phase 2 computes EffectiveCost for washer in this slot:
    SurplusForPacket = min(2kW, 3.5kW) = 2kW (washer max power)
    ImportForPacket = 0kW
    EffectiveCost = €0.08 (export opportunity cost)

  Phase 2 computes EffectiveCost for same washer at 13:00 (no surplus):
    EffectiveCost = €0.22 + CO2 = €0.256

  Greedy sort: 12:00 (€0.08) < 13:00 (€0.256). Washer gets surplus slot.
  Self-consumption emerges from the sort — no special Phase 5 logic needed.

  At high fill (ComfortBid = €0.05):
    12:00: EffectiveCost €0.08 > ComfortBid €0.05 → INELIGIBLE
    → Even PV surplus is too expensive. Export the surplus instead.
    → Correct: exporting earns €0.08, user only values next kWh at €0.05.
```

---

## 10. Phase 6: Penalty Threshold Check

After the initial allocation, check whether the plan would trigger any penalty thresholds.

Note: the Planner checks planned slot power against the threshold conservatively.
The actual breach determination uses the MeasurementWindow rolling average (computed
by the Monitor at runtime). The Planner flags any slot where planned power exceeds
the threshold as risky, even if the rolling average might absorb a brief spike.
This is intentionally conservative — better to reschedule than to rely on averaging.

```
For each PenaltyRule R where R.Active = true:

  If R.Condition = PEAK_DEMAND_EXCEEDED:
    PlannedPeakDemand = max(S.NetPlannedPower_kW for all S in current billing period)
    If PlannedPeakDemand > R.Threshold.Threshold_kW:
      // Plan would breach the threshold.
      
      // Find the breaching slots (where power exceeds threshold)
      BreachingSlots = [S where S.NetPlannedPower_kW > R.Threshold.Threshold_kW]
      
      // Compute avoidance cost:
      // Try to reschedule the lowest-value allocations in breaching slots
      // to non-breaching slots (later, or lower-power)
      
      For each BreachingSlot, sorted by lowest-MarginalValue allocation first:
        excessPower = S.NetPlannedPower_kW - R.Threshold.Threshold_kW
        victim = allocation with lowest MarginalValue in this slot
        
        // Can we move this allocation to a different slot?
        alternativeSlot = find slot where:
          - packet is eligible
          - slot has remaining capacity
          - slot does not push past threshold
          - slot EffectiveCost ≤ reasonable ceiling
        
        If alternativeSlot found:
          Move allocation. Breach avoided for this slot.
          moveCost = (alternativeSlot.EffectiveCost - BreachingSlot.EffectiveCost) × energy
        Else:
          avoidanceCostForThisSlot = ∞ (can't avoid)
      
      totalAvoidanceCost = Σ(moveCost for all moved allocations)
      
      If R.BreachedThisPeriod = false:
        // Haven't breached yet. Full penalty cost applies as barrier.
        If totalAvoidanceCost < R.Cost:
          Accept the moves. Breach avoided.
        Else:
          Revert moves. Accept the breach.
          R.BreachedThisPeriod = true (will be confirmed by Monitor at runtime)
          Add PlanWarning: "Plan will breach peak demand threshold. Penalty: €100."

      If R.BreachedThisPeriod = true:
        // Already breached this period. Penalty is sunk (€100 already incurred).
        // But STILL try to reschedule — threshold remains a soft constraint.
        // The difference: no €100 barrier to compare against. Instead, accept
        // moves if they're cheap (moveCost < reasonable threshold, e.g. €5),
        // reject if expensive (don't sacrifice €20 of comfort for a soft preference).
        softThreshold = min(R.Cost × 0.05, €5.00)  // small budget for post-breach compliance
        If totalAvoidanceCost < softThreshold:
          Accept the moves. Keep peak as low as possible.
        Else:
          Revert moves. Accept the higher peak.
          // Acceptable: penalty already sunk, and rescheduling is too expensive.
          // But we tried — this is different from "relax everything."

  If R.Condition = ENERGY_BUDGET_EXCEEDED:
    PlannedTotalUsage = Σ(S.PlannedImport_kW × S.Duration for all S in period)
                       + actual usage already recorded in PastEnergySum
    If PlannedTotalUsage > R.Threshold.Threshold_kWh:
      // Same two-path logic: if not yet breached, full penalty as barrier.
      // If already breached, still try to stay under as soft constraint.

  If R.Condition = EXPORT_LIMIT_EXCEEDED:
    PlannedPeakExport = max(S.PlannedExport_kW for all S in current billing period)
    If PlannedPeakExport > R.Threshold.Threshold_kW:
      // Same avoidance logic as PEAK_DEMAND_EXCEEDED but for export:
      // Find slots where export exceeds threshold.
      // Try to increase self-consumption or reduce PV output (CROPPABLE) in those slots.
      // Compare avoidance cost with penalty cost.
      // Note: Phase 5 already curtails to ExportCapacityLimit (hard limit),
      // but EXPORT_LIMIT_EXCEEDED is a PENALTY threshold (may be lower than hard limit).
```

### 10.1 Penalty Decision as Worked Example

```
Scenario:
  PenaltyRule: Peak demand > 15kW → €100/month
  Current billing period peak so far: 12kW (no breach yet)
  Planner wants to schedule EV (7kW) + Heat pump (5kW) + Baseline (4kW) = 16kW in slot 18:00

  Breach: 16kW > 15kW → would trigger €100 penalty

  Option A: Accept breach
    Cost = €100 (one-time, for the month)

  Option B: Defer EV to slot 19:00 (heat pump done by then)
    19:00 EffectiveCost = €0.35/kWh (vs €0.25/kWh at 18:00)
    Extra cost = 7kW × 1h × (€0.35 - €0.25) = €0.70

  Decision: defer EV. €0.70 << €100.

  But if EV has critical time pressure (MarginalValue = 5.0) and 19:00 is its
  last possible slot:
    Option B is unavailable.
    Option C: Reduce heat pump to 3kW (stepped: 0/3/6) → total 14kW, no breach.
    But heat pump loses 2kW × 1h = 2kWh of heating.
    If heat pump's ComfortBid at current fill = €0.20/kWh:
      lost comfort value = 2kWh × €0.20 = €0.40
    Decision: reduce heat pump. €0.40 << €100.
```

---

## 11. Phase 7: Build Flexibility Envelopes

After FIRM-slot allocation is complete, characterize the remaining flexible demand.

```
For each packet P with packetEnergyToAllocate[P] > 0:

  // Energy already committed in FIRM slots
  firmEnergy = P.UndeliveredEnergy() - packetEnergyToAllocate[P]

  // Find the flexible window for this packet
  eligibleFlexSlots = [S for S in FlexibleSlots
                       where S.TimeStamp ≥ P.EarliestStart
                       AND (P.CompletionPolicy = CONTINUE OR S.TimeStamp < P.LatestEnd)
                       AND asset is forecast-available in S]

  If eligibleFlexSlots is empty:
    // All energy must come from FIRM slots. If still unserved → warning in Phase 8.
    Continue to next packet.

  // Compute rate range for eligible slots (using GridEffectiveCost for FLEXIBLE slots)
  eligibleRates = [S.GridEffectiveCost for S in eligibleFlexSlots
                   where S.GridEffectiveCost ≤ min(ComfortBidAtCurrentFill, MaxMarginalRate)]

  If eligibleRates is empty:
    // No affordable FLEXIBLE slots exist. Warning in Phase 8.
    Continue to next packet.

  // Build envelope
  Envelope = FlexibilityEnvelope {
    PacketID        = P.PacketID
    AssetID         = P.AssetID
    EnergyNeeded    = packetEnergyToAllocate[P]
    MaxPower        = Asset.Profile.PowerRange.MaxPower
    MinPower        = Asset.Profile.PowerRange.MinPower (or smallest STEPPED level)
    WindowStart     = min(eligibleFlexSlots.TimeStamp)
    WindowEnd       = max(eligibleFlexSlots.TimeStamp) + StepSize
    SlotsAvailable  = len(eligibleFlexSlots)
    MaxAcceptableRate = min(ComfortBid at current fill, ActiveTier.MaxMarginalRate)
    MinAcceptableRate = ComfortBid at projected fill after full delivery
    BudgetRemaining = ActiveTier.MaxTotalCost - P.AccumulatedCost - firmCost
    EstimatedCost   = EnergyNeeded × mean(eligibleRates)
    EstimatedCO2    = EnergyNeeded × mean(eligible CO2 rates)
  }

  Append Envelope to Plan.Envelopes[]

Note: packets that were fully allocated in FIRM slots (packetEnergyToAllocate = 0)
do NOT get envelopes. Their demand is committed, not flexible.

Note: packets where early firm-up was applied (Phase 2 variance check) also have
packetEnergyToAllocate = 0 and no envelope. Their FLEXIBLE slots were treated as
FIRM for that packet because holding flexibility had no value (flat rates).
```

---

## 11.1. Phase 8: Finalize

```
1. Write PacketAllocations into each FIRM PlanTimeSlot.

2. Compute FIRM slot summaries:
   For each FIRM slot:
     NetPlannedPower = Σ(AllocatedPower for all allocations) + BaselineLoad
     PlannedImport = max(0, NetPlannedPower)
     PlannedExport = max(0, -NetPlannedPower)
     ImportFlexibility = ImportCapacityLimit - PlannedImport
     ExportFlexibility = ExportCapacityLimit - PlannedExport

3. Write PlannedPowerProfile for each EnergyPacket (FIRM allocations only):
   For each packet:
     PlannedPowerProfile = [
       EnergySnapshot(slot.TimeStamp, allocation.AllocatedPower, cumulative)
       for each FIRM slot where packet has an allocation
     ]

4. Write PlannedEnergySum (FIRM slots only — authoritative for Dispatcher):
   PlannedEnergySum = [
     PowerSnapshot(slot.TimeStamp, slot.NetPlannedPower)
     for each FIRM slot
   ]

5. Write FlexibilityEnvelopes (from Phase 7) into Plan.

6. Detect and emit PlanWarnings:
   For each packet:
     If packetEnergyToAllocate[P] > 0 AND no eligible FIRM slots remain in active tier
        AND no FlexibilityEnvelope exists for P:
       → "Packet cannot complete in Tier N" (triggers tier fallback in Post-Plan step)
     If packetEnergyToAllocate[P] > 0 AND FlexibilityEnvelope exists:
       → INFO: "X kWh of Y kWh awaiting VTN price signals in window HH:MM-HH:MM"
     If CompletionPolicy = STOP AND EstimatedCompletion < 1.0:
       → "Battery charge will reach X% by deadline. Asset freed at LatestEnd."
   For FIRM slots near capacity limits:
     If ImportFlexibility < AutoFollowHeadroom × 0.5:
       → "Tight capacity in slot HH:MM — limited room for deviation absorption"
   For penalty avoidance:
     If penalty was accepted (not avoided):
       → severity = WARNING, message includes penalty cost
```

---

## 12. Near-Horizon vs Far-Horizon: The Two-Layer Plan

The core architectural decision: **near-horizon slots get firm allocations, far-horizon
slots preserve flexibility for VTN coordination.**

### 12.1 How the Boundary Works

```
Time flows →

|←── FIRM ──→|←────────── FLEXIBLE ────────────→|
|  allocated  |  envelopes only, no allocations  |
now    FirmBoundary                          EndTime

Every 5 minutes, the Planner re-runs:
  - FirmBoundary slides forward
  - FLEXIBLE slots that were just beyond the boundary become FIRM
  - The greedy algorithm firms them using the best currently known rates
  - New FLEXIBLE slots are added at the far end if the horizon extends
```

### 12.2 What Happens Without VTN Signals

The system never waits. Time passes, and each plan cycle slides the FIRM boundary forward.

```
18:00  EV needs 26 kWh by 07:00. NearHorizonDuration = 2h.
       FIRM: 18:00-20:00 → allocations made (but off-peak hasn't started)
       FLEXIBLE: 20:00-07:00 → FlexibilityEnvelope reported to VTN:
         "26 kWh, 7kW max, window 20:00-07:00, max rate €0.30/kWh"

20:00  FIRM boundary now covers 20:00-22:00. Off-peak starts.
       Greedy allocator sees €0.12 slots at 20:00.
       But it also knows 02:00 is still FLEXIBLE (no rate data? same rate?).
       Early firm-up check: if all FLEXIBLE slots have same €0.12 rate → firm up.
       → EV starts charging at 20:00.

       If rate variation existed (e.g. VTN published €0.05 at 02:00):
       → EV waits. 02:00 slots stay FLEXIBLE until they enter near-horizon.
       → At 00:00, FIRM covers 00:00-02:00. Still waiting for 02:00.
       → At 02:00, cheap slots enter FIRM. Greedy allocates. EV charges at €0.05.
```

### 12.3 What Happens With VTN Signals

```
18:00  EV FlexibilityEnvelope reported to VTN.
       VTN sees: "this VEN has 26 kWh shiftable demand, 20:00-07:00."

18:30  VTN sends PRICE event: "02:00-04:00 €0.05/kWh" (wind surplus incentive)
       → OpenADR IF updates PlannedRates → PlanTrigger.RATE_CHANGE → replan
       
       Planner re-runs. Now the rate landscape shows:
         20:00-02:00: €0.12/kWh
         02:00-04:00: €0.05/kWh (VTN signal)
         04:00-07:00: €0.12/kWh
       
       FLEXIBLE slots 02:00-04:00 now have significantly lower rates.
       Rate variance is high → flexibility preserved (don't firm up yet).
       Envelope updated: "26 kWh, prefer 02:00-04:00 window."
       
02:00  FIRM boundary reaches 02:00. Greedy allocates at €0.05.
       EV charges 02:00-05:45. Total cost: €1.32 (vs €3.16 at blind off-peak).
```

### 12.4 Per-Packet Urgency Override

Some packets can't afford to stay FLEXIBLE:

```
Example: Battery charge STOP at 17:00. Current time: 14:00. NearHorizon = 2h.
  FirmBoundary = 16:00.
  Battery needs 3h of charging at 5kW. Only 3h available (14:00-17:00).
  TimePressure ≥ 2.0 for all slots → ALL slots forced FIRM despite 16:00-17:00
  being beyond the global near-horizon.

Example: EV charge by 07:00. Current time: 18:00. NearHorizon = 2h.
  13 hours available, 3.75h needed. TimeSlack = 9.25h. Comfortable.
  Slots 20:00-07:00 stay FLEXIBLE. Flexibility reported to VTN.
```

The boundary is not purely clock-based. It's max(global clock, per-packet urgency).

---

## 13. Batch Process Scheduling Risk

CONTINUE packets with high PostDeadlineComfortBid (washing machine, dishwasher)
present a scheduling risk: once started, they're expensive to interrupt.

```
For each batch-like packet (CONTINUE with high bid AND estimated duration > 30 min):

  proposedStart = earliest cheap slot where the asset is available
  estimatedEnd = proposedStart + Asset.Profile.typicalCycleDuration
    (or estimatedEnd = proposedStart + UndeliveredEnergy / typicalPower)

  // Check what happens in the window [proposedStart, estimatedEnd]
  windowSlots = slots from proposedStart to estimatedEnd

  riskCost = 0
  For each slot in windowSlots:
    If slot.EffectiveCost > threshold (e.g. > 2× average EffectiveCost):
      // This is an expensive slot. If the batch process overruns into it,
      // the high bid will force it to keep running at this expensive rate.
      riskCost += typicalPower × (slot.EffectiveCost - averageCost) × slot.Duration

    If slot approaches a PenaltyRule threshold:
      riskCost += fraction_of_penalty_risk

  // Compare: starting at proposedStart vs starting later
  If riskCost > significantThreshold:
    Defer start to a window with lower risk profile.
    Add PlanWarning (INFO): "Washing machine start delayed to avoid expensive window"

This is a heuristic, not an optimization. The Planner can't predict exactly when
a washing machine cycle will end. But it can avoid obviously bad start times.
```

---

## 14. Algorithm Complexity

For residential scale, the algorithm is fast:

```
Typical dimensions:
  Packets:    3–10 (EV, battery, heater, washer)
  FIRM slots: ~24–48 (2h near-horizon at 5-min steps)
  All slots:  288 per day (5-min steps) x 2 days = ~576
  Assets:     3–15

Phase 2 (Score):     O(packets x FIRM slots) = ~480 CalcCache entries (much less than before)
Phase 3 (Allocate):  O(packets x FIRM slots x log) for sort = negligible
Phase 4 (Storage):   O(FIRM slots x storage_assets) = ~48
Phase 5 (PV residual): O(FIRM slots) = ~24-48 (just residual export, no scoring)
Phase 6 (Penalty):   O(penalty_rules x breaching_slots x packets) = small
Phase 7 (Envelopes): O(packets x FLEXIBLE slots) = ~5,280 (aggregation only, no scoring)
Phase 8 (Finalize):  O(FIRM slots + packets) = linear

Total: well under 50ms on any modern hardware.
Faster than before because greedy allocation only processes FIRM slots (~5-10% of grid).
Replanning every 5 seconds (during alerts) is easily feasible.

Note: early firm-up (§5.1) may increase FIRM slot count for some packets,
but this is bounded by total slot count and only triggers when rates are flat.
```

---

## 15. Worked Example: Full Plan Cycle

A complete example tying all phases together.

```
Setup:
  Assets: PV (8kW peak), Battery (10kWh, 5kW charge/discharge, 92% efficiency),
          EV (50kWh, 7kW charger), Heat pump (3/6kW stepped)
  Time: 14:00, planning to 14:00 next day
  CO2Weight: 0.0001 €/g (= €100/tonne)
  PenaltyRule: Peak demand > 15kW → €100/month (not yet breached)

Rates (simplified):
  14:00-17:00: Import €0.20, Export €0.08, CO2 350g/kWh
  17:00-20:00: Import €0.40, Export €0.10, CO2 420g/kWh  (peak)
  20:00-06:00: Import €0.12, Export €0.05, CO2 280g/kWh  (off-peak)
  06:00-14:00: Import €0.22, Export €0.08, CO2 360g/kWh

Forecasts:
  PV: 8kW now, dropping to 0 by 18:00 (winter afternoon)
  Heat pump: 4kW continuous (cold day)
  Baseline: 0.5kW standby

Active Packets:
  EV: "charge to 80% by 07:00, budget €3.00"
    Current SoC: 30%, TargetEnergy: 26.3 kWh
    ComfortRates: [0%→€0.40, 50%→€0.25, 80%→€0.10, 100%→€0.02]
    CompletionPolicy: CONTINUE, PostDeadlineComfortBid: €0.02 (free energy only)
    
  Heat pump: "maintain 21°C until 22:00"
    UndeliveredEnergy: ~24 kWh over 8 hours
    ComfortRates: [0%→€0.35, 80%→€0.20, 100%→€0.05]
    CompletionPolicy: STOP at 22:00

  Battery: "charge for evening discharge"
    Current SoC: 20%, Target: 90%
    CompletionPolicy: STOP at 17:00 (discharge packet starts 17:00)

  Battery discharge: "discharge during peak 17:00-20:00"
    CompletionPolicy: STOP at 20:00
```

**Phase 1: Prepare**
```
Grid built: 288 slots (14:00 today → 14:00 tomorrow)
NearHorizonDuration = 2h → GlobalFirmBoundary = 16:00

Slot classification:
  14:00-16:00: FIRM (within near-horizon)
  16:00-17:00: Battery charge has TimePressure ≥ 2.0 (deadline 17:00, needs 3h)
               → forced FIRM for battery packet (urgency override)
  16:00-07:00: FLEXIBLE for EV (TimeSlack = 9.25h, comfortable)
  17:00-20:00: FLEXIBLE for most packets
               Battery discharge 17:00-20:00 has TimePressure ≥ 2.0 → forced FIRM
  20:00-07:00: FLEXIBLE (far horizon)
  07:00-14:00: FLEXIBLE (next day)

Baseline from forecasts:
  PV:             -8kW at 14:00, declining to 0 by 18:00
  Heat pump:      4kW continuous (from physical model)
  SITE_RESIDUAL:  0.5 kW (learned heuristic — fridge, lights, router, chargers, etc.)

  14:00: BaselineLoad = -8 + 4 + 0.5 = -3.5 kW → SurplusAvailable = 3.5kW
  15:00: BaselineLoad = -5 + 4 + 0.5 = -0.5 kW → SurplusAvailable = 0.5kW
  16:00: BaselineLoad = -2 + 4 + 0.5 = +2.5 kW → SurplusAvailable = 0
  17:00+: BaselineLoad = 0 + 4 + 0.5 = +4.5 kW → SurplusAvailable = 0
  22:00+: BaselineLoad = 0 + 0 + 0.5 = +0.5 kW → SurplusAvailable = 0

Early firm-up check for EV:
  EV's eligible FLEXIBLE slots (20:00-07:00) all have GridEffectiveCost = €0.148
  Rate variance = 0 → no value in holding flexibility → early firm-up triggered.
  EV's 20:00-07:00 slots treated as FIRM for EV. (In practice: flat off-peak = commit now.)

  If VTN had published a special €0.05 window at 02:00-04:00:
  Rate variance > 10% → flexibility preserved. EV stays FLEXIBLE 20:00-07:00.
```

**Phase 2: Score (FIRM slots only, including early-firmed slots)**
```
EV at 14:00 (FIRM, 3.5kW surplus available):
  SurplusForPacket = min(7kW, 3.5kW) = 3.5kW
  ImportForPacket = 7 - 3.5 = 3.5kW
  EffectiveCost = (3.5×0.08 + 3.5×0.235) / 7 = €0.158/kWh (blended)
  Fill = 30% → ComfortBid = 0.38 (interpolated)
  WithinComfortBid: 0.158 ≤ 0.38 → yes
  WithinMarginalRate: 0.158 ≤ 0.30 → yes
  → Eligible. Cheaper than pure import (€0.235) because half is PV surplus.

  But: battery also wants this surplus (see below). Surplus is shared pool.
  If battery claims surplus first (higher MarginalValue for storage cycle):
    EV at 14:00 falls back to pure import: EffectiveCost = €0.235.
    Still eligible but more expensive than off-peak.

EV at 20:00 (early-firmed, was FLEXIBLE, no surplus):
  SurplusForPacket = 0
  EffectiveCost = 0.12 + 280×0.0001 = 0.148 €/kWh (pure grid import)
  Fill = 30% → ComfortBid = 0.38 → 0.148 < 0.38 → eligible
  MarginalValue = 0.38 × 1.0 = 0.38
  Cheapest eligible slot for EV → preferred over 14:00 blended.

EV at 20:00 at 80% fill:
  EffectiveCost = 0.148 €/kWh
  Fill = 80% → ComfortBid = 0.10 (interpolated)
  WithinComfortBid: 0.148 > 0.10 → INELIGIBLE
  → At 80% fill, even off-peak is too expensive.

  What about PV surplus tomorrow? ExportPrice = €0.08.
  ComfortBid = €0.10 → 0.08 < 0.10 → ELIGIBLE for surplus slots.
  → This is why the EV naturally charges the last few % from PV surplus
    next day, where the "cost" is only the forgone export revenue (€0.08).

Battery charge at 14:00 (3.5kW surplus available):
  SurplusForPacket = min(5kW, 3.5kW) = 3.5kW (battery max charge 5kW)
  ImportForPacket = 5 - 3.5 = 1.5kW
  EffectiveCost = (3.5×0.08 + 1.5×0.235) / 5 = €0.127/kWh (blended)
  Deadline = 17:00 → SlotsUntilDeadline = 36, SlotsNeeded ≈ 17
  TimeSlack = 19 → TimePressure = 1.0
  → Eligible. Cheap because mostly PV surplus.
```

**Phase 3: Allocate Consumption**
```
Sorted by EffectiveCost ascending (surplus-aware):
  PV surplus slots (14:00-15:00) at ~€0.08 are cheapest (ExportPrice opportunity cost)
  Off-peak slots (20:00-06:00) at ~€0.148 are next
  Blended surplus+import slots (14:00-16:00) vary
  Peak slots (17:00-20:00) at ~€0.442 are most expensive

Battery charge claims surplus first (Phase 4, but interacts):
  14:00: battery takes 3.5kW surplus → slotRemainingSurplus drops to 0
  15:00: battery takes 0.5kW surplus

EV at 14:00 after battery claims surplus: pure import at €0.235.
  Off-peak at €0.148 is cheaper → EV defers to off-peak.

EV gets off-peak slots: 20:00-06:00 at 7kW, EffectiveCost = €0.148
  26.3 kWh needed at €0.12 = €3.16... exceeds €3 budget.
  Budget constrains: can afford 3.00 / 0.12 = 25 kWh at this rate.
  Remaining 1.3 kWh → post-deadline CONTINUE at €0.02 bid.
    PV surplus tomorrow: EffectiveCost = €0.08. ComfortBid €0.02 < €0.08 → ineligible.
    But PostDeadlineComfortBid applies to ALL modes. €0.02 is the max bid.
    → EV needs "free" energy with EffectiveCost < €0.02. Only possible if ExportPrice < €0.02.
    → Or user raises PostDeadlineComfortBid to €0.10 (accepts surplus at opportunity cost).

Heat pump: baseline at 4kW, controllable 3-6kW.
  14:00-17:00: heat pump is in baseline. PV covers it. No packet cost.
  17:00-20:00: peak, EffectiveCost = €0.442, no surplus.
    Heat pump ComfortBid at mid-fill = €0.20.
    0.442 > MaxMarginalRate? If MaxMarginalRate = €0.50 → eligible but expensive.
    Battery discharge displaces this import (Phase 4).
```

**Phase 4: Storage**
```
Battery charge: 14:00-17:00 from PV surplus
  14:00: SurplusAvailable 3.5kW → battery charges at 3.5kW
    ChargeCost = ExportPrice €0.08/kWh (opportunity cost of surplus)
    ChargeOpportunityCost = €0.08 / 0.92 = €0.087/kWh (adjusted for efficiency)
  15:00: SurplusAvailable 0.5kW → charge at 0.5kW, same calculation
  16:00: no surplus → could charge from grid at €0.235, but more expensive
    Battery only needs ~6 kWh. Gets ~4 kWh from surplus slots. Remainder from grid.
  Total charge: ~6 kWh. Cost: ~4kWh × €0.08 + 2kWh × €0.20 = €0.72
  SoC moves from 20% to ~80%

Battery discharge: 17:00-20:00
  DischargeValue = €0.442/kWh (peak GridEffectiveCost)
  Round-trip check: €0.442 > €0.087 (charge opportunity cost) → profitable.
  Profit: (€0.442 - €0.087) × 5.5kWh delivered = €1.95 savings.
  Heat pump fed from battery instead of grid at peak.
  Remaining peak hours: heat pump imports from grid at €0.40
```

**Phase 5: Residual PV Surplus**
```
After Phase 3 (no EV consumption from surplus) and Phase 4 (battery charged):
  14:00: surplus 3.5kW - battery 3.5kW = 0kW remaining. Nothing to export.
  15:00: surplus 0.5kW - battery 0.5kW = 0kW remaining. Nothing to export.
  
  If battery were already full at 14:00:
    3.5kW × €0.08 = €0.28/h exported. Phase 5 records export allocation.
```

**Phase 6: Penalty Check**
```
Peak planned demand: at 17:00-17:15 (battery discharged, heat pump 6kW + standby 0.5kW)
  Wait — battery is discharging, so it REDUCES import.
  17:00: heat pump 4kW + standby 0.5kW - battery discharge 5kW = -0.5kW (net export!)
  18:15 (battery depleted): heat pump 4kW + standby 0.5kW = 4.5kW import
  
Peak is 4.5kW << 15kW threshold. No penalty risk. ✓

But if EV were also charging at peak: 4.5 + 7 = 11.5kW, still under 15kW. ✓
```

**Phase 7: Build Flexibility Envelopes**
```
EV: packetEnergyToAllocate = 0 (fully allocated in FIRM + early-firmed slots). No envelope.
Battery charge: packetEnergyToAllocate = 0 (all slots FIRM due to urgency). No envelope.
Battery discharge: packetEnergyToAllocate = 0 (all slots FIRM due to urgency). No envelope.
Heat pump: packetEnergyToAllocate = 0 (baseline load, not packet-allocated). No envelope.

In this example, all packets are fully committed. No FlexibilityEnvelopes generated.
This is because: battery had urgency, EV had flat off-peak rates (early firm-up).

Alternative scenario (VTN published variable rates 20:00-07:00):
  EV would have stayed FLEXIBLE for 20:00-07:00.
  Envelope: EnergyNeeded=26.3kWh, MaxPower=7kW, Window=20:00-07:00, MaxRate=€0.38/kWh
  VTN sees: "this VEN has 26.3 kWh shiftable demand overnight."
  If VTN sends €0.05 for 02:00-04:00 → next replan → EV charges then.
```

**Phase 8: Finalize**
```
Plan summary:
  EV: charges 20:00-23:45 (off-peak), 25 kWh, €3.00, reaches ~78% SoC
    Remaining 1.3 kWh → CONTINUE with €0.02 bid (needs very cheap energy)
  Battery: charges 14:00-17:00 (mostly PV surplus at €0.08 opportunity cost),
    discharges 17:00-18:15 (saves €1.95 vs grid import at peak)
  Heat pump: runs continuously, peak hours partly covered by battery discharge
  PV surplus 14:00-17:00: consumed by battery, no export needed
  Envelopes: none (all demand committed)

PlanWarnings:
  INFO: "EV charge estimated at €3.00 (budget: €3.00). Will reach 78% by 07:00."
  INFO: "Remaining 2% continues at €0.02 bid. Consider raising bid to €0.10 for PV surplus."

Total planned cost: ~€5.10 (EV €3.00 + battery charge €0.72 + heat pump peak ~€1.40)
Compared to no optimization: ~€9.50 (all at average rate, no PV self-consumption)
Savings: ~€4.40 per day from battery time-shifting and off-peak EV charging.

Note: PV surplus is no longer "free" in the accounting. The battery "pays" €0.08/kWh
(forgone export revenue) for surplus energy, which is correct — that revenue is real.
But the round-trip savings (charge at €0.08, discharge displaces €0.442) are still
substantial: €0.355/kWh net benefit per kWh stored.
```

---

*End of Step 4 (Draft 4). Changes from Draft 3:*
- *PV surplus is no longer "free" — it costs ExportPrice (forgone export revenue). This is the opportunity cost of self-consumption. All EffectiveCost calculations are now surplus-aware.*
- *CalcCache (§3): added SurplusForPacket_kW, ImportForPacket_kW. EffectiveCost is now blended (surplus × ExportPrice + import × ImportPrice) / MaxPower, with zero CO2 for surplus portion.*
- *Phase 1 (§5): added SurplusAvailable_kW per slot = max(0, -BaselineLoad). Surplus is a shared pool tracked across packets.*
- *Phase 1 (§5): EffectiveCost no longer pre-computed per slot — it depends on surplus availability which varies per-packet.*
- *Phase 2 (§6): surplus-aware EffectiveCost computation with three cases (pure surplus, pure import, blended). surplusTentativelyClaimed[] tracks forward projection of surplus consumption.*
- *Phase 3 (§7): added slotRemainingSurplus[] tracking. Allocation splits power into surplusUsed + gridUsed. Cost = surplus×ExportPrice + grid×ImportPrice. CO2 only from grid portion. Budget check consumes surplus first (cheapest).*
- *Phase 4 (§8): battery charge cost from surplus uses ExportPrice as ChargeCost (not €0). Round-trip test uses correct opportunity cost.*
- *Phase 5 (§9): renamed "Allocate Residual PV Surplus". No longer makes self-consume decision (handled by Phase 3 sort). Only handles residual export after all packets and storage have claimed surplus. Includes worked explanation of why Phase 3 sort produces correct self-consumption.*
- *PacketAllocation (Step 1 §6.3): added SurplusPower_kW, GridPower_kW split.*
- *PlanTimeSlot (Step 1 §6.2): added SurplusAvailable_kW field.*
- *Updated worked example (§15): all phases show surplus-aware costs. Battery charges at €0.08 opportunity cost (not €0). ComfortBid eligibility gate correctly rejects PV surplus when ExportPrice > ComfortBid.*

*Changes from Drafts 1–3 (preserved):*
- *Two-layer plan, FlexibilityEnvelope, ComfortBid dual role, SITE_RESIDUAL, UndeliveredEnergy rename*

*Proceed to Step 6 (Validation & Issue Identification) after review.*
