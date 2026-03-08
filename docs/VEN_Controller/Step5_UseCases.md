# Step 5 — Use Cases: VEN Controller (HEMS)

**Scope:** Single-site residential Home Energy Management System acting as OpenADR 3.1 VEN.  
**Version:** Draft 2  
**Prerequisite:** Step 1 Entity Model (Draft 6), Step 2 Architecture (Draft 5), Step 3 Data Flow (Draft 4), Step 4 Algorithm (Draft 3)

---

## Overview

Twelve use cases in four categories. Each traces through the actual entities, components,
and algorithm phases defined in Steps 1–4. The goal is to verify that every entity has a
purpose, every component knows its role, and the algorithm produces correct behavior.

```
Normal Operation (UC-01 to UC-04):
  UC-01  EV overnight charge                         — standard FIRM + FLEXIBLE flow
  UC-02  Washing machine batch run                   — CONTINUE policy, batch risk
  UC-03  PV surplus cascade                          — self-consume → store → export
  UC-04  Day-ahead price update from VTN             — RATE_CHANGE replan cycle

VTN Coordination (UC-05 to UC-07):
  UC-05  VTN sends favorable far-horizon pricing     — flexibility → firm-up
  UC-06  Grid emergency alert                        — synthetic packet, priority override
  UC-07  VTN capacity reservation request            — capacity management flow

Edge Cases (UC-08 to UC-10):
  UC-08  EV disconnects mid-charge                   — session failure, asset state change
  UC-09  Tier fallback on time constraint             — multi-tier degradation, budget inversion
  UC-10  Peak demand penalty avoidance                — MeasurementWindow, rolling average breach

Stress / Degraded (UC-11 to UC-12):
  UC-11  Consumption-only site (no PV, no battery)   — algorithm without generation
  UC-12  VTN communication loss                      — stale data, conservative planning

Additional (UC-13 to UC-14):
  UC-13  VTN direct override (DISPATCH_SETPOINT)     — override session, compliance
  UC-14  Thermal feedback loop (heat pump temp drop)  — weather → energy → schedule → temp
```

---

## UC-01: EV Overnight Charge

**Scenario:** User plugs in EV at 18:00, wants 80% SoC by 07:00 tomorrow morning, budget €3.

### Preconditions
```
Time:             18:00
EV.State:         SoC = 30%, IsConnected = true, Responsiveness = RESPONSIVE
EV.Profile:       MaxPower = 7kW, Adjustability = STEPLESS, AutoFollow = false
Rates:            peak 18:00-20:00 (€0.40/kWh), off-peak 20:00-06:00 (€0.12/kWh),
                  morning 06:00-14:00 (€0.22/kWh)
NearHorizonDuration: 2h
```

### Step-by-step

**1. User Request → EnergyPacket (User Request Manager)**
```
UserRequest:
  AssetID = "ev_01", Mode = BY_DEADLINE
  Deadlines: [ {Deadline: "2025-03-15T07:00", MaxCost: €3.00} ]
  
  (User's UI showed: "by tomorrow morning" → resolved to 07:00 tomorrow by the
  presentation layer. The entity model never sees natural language — UserDeadline
  contains a concrete RFC 3339 timestamp. Shortcuts like "tonight", "tomorrow morning",
  "this weekend" are a UI concern above the User Request Manager.)

User Req Mgr translates:
  EnergyPacket:
    TargetEnergy = 50kWh × (0.80 - 0.30) = 25 kWh
    EarliestStart = 18:00 (now)
    LatestEnd = 07:00 tomorrow
    CompletionPolicy = CONTINUE (EV default)
    PostDeadlineComfortBid = €0.02 (low: opportunistic top-up)
    ValueCurve:
      DeadlineTiers: [ {Deadline: 07:00 tomorrow, MaxTotalCost: €3.00, MaxMarginalRate: €0.30} ]
      ComfortRates: [0%→€0.40, 50%→€0.25, 80%→€0.10, 100%→€0.02]
    Status = PENDING

→ PlanTrigger.USER_REQUEST
→ UserNotification: "EV charge to 80% accepted. Estimating cost..."
```

**2. Planner: Phase 1 — Prepare**
```
Grid: 156 slots (18:00 → 07:00+, 5-min steps)
FirmBoundary = 20:00 (18:00 + 2h)

Slot classification:
  18:00-20:00: FIRM (within near-horizon)
  20:00-07:00: FLEXIBLE

  EV TimeSlack: 13h available, ~3.6h needed (25kWh / 7kW). Comfortable.
  No urgency override. 20:00-07:00 stays FLEXIBLE.

Early firm-up check:
  Off-peak 20:00-06:00 all at €0.148 effective (€0.12 + CO2).
  Morning 06:00-07:00 at €0.256 effective (€0.22 + CO2).
  Rate variance across FLEXIBLE window > 10% → flexibility preserved.
  (If off-peak were flat all the way to 07:00, early firm-up would trigger.)

Baseline: heat pump 3kW + SITE_RESIDUAL 0.5kW = 3.5kW continuous import.
```

**3. Planner: Phase 2 — Score (FIRM slots only: 18:00-20:00)**
```
EV at 18:00: EffectiveCost = €0.442 (peak)
  Fill = 30% → ComfortBid = €0.38
  WithinComfortBid: 0.442 > 0.38 → INELIGIBLE
  EV won't charge during peak. Too expensive for comfort value at this fill.

EV at 19:00: EffectiveCost = €0.442 → same result. INELIGIBLE.

Result: zero FIRM allocations for EV. All energy deferred to FLEXIBLE window.
```

**4. Planner: Phase 7 — Build Flexibility Envelope**
```
EV packetEnergyToAllocate = 25 kWh (nothing allocated in FIRM)

FlexibilityEnvelope:
  EnergyNeeded = 25 kWh
  MaxPower = 7 kW
  WindowStart = 20:00, WindowEnd = 07:00
  SlotsAvailable = 132 (11h × 12 slots/h)
  MaxAcceptableRate = min(€0.38, €0.30) = €0.30 (tier ceiling wins)
  BudgetRemaining = €3.00
  EstimatedCost = 25 × avg(€0.148, €0.256) ≈ €3.85
    → exceeds budget. Planner notes: "can only afford ~20 kWh at these rates"
```

**5. Planner: Phase 8 — Finalize + Post-Plan**
```
EstimatedCost = €3.00 (budget-capped)
EstimatedCompletion = (PastEnergy 0 + FirmEnergy 0 + affordable 20kWh) / 25kWh = 0.80
  → will reach ~74% SoC (30% + 44%×0.80), not quite 80%.
  → Remaining 5kWh → post-deadline CONTINUE at €0.02 (free energy next day)

PlanWarning INFO: "EV will reach ~74% by 07:00 within budget. Remaining 5kWh
  continues at low priority for free/cheap energy."

UserNotification: "EV charge estimated at €3.00. Will reach ~74% by 07:00.
  Remaining charge continues opportunistically."
```

**6. OpenADR IF: Report Flexibility to VTN**
```
USAGE_FORECAST report:
  18:00-20:00: 0 kW (EV idle)  — point forecast (FIRM)
  20:00-07:00: 0-7 kW range    — flexible demand (from envelope)

DOWN_REGULATION_AVAILABLE:
  20:00-07:00: 7 kW shiftable demand available

VTN sees: "this VEN has 25 kWh of flexible overnight demand."
```

**7. Sliding window: 20:00 replan**
```
FirmBoundary slides to 22:00.
20:00-22:00 slots become FIRM. Off-peak rate €0.148.

Phase 2 scores EV at 20:00:
  Fill = 30% → ComfortBid = €0.38
  WithinComfortBid: 0.148 < 0.38 → eligible
  WithinMarginalRate: 0.148 < 0.30 → eligible
  WithinBudget: 7kW × 5min × €0.12 per slot → yes

Phase 3 allocates: EV charges at 7kW, 20:00-22:00.
  Energy in FIRM = 7kW × 2h = 14 kWh
  Remaining envelope: 11 kWh in 22:00-07:00 FLEXIBLE window

Dispatcher starts DeviceSession. EV begins charging.
```

**8. Completion: 23:30**
```
EV reaches budget limit (€3.00) at approximately 23:30.
  Delivered: ~20.8 kWh at €0.12 = €2.50 (20:00-23:30)
  + some higher-cost initial slots → total ≈ €3.00

Dispatcher: AccumulatedCost_EUR ≈ €3.00 = MaxTotalCost.
  → No more eligible slots in Tier 1.
  → Tier 1 exhausted. Post-deadline CONTINUE kicks in.
  → PostDeadlineComfortBid = €0.02.
  → Off-peak EffectiveCost €0.148 > €0.02 → ineligible.
  → Tomorrow PV surplus: EffectiveCost = ExportPrice €0.08 > €0.02 → also ineligible.
  → EV needs energy where EffectiveCost < €0.02. Unlikely unless ExportPrice is very low.
  
  Design note: PostDeadlineComfortBid €0.02 is essentially "only if curtailed PV is free."
  For typical ExportPrices (€0.05-€0.10), the EV won't charge post-deadline.
  User could raise PostDeadlineComfortBid to €0.10 to accept PV surplus.

Status: SCHEDULED (waiting for ultra-cheap energy). May eventually PARTIAL_COMPLETED
if user decides to accept, or stays indefinitely at low priority.
```

### Entities exercised
```
UserRequest, EnergyPacket, ValueCurve, DeadlineTier, ComfortRate, CompletionPolicy,
FlexibilityEnvelope, PlanTimeSlot (FIRM + FLEXIBLE), PacketAllocation, DeviceSession,
SiteMeter, SITE_RESIDUAL, AssetState, DispatchCommand, PlanWarning, UserNotification
```

---

## UC-02: Washing Machine Batch Run

**Scenario:** User starts washing machine at 10:00. Cycle takes ~2h at 2kW. User says "run when cheap, done by 14:00."
(UI resolves "done by 14:00" → concrete timestamp 2025-03-15T14:00.)

### Preconditions
```
Time:             10:00
Asset:            WASHING_MACHINE, 2kW fixed (ON_OFF), AutoFollow = false
Rates:            10:00-12:00: €0.25/kWh, 12:00-14:00: €0.18/kWh
PV forecast:      5kW at 10:00, 7kW at 12:00
```

### Step-by-step

**1. User Request → EnergyPacket**
```
EnergyPacket:
  TargetEnergy = 4 kWh (2kW × 2h)
  EarliestStart = 10:00
  LatestEnd = 14:00
  CompletionPolicy = CONTINUE (washing machine default, high bid)
  PostDeadlineComfortBid = €5.00 (mid-cycle: must finish, but competes on price)
  DeadlineTiers: [ {Deadline: 14:00, MaxTotalCost: €2.00, MaxMarginalRate: €0.50} ]
  ComfortRates: [0%→€0.50, 50%→€0.40, 100%→€0.20]
  Status = PENDING
```

**2. Planner: Phase 1**
```
Grid: 48 slots (10:00 → 14:00)
FirmBoundary = 12:00 (10:00 + 2h)

  10:00-12:00: FIRM
  12:00-14:00: FLEXIBLE

Washer needs 24 slots (2h). Only 48 available. TimeSlack = 24 slots. Moderate.
No urgency override yet.

Early firm-up: 12:00-14:00 at €0.18 vs 10:00-12:00 at €0.25.
  Variance > 10% → flexibility preserved (12:00-14:00 stays FLEXIBLE).
```

**3. Planner: Phase 2 — Score (FIRM: 10:00-12:00)**
```
Washer at 10:00: 
  PV surplus exists: baseline = -5kW + 3kW + 0.5kW = -1.5kW → SurplusAvailable = 1.5kW
  SurplusForPacket = min(2kW, 1.5kW) = 1.5kW
  ImportForPacket = 2 - 1.5 = 0.5kW
  EffectiveCost = (1.5×€0.08 + 0.5×€0.285) / 2 = €0.131/kWh (blended)
  Fill = 0% → ComfortBid = €0.50
  WithinComfortBid: 0.131 < 0.50 → eligible
  MarginalValue = 0.50 × 1.0 = 0.50

Washer at 12:00 (FLEXIBLE, stronger PV):
  PV surplus: baseline = -7kW + 3kW + 0.5kW = -3.5kW → SurplusAvailable = 3.5kW
  SurplusForPacket = min(2kW, 3.5kW) = 2kW (all from surplus)
  ImportForPacket = 0kW
  EffectiveCost = €0.08/kWh (pure surplus: ExportPrice opportunity cost)
  Even cheaper than 10:00 blended.
```

**4. Batch Risk Check (§13)**
```
Washer is CONTINUE with high bid (€5.00). Duration = 2h.

If started at 10:00, runs 10:00-12:00.
  Window slots: all at €0.25 (FIRM). No penalty risk.
  riskCost = 0. Safe to start.

If started at 12:00, runs 12:00-14:00.
  Window slots: all at €0.18 (cheaper). No penalty risk.
  Better: cheaper slots AND PV surplus is stronger at noon.
```

**5. Decision: defer to 12:00**
```
Phase 3 defers start to 12:00 because:
  - 12:00: pure PV surplus at €0.08/kWh (opportunity cost only)
  - 10:00: blended €0.131/kWh (partial surplus, partial import)
  - Greedy sort: 12:00 (€0.08) < 10:00 (€0.131). Washer goes to 12:00.
  - Batch risk check: 12:00-14:00 is safe (no expensive slots in window).

But 12:00-14:00 is FLEXIBLE. So:
  FlexibilityEnvelope: 4 kWh, 2kW, window 12:00-14:00, MaxRate €0.50

At 12:00 replan, these slots become FIRM. Washer starts.
```

**6. Execution: 12:00-14:00**
```
Dispatcher starts DeviceSession at 12:00.
Washer runs at 2kW. PV surplus covers all consumption.
  SurplusPower = 2kW, GridPower = 0kW.
  CostInSlot = 2kW × €0.08 × 2h = €0.32 (opportunity cost of forgone export)
Actual cost: €0.32 (not "free" — we forgo €0.32 in export revenue).
But much cheaper than grid import at €0.18 (would have been €0.72 for 4kWh).
Net saving from self-consumption: €0.72 - €0.32 = €0.40.

At 14:00: FillPercentage = 1.0 → Status = COMPLETED
DeviceSession closed. UserNotification: "Washing done. Cost: €0.32."
```

**7. What if washer overruns past 14:00?**
```
CompletionPolicy = CONTINUE, PostDeadlineComfortBid = €5.00.
  €5.00 > any EffectiveCost → always eligible post-deadline.
  MarginalValue = 5.00 × 1.0 = 5.00 (very high priority).
  Washer keeps running, beats almost everything for slot access.
  Only a grid emergency synthetic packet (bid €15.00) would displace it.

After cycle finishes: Status = COMPLETED. Cost slightly higher than estimated.
```

### Entities exercised
```
CompletionPolicy = CONTINUE, PostDeadlineComfortBid (high), batch risk heuristic,
PV self-consumption (Phase 5), FlexibilityEnvelope with short window, DeviceSession
```

---

## UC-03: PV Surplus Cascade

**Scenario:** Sunny afternoon. PV produces more than the site consumes. System decides: self-consume, store, or export.

### Preconditions
```
Time:             13:00
PV.State:         ActualPower = -8 kW (producing)
Battery.State:    SoC = 40%, MaxCharge = 5kW, MaxDischarge = 5kW
Heat pump:        consuming 3kW (baseline)
SITE_RESIDUAL:    0.5kW (baseline)
EV:               not connected
Rates:            Import €0.22/kWh, Export €0.08/kWh
```

### Step-by-step

**1. Baseline computation (Phase 1)**
```
BaselineLoad = heat pump 3kW + SITE_RESIDUAL 0.5kW + PV -8kW = -4.5kW
  → 4.5kW net surplus available.
```

**2. Phase 3 + Phase 4: Surplus-aware allocation (FIRM slots, 13:00-15:00)**
```
For each FIRM slot at 13:00:
  SurplusAvailable = 4.5kW (after heat pump and residual baseline)

  Battery has active charge packet (charge to 90% by 17:00 for evening discharge).
  Phase 4 evaluates battery charge from surplus:
    ChargeCost = ExportPrice €0.08/kWh (opportunity cost of surplus)
    ChargeOpportunityCost = €0.08 / 0.92 = €0.087/kWh (efficiency-adjusted)
    Future discharge displaces import at €0.442/kWh (peak).
    Round-trip profit: €0.442 - €0.087 = €0.355/kWh. Highly profitable.
    → Battery charges at 4.5kW (capped at MaxCharge 5kW → 4.5kW OK)
    → slotRemainingSurplus = 0

  No other consumption packets active. No surplus remaining.
  
Phase 5: Residual PV Surplus
  slotRemainingSurplus[13:00] = 0 (battery claimed all). Nothing to export.
```

**3. SiteMeter confirms (Monitor cycle)**
```
SiteMeter.NetImport = -0.0kW (balanced: PV → heat pump + residual + battery)
  PV: -8kW
  Heat pump: +3kW
  SITE_RESIDUAL: +0.5kW
  Battery charge: +4.5kW
  Net: 0kW (neither importing nor exporting)

SITE_RESIDUAL.State.ActualPower = SiteMeter.NetImport - Σ(known assets)
  = 0 - (-8 + 3 + 4.5) = 0 - (-0.5) = 0.5kW ✓ (matches forecast)
```

**4. 14:30 — Cloud rolls in**
```
PV drops to 3kW. Baseline becomes: 3kW + 0.5kW - 3kW = +0.5kW (net import).
Surplus gone. Battery charging stops (no free energy).

Monitor detects deviation:
  Planned: battery charging at 4.5kW from PV
  Actual: PV only 3kW, deficit
  NetDeviation = significant → PlanTrigger.DEVICE_DEVIATION

Replan: battery charge paused. Heat pump + residual served from grid at €0.22.
Battery SoC = ~60% (charged from 40% over 1.5h at ~4kW avg).
Remaining charge to 90% deferred to next PV window or cheap off-peak.
```

**5. 15:30 — Sun returns**
```
PV back to 6kW. Surplus = 6 - 3 - 0.5 = 2.5kW.
Battery resumes charging at 2.5kW.
No export needed (battery not full, peak discharge valuable).
```

### Priority chain verified
```
The old "Self-consume > Store > Export" priority is now emergent from EffectiveCost:
  - Self-consume: EffectiveCost = ExportPrice (€0.08). Cheap.
  - Store for later discharge: ChargeCost = ExportPrice (€0.08), displaces peak (€0.44).
  - Export: earns ExportPrice (€0.08). Same revenue as opportunity cost.
  - Greedy sort handles this automatically — no explicit priority chain needed.

In this scenario:
  1. Heat pump served from PV (in baseline — consumed before surplus computed) ✓
  2. Battery charges from surplus at €0.08/kWh (stores for peak) ✓
  3. Export: no surplus remaining after battery ✓
```

### Entities exercised
```
PV forecast, SITE_RESIDUAL (measurement and verification), SiteMeter (balance check),
Phase 5 PV surplus cascade, Phase 4 storage round-trip efficiency test,
PlanTrigger.DEVICE_DEVIATION (cloud event), battery charge/discharge packets
```

---

## UC-04: Day-Ahead Price Update from VTN

**Scenario:** At 16:00, VTN publishes new prices for tomorrow. Some slots are cheaper than today's forecast.

### Preconditions
```
Time:             16:00
Current rates:    known until midnight (day-ahead from previous publication)
Active packets:   EV (pending, charges tonight), battery (discharge planned 17:00-20:00)
```

### Step-by-step

**1. VTN event received (OpenADR IF)**
```
VTN publishes PRICE event for tomorrow 00:00-24:00:
  00:00-06:00: €0.08/kWh (wind surplus, very cheap)
  06:00-09:00: €0.30/kWh (morning peak)
  09:00-16:00: €0.15/kWh (solar + moderate demand)
  16:00-20:00: €0.35/kWh (evening peak)
  20:00-24:00: €0.12/kWh (off-peak)

OpenADR IF translates:
  For each interval in event.intervals:
    PlannedRates.append(RateSnapshot(interval.timeStamp, ImportPrice, ExportPrice, CO2))

→ PlanTrigger.RATE_CHANGE
```

**2. Planner replan (triggered by RATE_CHANGE)**
```
New rate landscape extends planning horizon to include tomorrow.

EV packet still has FlexibilityEnvelope for tonight 20:00-07:00.
New information: 00:00-06:00 tomorrow is €0.08 (cheaper than tonight's off-peak €0.12).

Phase 1: FLEXIBLE slots 00:00-06:00 now show lower EffectiveCost.
  Rate variance in EV's FLEXIBLE window increases (€0.148 vs €0.108).
  Flexibility preserved (variance > 10%).

Phase 7: Updated FlexibilityEnvelope for EV:
  Same EnergyNeeded, but EstimatedCost drops (more cheap slots available).
  Budget fits better: 25kWh × €0.08 = €2.00 < €3.00 budget.
  → EV could fully charge within budget using 00:00-06:00 window.

Post-Plan: EstimatedCost revised downward.
  UserNotification: "Updated estimate: EV charge ~€2.00 (was ~€3.00). Cheaper overnight rates available."
```

**3. OpenADR IF reports updated flexibility**
```
DOWN_REGULATION_AVAILABLE updated:
  20:00-00:00: 7kW (could charge here, but prefers to wait)
  00:00-06:00: 7kW (preferred window, cheapest)

VTN now sees: "VEN prefers 00:00-06:00 for 25kWh flexible demand."
```

**4. Sliding window resolves: midnight**
```
FirmBoundary reaches 00:00. Slots 00:00-02:00 become FIRM.
Phase 2 scores: €0.108 effective, ComfortBid €0.38 → eligible, cheap.
Phase 3 allocates: EV charges at 7kW from 00:00.

By 03:35: EV fully charged. 25kWh × €0.08 = €2.00. Budget saved €1.00 vs off-peak.
Status = COMPLETED.
```

### Entities exercised
```
PlanTrigger.RATE_CHANGE, RateSnapshot update, FlexibilityEnvelope revision,
EstimatedCost downward adjustment, UserNotification update, FIRM boundary slide,
Budget check (fits now where it was tight before)
```

---

## UC-05: VTN Sends Favorable Far-Horizon Pricing

**Scenario:** VTN actively signals a cheap window to attract flexible demand.

### Preconditions
```
Time:             19:00
EV:               plugged in, 25kWh needed by 07:00, budget €3.00
Current plan:     FlexibilityEnvelope reported: 25kWh, 20:00-07:00, MaxRate €0.30
Off-peak rate:    €0.12/kWh (standard)
```

### Step-by-step

**1. VTN responds to flexibility report**
```
VTN sees aggregated flexible demand from multiple VENs.
Sends targeted PRICE event:
  02:00-04:00: €0.03/kWh (incentive price for wind surplus absorption)

This is an OpenADR event with payload type PRICE, intervals at 02:00-04:00.
```

**2. OpenADR IF processes**
```
PlannedRates[02:00-04:00].ImportPrice = €0.03/kWh
→ PlanTrigger.RATE_CHANGE
```

**3. Planner replan**
```
Phase 1:
  FirmBoundary = 21:00 (19:00 + 2h)
  20:00-07:00 remains FLEXIBLE for EV.
  FLEXIBLE rate variance now very high: €0.148 vs €0.065 (02:00-04:00).
  Flexibility preserved — the cheap window is the reason to hold.

Phase 7: FlexibilityEnvelope updated:
  EstimatedCost = 25kWh × €0.03 = €0.75 (if all energy fits in 02:00-04:00)
  Check: 2h × 7kW = 14kWh. Need 25kWh. Can't fit entirely.
  Revised: 14kWh at €0.03 + 11kWh at €0.12 = €0.42 + €1.32 = €1.74

UserNotification: "EV charge estimate revised to ~€1.74 (VTN offering cheap rate 02:00-04:00)."
```

**4. 02:00 — FIRM boundary reaches cheap window**
```
Slots 02:00-04:00 become FIRM. EffectiveCost = €0.065.
Phase 3: EV charges at 7kW. 14kWh delivered.

04:00-07:00: remaining 11kWh at off-peak €0.148.
  FirmBoundary slides. Allocates as slots become FIRM.

By 05:35: EV fully charged. Total cost: €1.74.
  Savings vs blind off-peak (€3.16): €1.42.
  Savings vs budget (€3.00): €1.26 returned.
```

### Key insight
```
Without FlexibilityEnvelopes: Planner would have committed EV to 20:00-23:45 at €0.12.
  Total cost: €3.00 (budget-limited). VTN incentive arrives but EV already committed.

With FlexibilityEnvelopes: EV waited. VTN signal resolved the flexibility optimally.
  Both user and grid benefit: user pays less, grid absorbs wind surplus.
```

### Entities exercised
```
FlexibilityEnvelope (the core mechanism), RATE_CHANGE from VTN incentive pricing,
far-horizon flexibility preserved until resolved, EstimatedCost revision,
DOWN_REGULATION_AVAILABLE → VTN response → PRICE event → resolution
```

---

## UC-06: Grid Emergency Alert

**Scenario:** VTN sends ALERT_GRID_EMERGENCY at 18:30 during peak. System must shed load immediately.

### Preconditions
```
Time:             18:30
Active:           Heat pump 6kW (STEPPED), EV charging 7kW, Battery idle (SoC 80%)
SiteMeter:        NetImport = 13.5kW (6 + 7 + 0.5 residual)
```

### Step-by-step

**1. OpenADR IF receives alert**
```
Event: ALERT_GRID_EMERGENCY
  Payload: SIMPLE with level = 3 (maximum shed)
  Duration: PT30M (30 minutes)

OpenADR IF creates synthetic EnergyPacket:
  PacketID = "alert_grid_emergency_001"
  AssetID = null (affects whole site)
  TargetEnergy = reduce import to minimum
  ComfortBid = €5.00 (extremely high priority)
  TimePressure = 3.0 (emergency)
  MarginalValue = 5.00 × 3.0 = 15.00 (beats everything)

→ PlanTrigger.ALERT (immediate replan, bypasses ReplanCooldown)
```

**2. Planner: immediate replan**
```
Emergency packet has MarginalValue = 15.00.
All other packets: EV MarginalValue ≈ 0.38, Heat pump ≈ 0.20.

Phase 3 allocates emergency packet first for 18:30-19:00 slots:
  Emergency needs capacity. EV and heat pump must yield.

  EV: STEPLESS → can reduce to 0. Lost allocation for these 6 slots.
    EV pauses. Status = PAUSED. DeviceSession suspended.

  Heat pump: STEPPED (0/3/6kW) → reduces from 6kW to 3kW.
    Keeps minimum comfort. Lost 3kW × 30min = 1.5kWh.

  Battery: switches to discharge. 5kW discharge offsets remaining import.
    SiteMeter target: import → 3kW + 0.5kW - 5kW = -1.5kW (net export).
    Grid sees site as exporter during emergency. Maximum contribution.
```

**3. Dispatcher executes (immediate)**
```
DispatchCommand to EV: CommandedPower = 0kW, Reason = "emergency override"
DispatchCommand to Heat pump: CommandedPower = 3kW, Reason = "emergency override"
DispatchCommand to Battery: CommandedPower = -5kW (discharge), Reason = "emergency override"

SiteMeter.NetImport drops from 13.5kW to -1.5kW within ResponseDelay_s.
```

**4. Monitor tracks compliance**
```
For each 5-second cycle during 30 minutes:
  NetDeviation = SiteMeter.NetImport - planned (should be ~ -1.5kW)
  AssetLedger updated: battery discharge attributed to emergency
  PenaltyRule for EVENT_NONCOMPLIANCE: checking if actual import ≤ target
```

**5. 19:00 — Alert expires**
```
Emergency synthetic packet: Status = COMPLETED (duration fulfilled).
→ PlanTrigger.PERIODIC (normal replan)

Planner restores normal plan:
  EV resumes charging: Status = PAUSED → ACTIVE
  Heat pump returns to 6kW
  Battery stops discharging (SoC = ~72%, enough for reduced evening peak)

DeviceSession for EV resumes. EnergyPacket.AccumulatedCost updated for pause gap.
```

### Entities exercised
```
ALERT_GRID_EMERGENCY, synthetic EnergyPacket with extreme bid, PlanTrigger.ALERT,
STEPPED asset reduction, battery emergency discharge, PAUSED status,
DeviceSession suspension/resume, EVENT_NONCOMPLIANCE penalty tracking
```

---

## UC-07: VTN Capacity Reservation Request

**Scenario:** VTN publishes available capacity and fee. VEN decides whether to reserve additional import capacity for tomorrow's EV charge.

### Preconditions
```
Time:             20:00
EV:               needs 40kWh by 07:00 tomorrow (large battery, low SoC)
Current capacity:  ImportSubscription = 10kW
EV.MaxPower:      11kW (exceeds subscription)
Heat pump:        3kW baseline
SITE_RESIDUAL:    0.5kW
Available:        10 - 3 - 0.5 = 6.5kW for EV (can't use full 11kW)
```

### Step-by-step

**1. Planner detects capacity shortfall**
```
EV needs 40kWh in ~10h overnight at 6.5kW effective = 6.15h minimum.
  But EV has STEPLESS 0-11kW. At 6.5kW (subscription-limited): ~6.15h.
  Fits within 10h window. But tight if VTN sends another signal.

If subscription were 15kW:
  Available for EV: 15 - 3.5 = 11.5kW → full 11kW usable.
  40kWh / 11kW = 3.6h. Much more comfortable. More flexibility.

Post-Plan step generates PlanWarning:
  INFO: "EV charge running at reduced power due to capacity subscription.
  Could benefit from additional 5kW reservation 22:00-06:00."
```

**2. VTN publishes CAPACITY_AVAILABLE event**
```
CAPACITY_AVAILABLE: 5kW additional import available, 22:00-06:00
CAPACITY_AVAILABLE_FEE: €0.02/kW/h (€0.10/kWh effective for 5kW)
```

**3. OpenADR IF evaluates**
```
Reservation cost: 5kW × 8h × €0.02 = €0.80
Benefit: EV charges at 11kW instead of 6.5kW.
  Time saved: 6.15h → 3.6h. Frees 2.55h of flexibility.
  Flexibility value: could shift EV to cheapest 3.6h window instead of needing 6h.
  Estimated savings from better scheduling: ~€0.50-€1.00

Decision threshold: reservation cost (€0.80) vs flexibility value (€0.50-€1.00).
  Marginal. System decides based on CO2Weight and user preferences.
  If CO2 benefit is significant → reserve (charge faster during wind).
  If purely cost → borderline, may not reserve.

Assume reserve:
  OadrCapacityRequest:
    Direction = IMPORT
    RequestedPower = 5kW
    Intervals = [22:00-06:00]
    OfferedFee = €0.02/kW/h
```

**4. VTN grants reservation**
```
IMPORT_CAPACITY_RESERVATION event: 5kW granted, 22:00-06:00.
OadrCapacityState.ImportReservation = 5kW (additive to subscription)
Effective limit 22:00-06:00: 10 + 5 = 15kW.

→ PlanTrigger.CAPACITY_CHANGE → replan

Planner: EV can now charge at full 11kW. EnergyToAllocate fills faster.
Flexibility increases: can serve EV in 3.6h, leaving 6.4h for VTN coordination.
```

### Entities exercised
```
OadrCapacityState, OadrCapacityRequest, CAPACITY_AVAILABLE / CAPACITY_AVAILABLE_FEE,
IMPORT_CAPACITY_RESERVATION, PlanTrigger.CAPACITY_CHANGE, ImportCapacityLimit per slot,
cost-benefit evaluation for reservation
```

---

## UC-08: EV Disconnects Mid-Charge

**Scenario:** EV is charging at 22:00. User unplugs at 22:15 (forgot something, drives away).

### Preconditions
```
Time:             22:15
EV:               Status = ACTIVE, SoC = 55%, charging at 7kW
DeviceSession:    active since 20:00, DeviationCount = 0
EnergyPacket:     FillPercentage = 0.50, AccumulatedCost = €1.50
```

### Step-by-step

**1. Asset Controller detects disconnection**
```
EV.State.IsConnected = false (physical disconnect detected)
EV.State.ActualPower = 0kW (no longer consuming)
EV.State.Responsiveness = OFFLINE

→ PlanTrigger.ASSET_STATE_CHANGE
→ AssetState update propagates to all readers
```

**2. Dispatcher reacts (next cycle, <5s)**
```
Current slot has PacketAllocation for EV at 7kW.
Dispatcher tries to send DispatchCommand: CommandedPower = 7kW.
  Asset Controller reports: device offline, command not deliverable.

Dispatcher:
  DeviceSession.EndTime = 22:15
  DeviceSession.EndReason = "asset_disconnected"
  EnergyPacket.Status = PAUSED (not FAILED — EV might return)
  DispatchState.NetDeviation = -7kW (planned 7kW import, actual 0kW)
```

**3. Monitor detects deviation**
```
SiteMeter.NetImport drops by 7kW.
NetDeviation = -7kW (importing less than planned). Significant.
→ PlanTrigger.DEVICE_DEVIATION already covered by ASSET_STATE_CHANGE.

Monitor updates:
  SITE_RESIDUAL.State.ActualPower = SiteMeter.NetImport - Σ(known assets)
  (Correctly attributes the change to EV disconnect, not to residual)
```

**4. Planner replan (triggered by ASSET_STATE_CHANGE)**
```
EV.State.IsConnected = false.
Phase 2: all EV slots → Skip (device not available).
Phase 7: FlexibilityEnvelope cannot be built (asset unavailable).

EnergyPacket stays PAUSED. No allocations, no envelope.
PlanWarning CRITICAL: "EV disconnected at 55% SoC. Cannot complete charge.
  Will resume when reconnected."

UserNotification: "EV disconnected. Charge paused at 55%.
  Reconnect before 07:00 to complete charge to 80%."
```

**5. 23:30 — User returns, plugs EV back in**
```
EV.State.IsConnected = true
EV.State.Responsiveness = RESPONSIVE (after confirmation delay)
→ PlanTrigger.ASSET_STATE_CHANGE

Planner replan:
  EV needs 12.5 kWh more (80% - 55% = 25% × 50kWh).
  Available: 23:30-07:00 = 7.5h. At 7kW: 1.8h needed. Comfortable.
  
  EnergyPacket.Status = PAUSED → SCHEDULED (planner assigns slots)
  New DeviceSession created.

Dispatcher begins executing at next cheap FIRM slot.
EV finishes by ~01:15. Status = COMPLETED.

AccumulatedCost = €1.50 (before) + ~€0.22 (after) = €1.72 total.
Well within €3.00 budget despite interruption.
```

### Entities exercised
```
AssetState.IsConnected, DeviceResponsiveness = OFFLINE, PlanTrigger.ASSET_STATE_CHANGE,
DeviceSession termination + new session creation, PAUSED status,
SiteMeter deviation detection, PlanWarning CRITICAL, UserNotification,
budget continuity across disconnect (AccumulatedCost preserved)
```

---

## UC-09: Tier Fallback on Time Constraint

**Scenario:** User needs car tonight (alternative: €4 public transport) and also on Friday (less urgent).
(UI resolves: "tonight" → Mon 22:00, "Friday" → Fri 18:00.)

User reasoning: "I need a fully charged car tonight — I'd pay up to €5 to avoid the train.
If tonight doesn't work out, the next time I need it is Friday, no rush, €1 is enough."

### Preconditions
```
Time:             17:00 Monday
EV:               SoC = 20%, TargetSoC = 80% → 30kWh needed, MaxPower = 7kW
Rates tonight:    €0.40/kWh (peak until 20:00), €0.12/kWh (off-peak 20:00+)
```

### Step-by-step

**1. User Request → EnergyPacket**
```
UserRequest:
  AssetID = "ev_01", Mode = BY_DEADLINE
  Deadlines: [
    {Deadline: "2025-03-17T22:00", MaxCost: €5.00},   // UI: "tonight" — high value, need the car
    {Deadline: "2025-03-21T18:00", MaxCost: €1.00}    // UI: "by Friday" — low urgency, cheap only
  ]

User Req Mgr translates:
  DeadlineTiers:
    Tier 0: {Deadline: Mon 22:00, MaxTotalCost: €5.00, MaxMarginalRate: €0.50}
    Tier 1: {Deadline: Fri 18:00, MaxTotalCost: €1.00, MaxMarginalRate: €0.10}
  ActiveTierIndex = 0 (try most urgent first)
  CompletionPolicy = CONTINUE (EV default)

Tier logic: highest budget goes to soonest deadline because urgency is highest.
  Tier 0: "I want it soon and it's worth €5 to me."
  Tier 1: "If I can't get it soon, relax — I have until Friday but only spend €1."
```

**2. Planner evaluates Tier 0**
```
Tier 0: deadline Mon 22:00, budget €5.00, max rate €0.50/kWh.

  Time available: 17:00-22:00 = 5 hours.
  Energy needed: 30kWh at 7kW = 4.3h minimum. Tight but feasible.

  Peak slots 17:00-20:00: EffectiveCost = €0.442.
    WithinMarginalRate: 0.442 ≤ 0.50 → eligible (user is willing to pay peak rates).
    WithinComfortBid at 20% fill: ComfortBid = €0.40. 0.442 > 0.40 → INELIGIBLE at start.
    → At very low fill, peak is too expensive even for Tier 0. Need off-peak.

  Off-peak slots 20:00-22:00: EffectiveCost = €0.148.
    WithinComfortBid: always eligible (ComfortBid ≥ €0.40 at low fill).
    Available: 2h × 7kW = 14 kWh. Only delivers 14 of 30 kWh needed.

  Problem: can't deliver 30kWh by 22:00 with only 2h of eligible off-peak.
  Could use peak slots if ComfortBid were higher. At ~40% fill, ComfortBid = €0.25.
    Still 0.442 > 0.25 → ineligible during peak even at mid-fill.

  Result: only 14 kWh deliverable by 22:00. EstimatedCompletion = 14/30 = 0.47.
  
Post-Plan: Tier 0 infeasible (can't reach 100% completion within deadline).
  ValueCurve.ActiveTierIndex = 1
  PlanWarning WARNING: "EV can only reach ~47% of target by tonight.
    Falling back to Friday deadline."
  UserNotification: "Not enough time to fully charge by 22:00. Switching to Friday target.
    Consider taking public transport tonight (~€4)."
```

**3. Planner re-evaluates with Tier 1**
```
Tier 1: deadline Fri 18:00, budget €1.00, max rate €0.10/kWh.
  EffectiveCost at off-peak (€0.148): 0.148 > 0.10 → INELIGIBLE.
  EffectiveCost at any available slot: all exceed €0.10.

  Budget check: 30kWh × €0.12 cheapest = €3.60. Far exceeds €1.00 budget.
  Even at cheapest rate, can only afford: €1.00 / €0.12 = 8.3 kWh.
  EstimatedCompletion = 8.3 / 30 = 0.28.

  But MaxMarginalRate = €0.10 blocks everything. Zero eligible slots.
  
  Wait — this means Tier 1 is also infeasible at current rates.
  Both tiers exhausted → Status = ABANDONED? No. CompletionPolicy = CONTINUE.
  → Post-deadline CONTINUE with PostDeadlineComfortBid.
  → PostDeadlineComfortBid = €0.02. Extremely low priority, waits for very cheap energy.

PlanWarning CRITICAL: "No tier can complete EV charge within constraints.
  Tier 0 (tonight): not enough time. Tier 1 (Friday): not enough budget.
  Continuing at minimal priority. Consider increasing Friday budget."
UserNotification: "EV charge paused — no affordable slots within your settings.
  Increase budget or accept higher rates to resume charging."
```

**4. What the user should have done differently**
```
Option A: Higher Tier 1 budget.
  If Tier 1 budget were €4.00: 30kWh × €0.12 = €3.60. Feasible at off-peak.
  EV charges tonight off-peak anyway, just at relaxed deadline.

Option B: Higher MaxMarginalRate on Tier 1.
  If MaxMarginalRate were €0.15: off-peak slots at €0.148 become eligible.
  €1.00 budget buys 8.3kWh. Partial charge. Better than nothing.

Option C: Accept peak rates in Tier 0.
  If ComfortRates were higher (e.g. €0.50 at 0% fill instead of €0.40):
  Peak slots at €0.442 become eligible. Full charge by 22:00. Expensive (~€5) but car is ready.

The system correctly refuses to spend money the user hasn't authorized.
Tier fallback is graceful but can't work magic with tight constraints.
```

### Key observation
```
Tier motivation is: highest budget for soonest deadline (highest user value).
  Tier 0 (tonight, €5): "I really want this, I'll pay."
  Tier 1 (Friday, €1): "If I can't have it now, relax and go cheap."

Failure mode for Tier 0: TIME (not enough hours to deliver energy).
Failure mode for Tier 1: MONEY (enough time, but user won't pay enough).
These are different constraints, and the system handles each appropriately.
```

### Entities exercised
```
DeadlineTier (multi-tier with decreasing budget), ActiveTierIndex advancement,
tier infeasibility detection (time-constrained vs budget-constrained),
ComfortBid blocking peak slots, CompletionPolicy CONTINUE as last resort,
PlanWarning CRITICAL when all tiers exhausted, UserNotification with guidance
```

---

## UC-10: Peak Demand Penalty Avoidance

**Scenario:** Multiple assets draw simultaneously. Import approaches 15kW threshold
with €100/month penalty. MeasurementWindow determines whether a spike counts as a breach.

### Preconditions
```
Time:             18:00
PenaltyRule:
  Condition:        PEAK_DEMAND_EXCEEDED
  Threshold:        15kW
  Cost:             €100/month
  MeasurementWindow: PT15M (15-minute rolling average)
  BreachedThisPeriod: false
  RollingAverage:   13.5kW (current 15-min average)
  CurrentPeakValue: 13.8kW (highest rolling average this month)

Heat pump:        6kW (STEPPED: 0/3/6)
EV:               7kW (STEPLESS)
SITE_RESIDUAL:    0.5kW
Battery:          idle (SoC 30%)
SiteMeter.NetImport: 13.5kW (under threshold)
```

### Scenario A: Brief spike — penalty avoided

**1. Dishwasher starts (unmodeled device, draws 2kW)**
```
t=18:00:00  Dishwasher starts.
            SiteMeter.NetImport jumps: 6 + 7 + 2.5 = 15.5kW (instantaneous).

t=18:00:05  Monitor reads SiteMeter (next cycle).
            SITE_RESIDUAL.State.ActualPower = 15.5 - (6+7) = 2.5kW (was 0.5kW).
            Detects deviation: actual 15.5kW vs planned 13.5kW.
            → PlanTrigger.DEVICE_DEVIATION

            Penalty check:
              RollingAverage over last 15 minutes:
                14:45-18:00 readings: mostly 13.5kW, last reading 15.5kW.
                Average = (179 readings × 13.5 + 1 reading × 15.5) / 180 ≈ 13.51kW
              13.51kW < 15kW → NO BREACH (spike is only 1 reading out of 180)
              
            → UserNotification WARNING: "Instantaneous demand 15.5kW. Rolling average 13.5kW.
              Penalty threshold 15kW not yet breached."
```

**2. Planner replan (immediate)**
```
Phase 6: Penalty Threshold Check

  SITE_RESIDUAL forecast updated to 2.5kW (dishwasher running).
  Projected load: 6 + 7 + 2.5 = 15.5kW sustained.
  
  If this continues for 15 minutes (MeasurementWindow), rolling average will rise:
    After 5 min: avg ≈ (120×13.5 + 60×15.5) / 180 = 14.17kW (safe)
    After 10 min: avg ≈ (60×13.5 + 120×15.5) / 180 = 14.83kW (approaching)
    After 15 min: avg = 15.5kW → BREACH

  Time until breach: ~14 minutes at sustained 15.5kW.
  The Planner has ~14 minutes to act.

Option A: Accept breach. Cost = €100.
Option B: Reduce EV from 7kW to 5kW.
  New total: 6 + 5 + 2.5 = 13.5kW. Under threshold.
  Lost EV energy: 2kW × 5min = 0.167kWh per slot. Trivial.
  MarginalValue lost ≈ €0.06.
Option C: Reduce heat pump from 6kW to 3kW.
  New total: 3 + 7 + 2.5 = 12.5kW. Under threshold.
  Lost heating: 3kW × 5min = 0.25kWh. ComfortBid €0.20 → value lost €0.05.

Decision: Option B (reduce EV). €0.06 << €100.
```

**3. Dispatcher executes within 5 seconds**
```
DispatchCommand to EV: CommandedPower = 5kW (reduced from 7kW)
SiteMeter.NetImport drops to 13.5kW.

Rolling average evolution (dishwasher still running):
  t=18:00-18:01: some readings at 15.5kW (before correction), most at 13.5kW after
  t=18:01-18:15: stable at 13.5kW
  15-minute average never reaches 15kW → NO BREACH ✓

PenaltyRule state: BreachedThisPeriod = false (preserved).
CurrentPeakValue = max(13.8kW previous, 13.51kW current avg) = 13.8kW (unchanged).
```

**4. Dishwasher finishes (45 minutes later)**
```
SITE_RESIDUAL drops to 0.5kW.
Planner replan: EV returns to 7kW.
Total load: 6 + 7 + 0.5 = 13.5kW.
Normal operation resumes. Penalty avoided entirely.
```

### Scenario B: Sustained overload — penalty breached

**Same setup, but assume Planner fails to react in time (e.g. system was mid-replan
when dishwasher started, and ReplanCooldown delayed the response by 2 minutes).**

```
t=18:00:00  Dishwasher starts. SiteMeter = 15.5kW.
t=18:00:05  Monitor detects. PlanTrigger.DEVICE_DEVIATION emitted.
            But ReplanCooldown = 30s and last replan was 25s ago. Must wait 5s.

t=18:00:10  Planner replans. Decides to reduce EV. But...

t=18:00:10  Meanwhile, user also turns on electric oven (another unmodeled device, 3kW).
            SiteMeter.NetImport = 6 + 7 + 2.5 + 3 = 18.5kW.
            SITE_RESIDUAL = 18.5 - 13 = 5.5kW.

t=18:00:15  Dispatcher sends EV reduction to 5kW.
            SiteMeter = 6 + 5 + 5.5 = 16.5kW. Still above 15kW.
            Need more reduction.

t=18:00:20  Monitor: RollingAverage climbing. Emergency replan.
            Planner: reduce heat pump to 3kW AND reduce EV to 3kW.
            New total: 3 + 3 + 5.5 = 11.5kW. Under threshold.

t=18:00:25  Dispatcher executes. SiteMeter drops to 11.5kW.

But: the rolling average has been accumulating high readings for 25 seconds.
  Not enough to breach 15kW on a 15-minute average (only ~2.8% of window).
  RollingAverage ≈ 13.5 × 0.972 + 17.5 × 0.028 = 13.61kW. Still safe.
  → NO BREACH with PT15M window.

Now change the scenario: MeasurementWindow = PT1M (strict utility metering).

  With PT1M window:
    t=18:00:00 to 18:00:25: 25 seconds at 15.5-18.5kW average
    t=18:00:25 to 18:01:00: 35 seconds at 11.5kW
    1-minute average ≈ (25×17 + 35×11.5) / 60 = 13.78kW. Still under 15kW. Safe.

  But if the oven had started at 17:59:50 (10 seconds earlier):
    Full minute 17:59:50-18:00:50 at 16.5-18.5kW average.
    1-minute average ≈ 17.5kW > 15kW → BREACH.
    
    BreachedThisPeriod = true. BreachTimeStamp = 18:00:50.
    → UserNotification ALERT: "Peak demand breach. 1-min avg = 17.5kW > 15kW.
      €100 penalty incurred for this month."
    
    Algorithm response post-breach:
      €100 is sunk. But the system does NOT relax and run unchecked.
      Planner continues to enforce 15kW threshold as a soft constraint.
      Same actions as Scenario A: reduce EV to 5kW, keep total under 15kW.
      
      Difference from pre-breach: before breach, exceeding 15kW carried €100 cost
      (hard barrier, would sacrifice significant comfort to avoid). After breach,
      exceeding carries €0 additional penalty but the system still prefers to comply
      (soft constraint, will spend up to ~€5 in rescheduling to stay under).
      
      If oven stays on and total would be 16.5kW:
        Planner reduces EV to 3kW + heat pump to 3kW → total 3+3+5.5 = 11.5kW.
        Cost of reduction: minor comfort loss (EV charges slower, house cools slightly).
        This is cheap enough → accepted under soft threshold.
      
      If reduction would cost €20 in comfort (e.g. critical EV deadline):
        Soft threshold exceeded → accept the higher peak.
        But this is a deliberate trade-off, not blind relaxation.
        PlanWarning: "Post-breach: exceeding threshold in slot 18:15.
          Rescheduling too expensive (€20 comfort loss). Peak will be 16.5kW."

    Why this matters:
      - Higher peaks may affect next month's contract/subscription terms
      - User set 15kW because they want to stay under — intent persists after breach
      - Good behavior for the system: always try to comply, penalty is the cost of failure
        not a license to stop trying
```

### Key insight
```
MeasurementWindow is the critical parameter:
  PT15M (typical utility): brief spikes are absorbed. System has minutes to react.
  PT1M (strict):           system needs seconds to react. Harder to avoid breach.
  PT5S (near-instantaneous): any spike is a breach. Only preventive planning works.

The correct value depends on how the utility actually meters peak demand.
Configuration must match the real billing methodology.

Post-breach behavior:
  Penalty is sunk (€100 already lost). But the system keeps trying to stay under
  the threshold — it's a soft constraint, not a license to run unchecked.
  The Planner spends a small budget (e.g. €5) on rescheduling to comply,
  but won't sacrifice major comfort (€20+) for a preference that no longer
  carries a penalty. This balances user intent with practical cost.
```

### Entities exercised
```
PenaltyRule (PEAK_DEMAND_EXCEEDED), MeasurementWindow (PT15M, PT1M),
RollingAverage computation, Threshold vs rolling average (not instantaneous),
SITE_RESIDUAL spike (multiple unmodeled devices), Monitor rolling average tracking,
Phase 6 penalty avoidance (pre-breach: hard barrier, post-breach: soft constraint),
BreachedThisPeriod lifecycle, softThreshold for post-breach compliance,
reaction time budget (MeasurementWindow - response latency = time to avoid breach)
```

---

## UC-11: Consumption-Only Site (No PV, No Battery)

**Scenario:** Apartment with EV, heat pump, washing machine. No PV, no battery. Grid meter only.

### Preconditions
```
Assets:           EV (7kW), Heat pump (3/6kW STEPPED), Washing machine (2kW ON_OFF)
                  SITE_RESIDUAL (0.8kW learned profile)
No PV:            BaselineLoad always ≥ 0 (never net export)
No battery:       HasAutoFollowCapacity() = false
Rates:            standard TOU: peak €0.35, off-peak €0.12, shoulder €0.22
```

### Step-by-step

**1. Algorithm behavior without generation**
```
Phase 1: BaselineLoad = heat pump forecast + SITE_RESIDUAL forecast. Always positive.
  No negative baseline (no PV).
  
Phase 4 (Storage): empty loop. No storage assets. Zero iterations. Fine.

Phase 5 (PV Surplus): BaselineLoad never < 0. No surplus. Empty loop. Fine.

Phase 7 (Envelopes): FlexibilityEnvelopes still work.
  EV and washing machine have flexible demand windows.
  VTN can still influence scheduling via price signals.
```

**2. Core value: load shifting**
```
EV packet: charge 30kWh by 07:00, budget €4.00.
  Peak slots ineligible (EffectiveCost > ComfortBid at most fill levels).
  Off-peak slots preferred: 30kWh × €0.12 = €3.60.
  System shifts EV entirely to off-peak. Savings vs flat average: ~€2.40/day.

Washing machine: 4kWh, done by 18:00.
  Shoulder rate €0.22 available now (10:00).
  Off-peak not available until 20:00 (past deadline).
  Shoulder is cheapest available. Runs now.
```

**3. Deviation handling without auto-follow**
```
HasAutoFollowCapacity() = false. No buffer for deviations.
Monitor detects SITE_RESIDUAL spike (someone turns on oven: +3kW unplanned).
SiteMeter.NetImport jumps.

Without battery to absorb: deviation persists until next replan.
Planner re-evaluates: might reduce EV power if penalty threshold approached.
Otherwise: deviation is just a measurement fact, no corrective action possible.

System operates more conservatively:
  PlanWarning INFO: "No auto-follow assets. Deviation absorption limited.
  Consider wider capacity margins."
```

**4. FREE mode handling**
```
User requests: "charge EV with free energy only" (OPPORTUNISTIC mode).
User Req Mgr checks: any production assets? No PV, no generator.
  
UserNotification WARNING: "No energy production assets registered.
  Free-energy-only modes will not receive energy. Consider cost-aware mode."

If user insists: EnergyPacket created with OPPORTUNISTIC mode.
  ComfortBid at all fill levels = €0.00 (free energy only).
  EffectiveCost of every slot > €0.00 (no free energy exists).
  → Zero eligible slots. Packet stays PENDING indefinitely.
  
After 24h of no progress:
  PlanWarning WARNING: "EV packet has no eligible slots. No free energy available."
  UserNotification: "EV charge stalled. No free energy available.
  Change to cost-aware mode to enable charging."
```

### Entities exercised
```
Algorithm with zero storage/production assets (Phases 4/5 empty), SITE_RESIDUAL
as only baseline (no PV offset), HasAutoFollowCapacity() = false,
FREE mode warning for no-production sites, load shifting as primary value
```

---

## UC-12: VTN Communication Loss

**Scenario:** Network connection to VTN drops at 14:00. System operates on stale data.

### Preconditions
```
Time:             14:00
Last VTN contact:  14:00 (rates valid until 00:00 tonight, day-ahead from earlier)
Active packets:   EV (charge tonight), battery (discharge 17:00-20:00)
Polling interval:  60s
```

### Step-by-step

**1. OpenADR IF detects timeout**
```
14:01: VTN poll fails (HTTP timeout).
14:02: Retry fails.
14:03: Third failure → VTN connection marked as lost.
  OadrEventCache entries: still valid (events have explicit end times)
  PlannedRates: valid until 00:00 (day-ahead data already received)
  
  No PlanTrigger emitted (rates haven't changed).
  UserNotification INFO: "VTN connection lost. Operating on cached rates."
```

**2. System continues with cached data**
```
14:00-00:00: rates are known (cached). Planner operates normally.
  EV charges off-peak as planned.
  Battery discharges at peak as planned.
  All normal. No degradation.

But: no new events can arrive. VTN cannot:
  - Send updated prices for tomorrow
  - Issue emergency alerts
  - Update capacity limits
  - Request capacity reservations
```

**3. 00:00 — Cached rates expire**
```
No rates available for tomorrow. Planner enters degraded mode.
Behavior determined by VenController.StaleRatePolicy configuration (§1.10.1):

Phase 1: for slots beyond 00:00:
  RateSnapshot missing. Planner checks StaleRatePolicy:

  LAST_KNOWN (Option A):
    Use the last known rate (€0.12 off-peak) for all future slots.
    Simple. Risk: actual rates might differ. But at least the system keeps running.
    EV continues charging at assumed €0.12. If actual rate is higher when VTN
    reconnects, AccumulatedCost may exceed estimates.

  SAFE_AVERAGE (Option B):
    Compute a conservative rate from PastRates history:
      e.g. 80th percentile by time-of-day from last 30 days.
      Night slots: €0.14 (safety margin above typical €0.12).
      Day slots: €0.28 (below peak but above average).
    Budget checks use conservative rate → less risk of overspend.
    Packets with tight budgets may self-limit (fewer eligible slots).

  DEFER_TO_FLEXIBLE (Option C):
    All slots beyond 00:00 become FLEXIBLE regardless of near-horizon.
    Envelopes only. Wait for VTN reconnection before committing.
    Most conservative. Risk: if VTN stays down for hours, packets stall.
    Near-horizon override still applies: when slots reach now + NearHorizonDuration,
    they MUST become FIRM (can't defer indefinitely). Falls back to LAST_KNOWN
    for those slots.

  HEURISTIC_FORECAST (Option D, default):
    Use learned rate heuristics from PastRates history:
      For each slot, compute expected rate from:
        - DayOfWeek pattern (weekday vs weekend)
        - TimeOfDay pattern (peak/off-peak/shoulder)
        - SeasonalFactor (summer vs winter pricing)
    Same approach as SITE_RESIDUAL heuristics but applied to rates.
    Fresh install: flat default rate (e.g. national average €0.20/kWh).
    After 2 weeks: reasonable day/night pattern.
    After 3 months: seasonal calibration.
    
    This is the best option for sustained outages: the system plans normally
    using its best rate prediction, improving over time. Estimations are honest
    (marked as ForecastSource=HEURISTIC) and will be corrected on reconnection.

Planner adds PlanWarning:
  WARNING: "Rates beyond 00:00 are estimated (policy: [StaleRatePolicy]).
  VTN connection lost. Estimates may differ from actual rates."

UserNotification: "VTN offline. Using [estimated/historical/deferred] rates.
  Costs may differ from actual when connection is restored."
```

**4. 00:30 — VTN reconnects**
```
OpenADR IF: VTN poll succeeds.
  VTN sends batch of events accumulated during outage:
    - Price events for tomorrow
    - Updated capacity limits
    - Possibly a missed alert (if event.modificationDateTime is in the past)

  For missed alerts:
    If event is still active (endTime > now): process normally.
    If event has expired: log for reporting, no action.
  
  PlannedRates updated with fresh data.
  → PlanTrigger.RATE_CHANGE
  Normal operation resumes.

UserNotification: "VTN connection restored. Rates updated."
```

**5. Reporting obligations during outage**
```
OadrReportObligation[]:
  Reports due during outage could not be sent.
  OpenADR IF queues them: reportID, payload, originalDueTime.
  
  On reconnection: send queued reports with original timestamps.
  VTN accepts late reports (OpenADR 3.1 allows delayed report delivery).
  
  If queue grows too large (days-long outage):
    Aggregate reports per reporting interval.
    Send summary rather than individual measurements.
```

### Entities exercised
```
VTN connection state, OadrEventCache validity, cached PlannedRates,
StaleRatePolicy (LAST_KNOWN / SAFE_AVERAGE / DEFER_TO_FLEXIBLE / HEURISTIC_FORECAST),
degraded mode planning with configurable behavior, rate heuristic learning,
report queue and late delivery, PlanWarning for stale data,
UserNotification for connectivity state, reconnection batch processing
```

---

## UC-13: VTN Direct Override (DISPATCH_SETPOINT)

**Scenario:** VTN sends DISPATCH_SETPOINT commanding heat pump to 2kW for 30 minutes.

### Preconditions
```
Time:             14:00
Heat pump:        running at 6kW (STEPPED: 0/3/6), active EnergyPacket "maintain 21°C"
Battery:          idle
```

### Step-by-step

**1. OpenADR IF receives DISPATCH_SETPOINT**
```
Event: DISPATCH_SETPOINT
  Target resource: heat_pump_01
  Payload: 2kW for PT30M (14:00-14:30)

OpenADR IF: this is a direct override — bypass Planner, send to Dispatcher.
```

**2. Dispatcher creates override session**
```
Existing heat pump DeviceSession: PAUSED (override takes priority).
  EnergyPacket status stays ACTIVE (override is temporary, packet resumes after).

Override DeviceSession created:
  SourcePacketID = null (no packet — VTN direct control)
  Reason = "DISPATCH_SETPOINT from VTN"

DispatchCommand to heat pump: CommandedPower = 2kW (nearest STEPPED level = 3kW)
  → Heat pump can't do 2kW (STEPPED: 0/3/6). Closest valid = 3kW.
  → If VTN tolerance allows: send 3kW, report 3kW in compliance report.
  → If strict compliance required: send 0kW (undershoot rather than overshoot).

Planner creates synthetic EnergyPacket for override tracking:
  TargetEnergy = 3kW × 0.5h = 1.5kWh
  Deadline = 14:30 (event end)
  CompletionPolicy = STOP
  → PlanTrigger.CAPACITY_CHANGE (asset capacity reduced for 30 min)
```

**3. Monitor tracks compliance**
```
For each cycle during override:
  ActualPower should be ≈ 3kW.
  PenaltyRule for EVENT_NONCOMPLIANCE:
    Threshold = 2kW ± TolerancePercent (e.g. 10% → 1.8-2.2kW range)
    Actual 3kW > 2.2kW → technically non-compliant.
    → PlanWarning: "DISPATCH_SETPOINT requested 2kW, nearest step is 3kW.
      Non-compliance risk: 50% overshoot."
    → UserNotification with option to manually turn off heat pump (0kW).
```

**4. 14:30 — Override expires**
```
Override DeviceSession closed. Heat pump packet resumes.
Normal planning cycle: heat pump returns to planned power level.
Replan accounts for 30-min gap in heating → may increase power temporarily.

Temperature may have dropped during override. ThermalModelParams recomputes
TargetEnergy: needs more energy now to reach target temperature.
```

### Entities exercised
```
DISPATCH_SETPOINT event, Dispatcher override session, STEPPED device compliance,
PAUSED packet during override, EVENT_NONCOMPLIANCE penalty check,
ThermalModelParams (temperature impact of override), PlanTrigger.CAPACITY_CHANGE
```

---

## UC-14: Thermal Feedback Loop (Heat Pump Temperature Drop)

**Scenario:** Heat pump maintains 21°C. Outdoor temp drops from 5°C to -2°C. System adapts.

### Preconditions
```
Time:             08:00
Heat pump:        3kW, maintaining 21°C, ThermalModelParams configured
                  ThermalMass = 2.5 kWh/K, InsulationFactor = 0.15 kW/K
Outdoor temp:     5°C at 08:00, forecast: dropping to -2°C by 14:00
Indoor temp:      21°C (at target)
EnergyPacket:     "maintain 21°C until 22:00", CompletionPolicy = STOP
                  Initial TargetEnergy: 18 kWh (computed at request time with outdoor 5°C)
```

### Step-by-step

**1. 08:00 — Initial plan**
```
ThermalModelParams computation:
  Target = 21°C, Current = 21°C (at target, no static energy needed)
  Heat loss per slot (at outdoor 5°C):
    lossRate = InsulationFactor × (21 - 5) = 0.15 × 16 = 2.4 kW
  Heat pump must deliver 2.4kW continuously to maintain temperature.
  Over 14h (08:00-22:00): 2.4 × 14 = 33.6 kWh / Efficiency 0.92 = 36.5 kWh

  Wait — outdoor temp is DROPPING. Loss rate increases over the day:
    08:00 (5°C): loss = 0.15 × 16 = 2.4 kW → needs 2.4kW
    11:00 (1°C): loss = 0.15 × 20 = 3.0 kW → needs 3.0kW
    14:00 (-2°C): loss = 0.15 × 23 = 3.45 kW → needs 3.45kW

  Total TargetEnergy over horizon: Σ(lossRate[slot] × dt) / Efficiency
    = ~42 kWh (more than initial estimate because outdoor temp drops)

  Planner allocates: heat pump at 3kW (STEPPED, closest step above 2.4kW) for morning,
  must switch to 6kW in afternoon when loss exceeds 3kW.
```

**2. 11:00 — Outdoor temp drops to 1°C (forecast confirmed)**
```
ExternalDataSource (weather) updates outdoor temp forecast.
Planner replan (PERIODIC):
  ThermalModelParams recomputes TargetEnergy:
    Remaining energy = loss from 11:00-22:00 with dropping outdoor temp
    At 3kW: heat pump can't keep up after 14:00 (loss > 3kW at outdoor -2°C).
    Must increase to 6kW starting at ~13:00.

  Phase 3: allocates 6kW slots from 13:00 onward.
  Budget impact: 6kW × 9h = 54 kWh at afternoon rates.
  
  UserNotification INFO: "Outdoor temperature dropping. Heat pump increasing to 6kW
  from 13:00. Estimated cost increase: €2.40."
```

**3. 14:00 — Outdoor temp reaches -2°C, heat pump at 6kW**
```
Heat loss: 3.45kW. Heat pump delivers 6kW (STEPPED, can't do 3.45).
  Excess: 6 - 3.45 = 2.55kW → indoor temp rises slightly above 21°C.
  This is acceptable. STEPPED assets overshoot is expected.

Monitor: no deviation. Planned 6kW, actual 6kW. ✓
Temperature: 21.3°C (slight overshoot). Fine.

If heat pump had only 3kW step:
  Delivers 3kW, needs 3.45kW. Deficit = 0.45kW.
  Indoor temp drops: 0.45kW / 2.5 kWh/K × 1h = 0.18°C/h.
  By 22:00: 21 - (0.18 × 8) = 19.6°C. Below target.
  EstimatedCompletion < 1.0 → PlanWarning: "Heat pump cannot maintain 21°C
  at current outdoor temperature. Expected: 19.6°C at end of period."
  CompletionPolicy = STOP → PARTIAL_COMPLETED at 22:00.
```

**4. 18:00 — Weather forecast update: overnight stays -2°C**
```
But the packet deadline is 22:00 (STOP). After 22:00, heating stops.
UserNotification: "Heating stops at 22:00 as scheduled. Indoor temp will drop
overnight. Current outdoor: -2°C."

User could create a new request: "maintain 18°C overnight" with low budget.
Or accept the temperature drop.
```

### Key insight
```
Thermal assets differ from electrical storage:
  - "Fill percentage" is temperature, not SoC
  - Target energy changes with external conditions (weather)
  - TargetEnergy must be RECOMPUTED each plan cycle
  - STEPPED assets cause overshoot/undershoot (inherent to discrete steps)
  - Heat loss is continuous and varies with outdoor temp
  - The feedback loop is: temp forecast → energy need → schedule → dispatch → temp change
```

### Entities exercised
```
ThermalModelParams (energy computation from temperature), ExternalDataSource (weather),
AssetForecast recomputation based on changing conditions, STEPPED overshoot/undershoot,
TargetEnergy recomputation each plan cycle, PARTIAL_COMPLETED for thermal assets,
UserNotification for cost impact of weather changes
```

---

## Summary: Entity Coverage

Every major entity from Step 1 is exercised by at least one use case:

| Entity | Use Cases |
|---|---|
| EnergyPacket (full lifecycle) | UC-01, UC-02, UC-08, UC-09, UC-14 |
| FlexibilityEnvelope | UC-01, UC-04, UC-05, UC-07 |
| ValueCurve / DeadlineTier / ComfortRate | UC-01, UC-02, UC-09 |
| CompletionPolicy (STOP/CONTINUE) | UC-02, UC-06, UC-09, UC-14 |
| PenaltyRule / PenaltyThreshold | UC-10, UC-13 |
| MeasurementWindow / RollingAverage | UC-10 (Scenario A + B) |
| SiteMeter / SITE_RESIDUAL | UC-03, UC-08, UC-10, UC-11 |
| SiteMeter (authoritative) | UC-03, UC-10 |
| AssetState (IsConnected, Responsiveness) | UC-08 |
| ThermalModelParams | UC-13, UC-14 |
| DeviceSession | UC-01, UC-02, UC-06, UC-08, UC-13 |
| Plan (FIRM + FLEXIBLE) | UC-01, UC-04, UC-05 |
| PacketAllocation (SurplusPower/GridPower) | UC-01, UC-02, UC-03, UC-10 |
| PlanWarning | UC-01, UC-09, UC-11, UC-12, UC-13, UC-14 |
| UserNotification | UC-01, UC-04, UC-08, UC-09, UC-11, UC-12, UC-14 |
| RateSnapshot / PlannedRates | UC-04, UC-05, UC-12 |
| StaleRatePolicy | UC-12 |
| OadrCapacityState / CapacityRequest | UC-07 |
| OadrEventCache | UC-06, UC-12 |
| DISPATCH_SETPOINT | UC-13 |
| PlanTrigger (all types) | UC-01 (USER_REQUEST), UC-04 (RATE_CHANGE), UC-06 (ALERT), UC-07 (CAPACITY_CHANGE), UC-08 (ASSET_STATE_CHANGE), UC-10 (DEVICE_DEVIATION), UC-12 (PERIODIC fallback), UC-13 (CAPACITY_CHANGE) |
| AssetForecast / Heuristics / AvailabilityWindows | UC-03 (PV), UC-10 (SITE_RESIDUAL spike), UC-11 (no PV), UC-14 (weather) |
| ExternalDataSource (weather) | UC-14 |
| Phase 4 (Storage) | UC-03 |
| Phase 5 (PV Residual Surplus) | UC-03 |
| Phase 6 (Penalty Check) | UC-10 |
| Phase 7 (Envelopes) | UC-01, UC-04, UC-05, UC-07 |
| Early firm-up heuristic | UC-01 |
| Batch risk check | UC-02 |
| Tier fallback (time + budget constrained) | UC-09 |
| Post-breach soft constraint | UC-10 (Scenario B) |
| STEPPED asset behavior | UC-10, UC-13, UC-14 |

### Algorithm Phase Coverage

| Phase | Normal | VTN | Edge | Stress |
|---|---|---|---|---|
| Phase 1 (Prepare) | UC-01, UC-02, UC-03 | UC-05 | UC-09 | UC-11, UC-12 |
| Phase 2 (Score) | UC-01, UC-02 | | UC-09 | UC-11 |
| Phase 3 (Allocate) | UC-01, UC-02, UC-03 | UC-06 | UC-10 | UC-11 |
| Phase 4 (Storage) | UC-03 | | | UC-11 (empty) |
| Phase 5 (PV Surplus) | UC-03 | | | UC-11 (empty) |
| Phase 6 (Penalty) | | | UC-10 | |
| Phase 7 (Envelopes) | UC-01, UC-02 | UC-04, UC-05, UC-07 | | UC-12 |
| Phase 8 (Finalize) | UC-01, UC-02 | | UC-09 | |

---

*End of Step 5 (Draft 2). Changes from Draft 1:*
- *UC-01: fixed "ByTonight 07:00" → concrete timestamp "2025-03-15T07:00" with note that natural language deadlines are a UI concern above the entity model*
- *UC-01 step 8: PostDeadlineComfortBid €0.02 < ExportPrice €0.08 → PV surplus also ineligible. Noted design implication.*
- *UC-02: surplus-aware EffectiveCost (ExportPrice opportunity cost, not €0)*
- *UC-03: PV surplus cascade uses surplus-aware Phase 3/4, not old Phase 5 self-consume logic*
- *UC-09: completely rewritten. Tier motivation inverted: highest budget (€5) for soonest deadline (tonight), lowest budget (€1) for relaxed deadline (Friday). Failure mode for Tier 0 is TIME, Tier 1 is MONEY. Shows both tiers failing and CONTINUE as fallback.*
- *UC-10: completely rewritten. Added MeasurementWindow (PT15M) to PenaltyRule. Two clear scenarios: Scenario A (brief spike, rolling average absorbs, penalty avoided) and Scenario B (sustained overload with PT1M strict metering, breach occurs, post-breach soft constraint continues enforcement).*
- *All UCs: concrete RFC 3339 timestamps in UserDeadline, no natural language keys*
- *Updated entity coverage table: MeasurementWindow/RollingAverage, PacketAllocation surplus split, tier fallback, post-breach soft constraint*

*Proceed to Step 6 (Validation & Issue Identification) after review.*
