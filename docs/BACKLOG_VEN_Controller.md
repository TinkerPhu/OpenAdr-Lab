# VEN Controller: Code vs. Documentation Fulfilment Analysis

**Date:** 2026-03-22
**Source docs:** `docs/archive/VEN_Controller/` (Steps 1–5, VEN-Assets, VEN-Flexibility, ImplementationPlan)
**Source code:** `VEN/src/controller/`, `VEN/src/entities/`

---

## Overview

The VEN Controller documentation describes a sophisticated 8-phase greedy scheduler for a residential HEMS. The implementation covers the core structure but has significant simplifications and missing features relative to the design. The entity model is ~95% structurally complete (all types/enums defined), but the algorithmic implementation is roughly 50–60% of what the spec describes.

---

## 1. Entity Model (Step 1)

| Topic | Status | Notes |
|---|---|---|
| Enumerations (S1.1–1.12) | DONE | All enums in `entities/asset.rs`: `AssetType`, `PowerAdjustability`, `DeviceResponsiveness`, `PacketStatus`, `PlanTrigger`, `CompletionPolicy`, `UserRequestMode`, `StaleRatePolicy`, `ForecastSource`, `RateType`, `RateUnit`, `FlexibilityDirection`, `PenaltyCondition`, `ExternalDataSourceType` |
| ComfortRate (S2.7) | DONE | `fill`, `max_marginal_price`, `max_marginal_co2` |
| DeadlineTier (S2.8) | DONE | `deadline`, `max_total_cost_eur` (Option), `max_marginal_rate_eur_kwh` (Option), `min_completion` |
| ValueCurve (S2.9) | DONE | `comfort_rates`, `deadline_tiers`, `active_tier_index` + `bid_at()` interpolation |
| EnergyPacket (S4.1) | DONE | All key fields: `id`, `asset_id`, `status`, temporal bounds, energy target, value curve, profiles, budget tracking, planner estimates |
| PacketAllocation (S6.3) | DONE | `packet_id`, `asset_id`, `power_kw`, `surplus_power_kw`, `grid_power_kw`, `marginal_value`, `cost_eur`, `co2_g` |
| PlanTimeSlot (S6.2) | DONE | All fields including `grid_effective_cost`, `rate_estimated`, `surplus_available_kw`, flexibility |
| Plan (S6.10) | DONE | Two-layer structure (FIRM + FLEXIBLE), horizons, summaries, warnings, envelopes |
| FlexibilityEnvelope (S6.9) | DONE | All fields present |
| AssetProfile (S3.1) | DONE | All fields per spec |
| AssetState (S3.2) | DONE | All fields per spec |
| AssetHeuristics (S3.3) | DONE | `daytime_profile_kw`, `weekday_weights`, `seasonal_factor` |
| AssetForecast (S3.6) | DONE | Including `availability_windows` |
| AssetLedger (S3.7) | DONE | Full accounting fields |
| AssetFlexibility (S3.5) | DONE | 4-axis flexibility model |
| SiteMeter (S3.9) | DONE | Net import, cumulative energy, etc. |
| PenaltyRule (S6.6) | DONE | Full threshold + breach tracking fields |
| DeviceSession (S4.2) | DONE | Session tracking fields |
| DispatchCommand / DispatchState | DONE | In `site_meter.rs` |
| OadrProgramConfig (S5.1) | DONE | In `capacity.rs` |
| OadrEventCache (S5.2) | DONE | In `capacity.rs` |
| OadrReportObligation (S5.3) | DONE | In `capacity.rs` |
| OadrCapacityState (S5.4) | DONE | In `capacity.rs` |

**Entity model verdict: ~95% structurally complete.** All structs and enums from Step 1 exist. However, many are "shelf-ware" — defined but never instantiated or used in the controller logic (see Section 9).

---

## 2. Architecture (Step 2) — Six Components

| Component | Status | Notes |
|---|---|---|
| OpenADR Interface | DONE | `openadr_interface.rs` (822 lines): `parse_rate_snapshots`, `parse_capacity_state`, `extract_report_obligations`, looping event support |
| Planner | DONE | `planner.rs` (842 lines): 8-phase greedy scheduler |
| Dispatcher | DONE | `dispatcher.rs` (132 lines): `build_setpoints`, `update_packets` |
| Monitor | DONE | `monitor.rs` (147 lines): `record_tick` — ledger + packet transitions |
| User Request Manager | DONE | `user_request.rs` (202 lines): `create_from_body` with validation |
| Reporter | DONE | `reporter.rs` (281 lines): measurement + status reports |
| Asset Controller | MISSING | No dedicated module. The simulator (`VEN/src/simulator/`) acts as proxy for physical device control. No real device protocol integration. |
| Controller Trace | DONE | `trace.rs` (287 lines): `ControllerEvent` enum, `ControllerTrace`, `AssetHistoryBuffer` |
| Timeline | DONE | `timeline.rs` (720 lines): uniform grid resampling, past+future merging |

---

## 3. Planning Algorithm (Step 4) — Phase-by-Phase Analysis

### 3.1 Phase 1: Prepare (Build Planning Grid)

| Requirement | Status | Detail |
|---|---|---|
| Build grid from horizon | DONE | `build_grid()` creates slots from `now` to `horizon_end` |
| FIRM/FLEXIBLE classification by `NearHorizonDuration` | DONE | `slot.end <= firm_boundary -> Firm, else Flexible` |
| **Per-packet urgency override** (TimePressure >= 2.0 forces FLEXIBLE->FIRM) | MISSING | Code uses a static clock boundary only. High-urgency packets beyond the near-horizon do NOT force slots to FIRM. A battery with deadline in 1 hour but beyond near-horizon won't get firm-scheduled. |
| Populate slot rates from tariffs | DONE | Resampled tariff time-series mapped to slots with TWM averaging |
| StaleRatePolicy (S1.10.1) for missing rates | PARTIAL | `rate_estimated = rates_empty` flag is set. But all 4 policies (`LAST_KNOWN`, `HEURISTIC_FORECAST`, `DEFER_TO_FLEXIBLE`, `SAFE_AVERAGE`) are NOT implemented — just falls back to hardcoded defaults. |
| Populate baseline from forecasts | DONE | PV forecast resampled and applied to compute `surplus_available_kw` |
| **SITE_RESIDUAL forecast** (S3.8) | MISSING | Baseline uses a flat `profile.base_load_kw()` — no learned heuristic residual, no derivation from SiteMeter minus known assets |
| **Thermal TargetEnergy recomputation** (S4 Phase 1 step 4a) | MISSING | No thermal model integration — heater/heat pump packets keep their initial target energy regardless of outdoor temp changes |
| **Asset forecasts for non-PV assets** (heater, cooking stove, EV availability) | PARTIAL | Only PV forecast is consumed (`forecast_maps.get("pv")`). Other asset forecasts are in the HashMap but not applied to baseline. |
| Classify packets (consumption/storage/export) | IMPLICIT | No explicit classification; battery handled separately in Phase 4 |
| **Asset availability per slot** (EV disconnection windows) | MISSING | No `AvailabilityWindows` check. An EV that's forecast to disconnect is still scheduled. |
| **Early firm-up heuristic** (S5.1 — flat rate variance check) | MISSING | No rate variance analysis on FLEXIBLE slots; no per-packet FLEXIBLE->FIRM promotion |

### 3.2 Phase 2: Score

| Requirement | Status | Detail |
|---|---|---|
| Build CalcCache per (packet, FIRM slot) | SIMPLIFIED | Uses `AllocEntry` instead of full `CalcCache`. Missing many fields. The `CalcCache` struct exists in `plan.rs` but is never instantiated. |
| **ComfortBid interpolation** at projected fill | DONE | `packet.value_curve.bid_at(fill)` with correct linear interpolation |
| **TimePressure computation** (S4.2) | DEVIATED | Doc: discrete levels (3.0, 2.0, 1.5, 1.0) based on `TimeSlack`. Code: continuous `(slots_needed / slots_remaining).clamp(1.0, 3.0)`. Close but not exactly spec. |
| **Surplus-aware EffectiveCost** (S6) | DONE | `eff_cost = import * (1-surplus_frac) + export * surplus_frac` — correct blending |
| **Budget gate** (MaxTotalCost check) | MISSING | No `WithinBudget` check. Packets with budget constraints can be scheduled past their budget. |
| **MaxMarginalRate gate** | MISSING | `DeadlineTier.max_marginal_rate_eur_kwh` is stored but never checked in eligibility |
| **CO2 gate** (`WithinCO2: CO2Rate <= MaxMarginalCO2`) | MISSING | `max_marginal_co2` is on `ComfortRate` but never used in eligibility |
| **Eligibility override for urgent packets** | DONE | `eligible = comfort_bid >= eff_cost \|\| time_pressure >= 2.0` — matches spec intent |
| **Forward projection** (tentative energy tracking for fill update) | DONE | `allocated[packet.id]` tracks planned energy, adjusting `fill` per slot |
| **Surplus tentative tracking across packets** | MISSING | `surplusTentativelyClaimed` from spec not implemented — surplus pool isn't reduced across packets during scoring |
| **Post-deadline CONTINUE handling** | MISSING | No special `PostDeadlineComfortBid` path. Post-deadline packets with `CompletionPolicy::Continue` are simply not scored (filtered by `slot.start >= latest_end`). |

### 3.3 Phase 3: Allocate Consumption

| Requirement | Status | Detail |
|---|---|---|
| Sort candidates | **BUG** | Doc: sort by `EffectiveCost ASC`, then `MarginalValue DESC`. Code (`planner.rs:286-291`): sort by `MarginalValue DESC` only. This means the planner **does not minimize cost** — it prioritizes urgency over cheapness. |
| Greedy allocation with capacity tracking | DONE | `slot.surplus_available_kw -= surplus_used; slot.net_import_kw += grid_used` |
| **Asset sharing / slot commitment** | MISSING | `assetSlotCommitment[A,S]` from spec not tracked. Two packets on the same asset in the same slot would both be allocated. |
| **STEPPED asset handling** (S7.2) | MISSING | No discrete power step logic. All assets treated as stepless. |
| **Final budget re-check** with actual allocation | MISSING | No budget-respecting reduction of power. |
| Surplus/grid split tracking | DONE | Correctly splits power into `surplus_used` and `grid_used` |
| PacketAllocation recording | DONE | Full `PacketAllocation` struct written per allocation |
| Pending->Scheduled transition | DONE | `PacketStatus::Pending -> Scheduled` on allocation |

### 3.4 Phase 4: Allocate Storage (Battery Arbitrage)

| Requirement | Status | Detail |
|---|---|---|
| Identify profitable charge/discharge pairs | SIMPLIFIED | Doc: quartile-based cheap/expensive identification with round-trip efficiency test. Code: median-based threshold (`price < median * sqrt(eff)` for charge, `price > median / sqrt(eff)` for discharge). Simpler but reasonable. |
| **Charge from post-Phase-3 surplus pool** | DONE | `surp_kwh = slot.surplus_available_kw * slot_h` uses the already-depleted surplus |
| SoC bounds (min/max) | DONE | `soc = (soc + charge * eff / cap).min(1.0)` and `min_soc` respected |
| **Sequential SoC tracking** | DONE | `soc` variable carried forward slot-by-slot |
| **Interaction with consumption packets** (S8.3) | MISSING | No post-Phase-3 reallocation. Battery charges/discharges independently; doesn't detect "consumption packet on expensive slot that could be served by battery". |
| **Separate charge/discharge packets** | MISSING | Battery uses synthetic `Uuid::nil()` allocations, not separate `EnergyPacket` instances for charge vs discharge as spec requires. |

### 3.5 Phase 5: Residual PV Surplus

| Requirement | Status | Detail |
|---|---|---|
| Export unclaimed surplus | IMPLICIT | `slot.net_export_kw` tracks residual but no explicit export allocation or curtailment |
| **Export capacity limit curtailment** | MISSING | No check against `ExportCapacityLimit` for PV curtailment |
| **PlanWarning for curtailment** | MISSING | No warnings generated |

### 3.6 Phase 6: Penalty Threshold Check

| Requirement | Status | Detail |
|---|---|---|
| Check planned peak vs penalty thresholds | **NOT IMPLEMENTED** | Comment in code: "deferred to Stage 4" (planner.rs:77). No penalty avoidance logic exists anywhere in the controller. `PenaltyRule` struct is defined but never used. |
| Breach/no-breach decision logic | MISSING | |
| Rescheduling lowest-value allocations | MISSING | |

### 3.7 Phase 7: Flexibility Envelopes

| Requirement | Status | Detail |
|---|---|---|
| Build envelopes for packets with unallocated FLEXIBLE energy | DONE | `build_envelopes()` correctly computes remaining energy, eligible windows, rate ranges |
| **Budget remaining computation** | STUB | `budget_remaining_eur: f64::MAX` — always infinite, never computed from tier costs |
| **Rate eligibility filter** (only slots where `GridEffectiveCost <= ComfortBid`) | MISSING | All FLEXIBLE slots included regardless of cost |
| Min/max acceptable rate | DONE | `bid_at(fill_now)` and `bid_at(fill_after)` |

### 3.8 Phase 8: Finalize

| Requirement | Status | Detail |
|---|---|---|
| Update packet estimates | DONE | `estimated_cost_eur`, `estimated_co2_g`, `estimated_completion` computed from FIRM allocations |
| Compute slot flexibility | DONE | `import_flexibility_kw = cap - net_import` |
| Firm/flexible summaries | DONE | `summarize_firm()` and `FlexibleSummary` |
| **Write PlannedPowerProfile** per packet | MISSING | `planned_power_profile` on `EnergyPacket` is never populated by the planner |
| **Detect and emit PlanWarnings** | MISSING | `warnings: vec![]` — always empty. No tier fallback, capacity warnings, or completion estimates |

---

## 4. Dispatcher Analysis

| Requirement | Status | Detail |
|---|---|---|
| Read FIRM slot for `now`, extract allocations | DONE | `build_setpoints()` finds current FIRM or FLEXIBLE slot |
| Map allocations to per-asset setpoints | DONE | Defaults + slot allocation overlay |
| Enforce export capacity limit on PV | DONE | `if *pv_sp < -export_cap { *pv_sp = -export_cap }` |
| **DeviceSession management** | MISSING | No session tracking. `DeviceSession` struct exists but is never used |
| **DispatchCommand / DispatchState** | MISSING | Structs exist but are never used |
| `update_packets` — packet lifecycle transitions | DONE | Scheduled->Active, Active->Completed, deadline->PartialCompleted |

---

## 5. Monitor Analysis

| Requirement | Status | Detail |
|---|---|---|
| Per-asset energy ledger | DONE | `record_tick` updates `AssetLedgerEntry` with energy, cost, CO2 |
| Packet transition events | DONE | `ControllerEvent::PacketTransition` emitted on status changes |
| **Deviation detection** (\|actual - planned\| > threshold -> replan) | MISSING | No comparison of actual vs planned power. No `DeviceDeviation` trigger from monitoring. |
| **Penalty threshold monitoring** (rolling average, breach detection) | MISSING | |
| **SITE_RESIDUAL computation** (SiteMeter - sum of assets) | MISSING | |

---

## 6. Reporter Analysis

| Requirement | Status | Detail |
|---|---|---|
| TELEMETRY_USAGE measurement report | DONE | `build_measurement_report()` — net site import, OPERATING_STATE, STORAGE_CHARGE_LEVEL |
| **TELEMETRY_STATUS report** (event-driven) | DONE | `build_status_report()` exists |
| **USAGE_FORECAST report** | MISSING | No forecast report generation |
| **CAPACITY_RESERVATION report** | MISSING | No flexibility/capacity request reporting to VTN |
| **Report obligation tracking** (DueAt, Fulfilled) | MISSING | `OadrReportObligation` struct exists but fulfillment tracking is not wired |

---

## 7. User Request Manager Analysis

| Requirement | Status | Detail |
|---|---|---|
| POST /requests -> (UserRequest, EnergyPacket) | DONE | `create_from_body()` with full validation |
| Multi-tier deadlines | DONE | Multiple `DeadlineTier` entries supported |
| SoC -> energy target resolution | DONE | Per-asset `resolve_request_target()` |
| Default comfort rates per asset type | DONE | `entry.state.default_comfort_rates()` |
| Default completion policy per asset type | DONE | `entry.state.default_completion_policy()` |
| **Tier fallback** (when Tier N fails -> try Tier N+1) | MISSING | `active_tier_index` is stored but never advanced by the planner |
| **User notifications** (plan changes, warnings) | MISSING | `UserNotificationSeverity` enum exists but no notification system |

---

## 8. Data Flow (Step 3) — Lifecycle Compliance

| Flow | Status | Notes |
|---|---|---|
| VTN -> rates -> planner | DONE | `parse_rate_snapshots` feeds `TariffTimeSeries` to planner |
| User -> request -> packet -> planner | DONE | POST /requests -> EnergyPacket -> plan cycle |
| Plan -> dispatcher -> sim setpoints | DONE | `build_setpoints()` reads plan slots |
| Sim tick -> monitor -> ledger + packet transitions | DONE | `record_tick()` |
| Monitor -> PlanTrigger -> replan | PARTIAL | `DeviceDeviation` trigger returned but actual deviation detection not wired |
| VTN events -> capacity state -> planner | DONE | `parse_capacity_state` feeds `OadrCapacityState` |
| Plan -> reporter -> VTN reports | DONE | Measurement reports built from asset history |
| **PastRates archival** (planned->past as time passes) | MISSING | No rate history management |
| **AssetHeuristics learning** (daily pattern updates) | MISSING | |
| **ExternalDataSource refresh** (weather, irradiation) | MISSING | |

---

## 9. Dead Code — Defined But Never Used

The following structs/enums are defined in `VEN/src/entities/` but never instantiated or referenced by any controller logic:

| Type | File | Notes |
|---|---|---|
| `PenaltyRule` | `asset.rs:338` | Phase 6 not implemented |
| `PenaltyThreshold` | `asset.rs:322` | Phase 6 not implemented |
| `PenaltyCondition` | `asset.rs:148` | Phase 6 not implemented |
| `DeviceSession` | `site_meter.rs:89` | No session tracking in dispatcher |
| `DispatchCommand` | `site_meter.rs:10` | Dispatcher uses setpoint maps instead |
| `DispatchState` | `site_meter.rs:65` | Not used |
| `AssetHeuristics` | `asset.rs:236` | No learning system |
| `CalcCache` | `plan.rs:199` | Planner uses simplified `AllocEntry` |
| `StaleRatePolicy` | `asset.rs:107` | Not implemented |
| `ExternalDataSource` | `asset.rs` | No external data fetching |
| `ExternalDataFetchStatus` | `asset.rs` | No external data fetching |
| `UserNotificationSeverity` | `asset.rs:137` | No notification system |
| `OadrProgramConfig` | `capacity.rs:28` | Defined but not consumed by planner |
| `AssetLedger` (entity version) | `asset.rs:272` | `AssetLedgerEntry` in `state.rs` is used instead |
| `TariffHeuristic` | `tariff_snapshot.rs` | No heuristic forecast |

---

## 10. Summary Scorecard

| Category | Fulfilment | Rating |
|---|---|---|
| Entity Model (Step 1) | All structs/enums defined | 85% — types exist but many are dead code |
| Architecture (Step 2) | 6/7 components implemented | 80% — no Asset Controller |
| Phase 1: Prepare | Grid + tariffs + PV forecast | 55% — missing urgency override, stale rate policy, thermal model, forecasts |
| Phase 2: Score | ComfortBid + TimePressure + surplus cost | 50% — missing budget/rate/CO2 gates, CONTINUE path |
| Phase 3: Allocate | Greedy allocation working | 55% — **wrong sort order**, no asset sharing, no STEPPED, no budget |
| Phase 4: Storage | Median-based arbitrage | 60% — simplified but functional; no packet-based tracking |
| Phase 5: PV Surplus | Implicit tracking | 40% — no export limit, no curtailment |
| Phase 6: Penalty | Not implemented | 0% |
| Phase 7: Envelopes | Core logic present | 65% — no budget or rate filtering |
| Phase 8: Finalize | Estimates computed | 55% — no warnings, no PlannedPowerProfile |
| Dispatcher | Setpoints from plan | 75% — no session tracking |
| Monitor | Ledger + transitions | 60% — no deviation detection |
| Reporter | Measurement reports | 55% — no forecast/capacity reports |
| User Request Manager | Full CRUD | 80% — no tier fallback |

---

## 11. Prioritized Backlog

### P0 — Bugs (Spec Deviations That Produce Wrong Results)

---

#### B-01: Phase 3 sort order is inverted

**Location:** `planner.rs:286-291`

**Problem:** The greedy allocation loop sorts candidates by `MarginalValue DESC` only. The spec requires a two-key sort: `EffectiveCost ASC` first, then `MarginalValue DESC` within the same cost level. The current code fills highest-priority packets first regardless of slot cost. A high-priority EV will grab an expensive peak slot (EUR 0.40/kWh) even when a cheap off-peak slot (EUR 0.12/kWh) is available and has capacity — because the sort never considers cost.

The fundamental goal of the algorithm is cost minimization subject to priority. The current sort inverts this into priority maximization ignoring cost.

**Solution options:**

- **Option A (spec-faithful):** Change the sort to `(EffectiveCost ASC, MarginalValue DESC)`. This requires storing `effective_cost` in `AllocEntry` (currently not kept). Add an `effective_cost: f64` field to `AllocEntry`, populate it from the surplus-aware cost computation already done during scoring, then sort by `(effective_cost ASC, marginal_value DESC)`.
- **Option B (composite score):** Combine cost and value into a single score: `score = MarginalValue / EffectiveCost`. This naturally prefers high-value-per-cost entries. Simpler sort but deviates from spec's explicit two-key approach. Risk: division by near-zero export prices could inflate scores for surplus slots.
- **Recommendation:** Option A. It matches the spec, is straightforward, and the `AllocEntry` change is trivial.

**Complexity:** Low. ~15 lines changed. One new field on `AllocEntry`, one modified sort comparator.

**Verification tests:**
1. Two packets (EV high-priority, heater low-priority) competing for slots at two price levels (cheap and expensive). Assert EV lands on cheap slot, not expensive — cost wins over priority when both fit.
2. Two packets competing for the same cheap slot with insufficient capacity. Assert the higher-MarginalValue packet wins the scarce slot (priority breaks ties within same cost).
3. Regression: existing planner test `boundary_aligned_tariffs_match_old_behavior` must still pass.
4. BDD scenario: EV + heater with known tariff profile; verify total plan cost is lower than with current sort order.

---

#### B-02: No budget enforcement

**Location:** `planner.rs` (Phase 2 scoring + Phase 3 allocation)

**Problem:** `DeadlineTier.max_total_cost_eur` and `max_marginal_rate_eur_kwh` are carried through the entire data model (set via POST /requests, stored on packets, visible in API responses) but never checked during scoring or allocation. A user who sets a EUR 3 budget on an EV charge can end up with a EUR 10 plan. The spec defines three gates: `WithinComfortBid`, `WithinMarginalRate`, and `WithinBudget` — all three must pass for eligibility. Only the first is implemented.

**Solution options:**

- **Option A (full spec):** In `allocate_consumption`, track `packet_planned_cost[packet_id]` alongside `allocated[packet_id]`. During scoring, check: (1) `eff_cost <= tier.max_marginal_rate_eur_kwh` if set, (2) `packet_planned_cost + slot_cost <= tier.max_total_cost_eur` if set. Mark ineligible if either fails. During allocation, re-check with actual (not tentative) cost and reduce power to fit remaining budget if needed.
- **Option B (allocation-only check):** Skip the scoring-phase gate (Phase 2), but add a budget check in the allocation loop (Phase 3) that caps `energy_kwh` to what the remaining budget allows. Simpler, but the scoring phase may produce stale MarginalValue entries for over-budget slots.
- **Recommendation:** Option A. The scoring-phase check prevents over-budget slots from entering the candidate set at all, which is cleaner and matches the spec.

**Complexity:** Medium. ~30–40 lines. Need to thread `active_tier` through scoring, add cost accumulator per packet, add two `if` checks in scoring and one power-capping block in allocation.

**Verification tests:**
1. Packet with `max_total_cost_eur = 1.00` and slots costing EUR 0.30/kWh each. Assert allocation stops after ~3.3 kWh (budget exhausted), not at target energy.
2. Packet with `max_marginal_rate_eur_kwh = 0.20` and slots at EUR 0.25 and EUR 0.15. Assert only the EUR 0.15 slots are used.
3. Packet with tight budget + high time pressure (>= 2.0). Assert urgency override still allows scheduling (eligibility override) but cost is still tracked.
4. Two-tier packet: Tier 0 has EUR 2 budget, Tier 1 has EUR 5. After Tier 0 exhaustion, verify behavior (currently no tier fallback — see F-05 — but budget tracking itself should be correct).
5. BDD scenario: EV charge with EUR 3 budget; verify GET /plan shows `estimated_cost_eur <= 3.00`.

---

#### B-03: No post-deadline CONTINUE path

**Location:** `planner.rs:256` (scoring loop filter)

**Problem:** Packets with `CompletionPolicy::Continue` that are past their `latest_end` are excluded from scoring entirely by the filter `slot.start >= latest_end -> skip`. The spec says these packets should continue to be scored using `PostDeadlineComfortBid` as a flat bid (no fill-based interpolation) with `TimePressure = 1.0`. The `post_deadline_comfort_bid` field on `EnergyPacket` is set by `user_request.rs` but never read by the planner.

This means a washing machine mid-cycle that passes its deadline with `CONTINUE` policy and a high bid (EUR 5.00/kWh) simply stops being scheduled — it gets no further energy allocations.

**Solution options:**

- **Option A (extend scoring loop):** Remove the `slot.start >= latest_end -> skip` filter for `CONTINUE` packets. When a CONTINUE packet is past its `latest_end`, use `post_deadline_comfort_bid.unwrap_or(0.0)` instead of `value_curve.bid_at(fill)`, and set `time_pressure = 1.0`. Everything else (surplus cost, eligibility check, allocation) works the same.
- **Option B (synthetic packet):** When Monitor detects a packet passing its deadline with `CONTINUE`, spawn a new synthetic packet with `EarliestStart = now`, no `LatestEnd`, and `ComfortRate = flat post_deadline_bid`. This keeps the main scoring loop unchanged. More complex and adds packet lifecycle management.
- **Recommendation:** Option A. Minimal code change, no new packet creation, works within existing flow.

**Complexity:** Low. ~15 lines. One condition change in the filter, one branch for bid/pressure computation.

**Verification tests:**
1. CONTINUE packet past deadline with `post_deadline_comfort_bid = 0.50`. Assert it still receives allocations in slots where `eff_cost <= 0.50`.
2. CONTINUE packet past deadline with `post_deadline_comfort_bid = 0.01`. Assert it only gets nearly-free surplus slots (eff_cost ~ export price).
3. STOP packet past deadline. Assert it is NOT scored (terminal status, filtered correctly).
4. CONTINUE packet past deadline competing with a pre-deadline packet in the same slot. Assert the pre-deadline packet wins if its MarginalValue is higher (post-deadline bid has TimePressure=1.0, no urgency boost).
5. BDD scenario: washing machine with 1h cycle, 30min deadline, CONTINUE policy, high bid. Verify it completes even after deadline passes.

---

### P1 — Missing Features (Entire Capabilities Absent)

---

#### F-01: Phase 6 — Penalty threshold check

**Location:** `planner.rs:77` (comment: "deferred to Stage 4")

**Problem:** The entire penalty avoidance phase is absent. The spec describes a post-allocation pass that checks whether the plan would breach peak demand thresholds, energy budget limits, or export limits. If a breach is detected, the planner should try to reschedule the lowest-MarginalValue allocations to non-breaching slots, comparing the rescheduling cost against the penalty cost. After breach, the threshold becomes a soft constraint. Without this, a site with a EUR 100/month peak demand penalty at 15 kW can happily plan 20 kW without any attempt to avoid the charge.

**Solution options:**

- **Option A (full spec):** Implement the complete Phase 6 as described in Step 4 S10. After Phase 3+4, iterate `PenaltyRule`s. For `PEAK_DEMAND_EXCEEDED`: find breaching slots, attempt to move lowest-value allocations to alternative slots, compare avoidance cost vs penalty cost. Two paths: pre-breach (full penalty as barrier) and post-breach (soft constraint with small rescheduling budget). Similar logic for `ENERGY_BUDGET_EXCEEDED` and `EXPORT_LIMIT_EXCEEDED`.
- **Option B (peak-only, simplified):** Implement only `PEAK_DEMAND_EXCEEDED` with a simpler approach: after allocation, scan for slots exceeding threshold, defer lowest-priority packets from those slots to later slots. No cost comparison — always avoid if possible. Handles the most common real-world penalty type.
- **Option C (capacity cap integration):** Model peak demand penalties as import capacity limits. Set `import_cap_kw` to the penalty threshold value. This piggybacks on existing capacity enforcement in the allocation loop. Downside: no cost-benefit analysis (always avoids), and doesn't model the "already breached" soft path.
- **Recommendation:** Option B first, then upgrade to Option A when needed. Peak demand penalties are the most impactful; energy budget and export limit penalties are rarer in residential.

**Complexity:** High. ~80–120 lines for Option B, ~150–200 for Option A. Needs access to `PenaltyRule` config, current-period peak tracking, slot scanning, allocation movement logic, and PlanWarning generation.

**Verification tests:**
1. Plan that would schedule EV + heater + baseline = 18 kW in one slot with 15 kW threshold. Assert EV is deferred to a later slot, keeping peak at 15 kW.
2. Same scenario but no alternative slot available. Assert breach is accepted and a PlanWarning is emitted with the penalty cost.
3. Already-breached scenario (`breached_this_period = true`). Assert soft constraint: planner still tries to avoid but accepts small exceedances without expensive rescheduling.
4. Verify `PenaltyRule` integration with monitor: after `record_tick` detects an actual breach, `breached_this_period` flips and next plan cycle uses the soft path.
5. BDD scenario: UC-10 from Step 5 — peak demand penalty avoidance.

---

#### F-02: Per-packet urgency override for FIRM boundary

**Location:** `planner.rs` Phase 1, `build_grid()`

**Problem:** The FIRM/FLEXIBLE boundary is purely clock-based (`slot.end <= firm_boundary`). The spec says that if any packet has `TimePressure >= 2.0` for a slot beyond the near-horizon, that slot must be forced to FIRM for that packet. Without this, a battery charge packet with deadline 17:00 and `near_horizon = 2h` at 14:00 would have slots 16:00–17:00 marked FLEXIBLE — the battery misses its deadline because those critical slots never get firm allocations.

**Solution options:**

- **Option A (two-pass grid build):** Build the grid as today (clock-based). Then do a second pass: for each non-terminal packet, compute TimePressure for each FLEXIBLE slot. If TimePressure >= 2.0, promote that slot to FIRM. Move it from `flex` vec to `firm` vec (or mark with a flag). This changes the grid after initial classification.
- **Option B (pre-compute per-packet firm boundaries):** Before building the grid, compute each packet's effective firm boundary as `max(global_firm_boundary, packet_latest_end - margin)`. Then during grid build, a slot is FIRM if it's within ANY packet's effective firm boundary. Simpler but requires knowing packets at grid-build time (currently packets are processed after grid build).
- **Option C (pass packets to build_grid):** Extend `build_grid` to accept packets, compute per-slot urgency during grid construction. Most aligned with spec but changes the function signature.
- **Recommendation:** Option A. Non-invasive, works as a post-processing step on the already-built grid. Clear separation of concerns.

**Complexity:** Medium. ~30–40 lines. Second pass over flexible slots + packets, TimePressure computation (already exists in scoring), vec partition adjustment.

**Verification tests:**
1. Battery packet with deadline in 1h, `near_horizon = 2h` (deadline within near-horizon). Assert slot is already FIRM (baseline behavior, no change needed).
2. Battery packet with deadline in 3h, `near_horizon = 2h`. Slots 2h–3h should be promoted to FIRM because TimePressure >= 2.0 (only 1h margin for 2h of charging needed).
3. EV packet with deadline in 12h, `near_horizon = 2h`, needs 4h of charging. TimeSlack = 8h. Assert slots stay FLEXIBLE (no urgency).
4. Two packets: one urgent (forces slot to FIRM), one relaxed. Assert the shared slot is FIRM (any packet forcing it wins).
5. BDD scenario: battery charge-then-discharge with tight deadline beyond near-horizon. Verify charge completes on time.

---

#### F-03: Deviation detection in Monitor

**Location:** `monitor.rs`

**Problem:** The `record_tick` function handles energy accounting and packet status transitions but never compares actual power against planned power per asset. The spec says the Monitor should compute `|actual_kw - commanded_kw|` for each asset and, if the deviation exceeds `AssetProfile.deviation_threshold_kw` for a sustained period, emit `PlanTrigger::DeviceDeviation` to trigger replanning. Currently, `DeviceDeviation` is only emitted when a packet reaches completion — not when an asset is drifting from plan.

**Solution options:**

- **Option A (per-asset deviation check in record_tick):** After the existing ledger and packet logic, add a loop over `sim.assets`. For each asset, compare `asset.power_kw` with the planned setpoint from the current plan slot's allocation. If `|delta| > threshold` for N consecutive ticks, set trigger to `DeviceDeviation`. Requires passing the current plan's setpoints into `record_tick` (or the `DispatchState`).
- **Option B (separate deviation_check function):** Create a new `pub fn check_deviations(sim, plan, thresholds, history) -> Option<PlanTrigger>` called from the tick loop alongside `record_tick`. Keeps record_tick focused on accounting. The caller merges triggers.
- **Recommendation:** Option B. Cleaner separation; `record_tick` already has a complex signature. The deviation check is logically distinct from energy accounting.

**Complexity:** Medium. ~40–50 lines for the function. Need: planned setpoint lookup from current plan slot, per-asset deviation counter (e.g., `HashMap<String, u32>` for consecutive ticks), threshold from profile or config, trigger emission.

**Verification tests:**
1. Asset planned at 5 kW, actual at 5 kW. Assert no trigger.
2. Asset planned at 5 kW, actual at 2 kW (deviation 3 kW > threshold 1 kW) for 3 consecutive ticks. Assert `DeviceDeviation` trigger.
3. Asset with brief spike (1 tick above threshold, then back). Assert no trigger (sustained check filters transients).
4. Multiple assets: one deviating, one not. Assert trigger fires (any asset can cause it).
5. BDD scenario: override a sim asset to deviate from plan; verify planner re-runs.

---

#### F-04: PlanWarnings generation

**Location:** `planner.rs:108` (`warnings: vec![]`)

**Problem:** The planner never generates any warnings. The spec defines warnings for: packet cannot complete in active tier, energy awaiting VTN signals, STOP packet won't reach 100%, tight capacity near limits, and penalty breach accepted. Without warnings, the user and downstream systems have no visibility into plan quality or problems.

**Solution options:**

- **Option A (post-finalize warning pass):** After `finalize_packets`, add a `generate_warnings()` function that scans the plan output. Conditions to check: (1) packet with `estimated_completion < 1.0` and `CompletionPolicy::Stop` -> WARNING, (2) packet with remaining flexible energy -> INFO, (3) slot with `import_flexibility_kw < 0.5 * headroom` -> INFO, (4) any penalty breach accepted (requires F-01) -> WARNING.
- **Option B (warnings accumulated during phases):** Collect warnings into a `Vec<PlanWarning>` passed through each phase. Each phase appends relevant warnings. More aligned with spec but requires threading the vec through all functions.
- **Recommendation:** Option A for now. Post-hoc scanning is simpler and doesn't change function signatures. Upgrade to Option B when warning detail needs per-phase context.

**Complexity:** Low-Medium. ~40–60 lines. Scanning logic over packets and slots, constructing `PlanWarning` structs.

**Verification tests:**
1. Packet that can only reach 70% fill within FIRM slots and no FLEXIBLE envelopes. Assert WARNING with message containing "cannot complete".
2. Packet with remaining energy in FLEXIBLE envelope. Assert INFO with "awaiting VTN price signals".
3. STOP packet with `estimated_completion = 0.85`. Assert WARNING mentioning partial completion.
4. Slot with `import_flexibility_kw = 0.1` (very tight). Assert INFO about tight capacity.
5. All packets fully allocated, no constraints. Assert `warnings` vec is empty.

---

#### F-05: Tier fallback

**Location:** `planner.rs`, `energy_packet.rs`

**Problem:** `ValueCurve.active_tier_index` is stored on every packet and persisted, but the planner never advances it. When Tier 0 is infeasible (can't reach `min_completion` within budget/deadline), the spec says the planner should increment `active_tier_index` and re-evaluate with the relaxed constraints of the next tier. Without this, a packet stuck on an infeasible Tier 0 stays there forever and never tries the fallback.

**Solution options:**

- **Option A (pre-plan tier check):** Before running the main allocation, check each packet's active tier feasibility: can enough FIRM+FLEXIBLE slots deliver `min_completion` within the tier's budget and deadline? If not, advance `active_tier_index` and emit a PlanWarning. Repeat until a feasible tier is found or all tiers exhausted (-> ABANDONED).
- **Option B (post-plan tier check):** Run the full allocation, then check `estimated_completion` against `active_tier.min_completion`. If below, advance tier, re-run planner. Simpler logic but wastes a planning cycle.
- **Recommendation:** Option A. Pre-plan check avoids wasted computation and matches the spec's "evaluate before scoring" approach.

**Complexity:** Medium. ~30–40 lines. Feasibility check per packet (compare available energy vs target * min_completion, compare available budget vs planned cost), tier advancement, warning generation. Depends on F-04 for warnings.

**Verification tests:**
1. Packet with Tier 0 deadline in 1h, needs 8h of charging. Assert `active_tier_index` advances to Tier 1 (which has a later deadline).
2. Packet with Tier 0 budget EUR 1.00, all slots cost EUR 0.30/kWh, needs 10 kWh. Assert tier advance (EUR 1 can only buy 3.3 kWh).
3. Packet with only one tier and infeasible constraints. Assert status -> `Abandoned` and warning emitted.
4. Packet where Tier 0 is feasible. Assert `active_tier_index` stays at 0.
5. BDD scenario: UC-09 from Step 5 — tier fallback when EV can't meet Tier 0 deadline.

---

#### F-06: CO2 eligibility gate

**Location:** `planner.rs` (Phase 2 scoring)

**Problem:** `ComfortRate.max_marginal_co2` is defined per fill level and stored on every comfort rate entry, but it is never checked in the eligibility evaluation. The spec requires: `CO2Rate <= interpolatedMaxMarginalCO2(P, ProjectedFill)` — slots with high CO2 intensity should be ineligible for packets whose users set strict CO2 limits. A user who sets `max_marginal_co2 = 200` gets scheduled on 400 g/kWh slots.

**Solution options:**

- **Option A (add CO2 interpolation + gate):** Add a `co2_bid_at(fill)` method to `ValueCurve` (analogous to `bid_at(fill)` but for `max_marginal_co2`). In the scoring loop, after computing `eligible`, add: `eligible = eligible && (slot.co2_g_kwh <= co2_bid)` (skip if co2_bid is 0.0, meaning no CO2 constraint).
- **Option B (single CO2 threshold, no fill curve):** Use a flat CO2 threshold from the packet (simpler than per-fill interpolation). Less flexible but covers the main use case.
- **Recommendation:** Option A. The infrastructure for fill-based interpolation already exists in `bid_at()`. Adding a parallel `co2_bid_at()` is trivial.

**Complexity:** Low. ~15 lines. One new method on `ValueCurve`, one additional check in the eligibility condition.

**Verification tests:**
1. Packet with `max_marginal_co2 = 200` at all fill levels. Slots at 150 g/kWh and 300 g/kWh. Assert only 150 g/kWh slot is eligible.
2. Packet with `max_marginal_co2 = 0.0` (no constraint). Assert all slots eligible regardless of CO2.
3. Packet with fill-dependent CO2: strict at low fill (200 g), relaxed at high fill (500 g). At 90% fill, slot at 400 g should be eligible; at 10% fill, it should not.
4. Urgent packet (TimePressure >= 2.0) with CO2 gate. Assert urgency override still works (packet scheduled despite CO2 being above threshold — matching the `eligible = ... || time_pressure >= 2.0` pattern).

---

#### F-07: PlannedPowerProfile not written

**Location:** `planner.rs` Phase 8, `finalize_packets()`

**Problem:** `EnergyPacket.planned_power_profile` is a `Vec<EnergySnapshot>` that should contain the optimizer's planned power at each FIRM slot for that packet. It is never populated. The UI timeline, diagnostics, and deviation detection all need this data to show "what was planned" vs "what happened". Without it, there is no planned curve for per-packet visualization.

**Solution options:**

- **Option A (build in finalize_packets):** In `finalize_packets`, for each packet, iterate FIRM slots, find allocations matching `packet.id`, and push `EnergySnapshot { ts: slot.start, power_kw: alloc.power_kw, cumulative_energy_kwh: running_sum }` into `planned_power_profile`.
- **Option B (build during allocation):** Append to `planned_power_profile` during the Phase 3 allocation loop as each allocation is committed. Avoids a second scan but mixes planning output into the allocation logic.
- **Recommendation:** Option A. Clean separation. `finalize_packets` is already scanning allocations for cost/co2 sums — adding profile construction is natural.

**Complexity:** Low. ~15–20 lines in `finalize_packets`. One additional inner loop with running cumulative sum.

**Verification tests:**
1. Packet allocated in 3 FIRM slots at different power levels. Assert `planned_power_profile` has 3 entries with correct timestamps and cumulative energy.
2. Packet with no FIRM allocations (all FLEXIBLE). Assert `planned_power_profile` is empty.
3. Assert `planned_power_profile` entries are sorted by timestamp.
4. Verify `cumulative_energy_kwh` on the last entry matches `estimated_completion * target_energy_kwh` (within rounding).
5. BDD scenario: GET /packets returns non-empty `planned_power_profile` for a scheduled packet; verify UI timeline shows planned curve.

---

### P2 — Simplifications (Working But Not Per Spec)

---

#### S-01: TimePressure is continuous, not discrete

**Location:** `planner.rs:251-253`

**Problem:** The spec defines discrete TimePressure levels:
- `TimeSlack <= 0` -> 3.0 (critical)
- `TimeSlack == 1` -> 2.0 (tight)
- `TimeSlack <= 3` -> 1.5 (pressure)
- `else` -> 1.0 (comfortable)

The code uses: `(slots_needed / slots_remaining).clamp(1.0, 3.0)` — a continuous ratio. This produces different values: e.g., `slots_needed=5, slots_remaining=10` gives 0.5 clamped to 1.0 (correct), but `slots_needed=4, slots_remaining=5` gives 0.8 clamped to 1.0 (spec says 1.5 since TimeSlack=1). The continuous version under-pressures near-deadline packets in the 1.0–2.0 range.

**Solution options:**

- **Option A (discrete levels per spec):** Compute `time_slack = slots_remaining - slots_needed`, then map to discrete levels with `match`. Direct translation of the spec pseudocode.
- **Option B (keep continuous but fix range):** Use `(slots_needed as f64 / slots_remaining as f64).max(1.0).min(3.0)` but add a floor at 1.5 when `time_slack <= 3`. Hybrid approach.
- **Recommendation:** Option A. The spec's discrete levels were chosen for predictability and debuggability. Easy to implement and test.

**Complexity:** Very low. ~10 lines. Replace the current expression with a `time_slack` computation and match statement.

**Verification tests:**
1. `slots_needed=5, slots_remaining=3` (slack=-2). Assert TimePressure = 3.0.
2. `slots_needed=5, slots_remaining=6` (slack=1). Assert TimePressure = 2.0.
3. `slots_needed=5, slots_remaining=8` (slack=3). Assert TimePressure = 1.5.
4. `slots_needed=5, slots_remaining=20` (slack=15). Assert TimePressure = 1.0.
5. Post-deadline CONTINUE packet. Assert TimePressure = 1.0 regardless of slots (see B-03).

---

#### S-02: Battery arbitrage uses median, not quartiles

**Location:** `planner.rs:363-433`

**Problem:** The spec identifies cheap and expensive slots using quartile analysis: charge in slots in the lower cost quartile, discharge in slots in the upper quartile. The code uses a simpler median-based threshold: charge when `price < median * sqrt(eff)`, discharge when `price > median / sqrt(eff)`. This is a reasonable approximation but may over-charge in middling-cost slots or miss optimal discharge windows when the cost distribution is skewed.

**Solution options:**

- **Option A (quartile-based):** Compute Q1 and Q3 of `grid_effective_cost` across FIRM slots. Charge in slots below Q1, discharge in slots above Q3. Matches spec.
- **Option B (percentile-configurable):** Use configurable low/high percentile thresholds (e.g., 25th/75th). More flexible than fixed quartiles.
- **Option C (keep median, document deviation):** The current approach works and produces reasonable results for typical residential tariff profiles. Document the deviation and leave as-is.
- **Recommendation:** Option C for now. The median approach is adequate for the current simulation profiles. Upgrade to Option A when real tariff data reveals edge cases.

**Complexity:** Low (Option A: ~15 lines to compute percentiles and change the thresholds).

**Verification tests:**
1. Uniform tariff (all slots same price). Assert no charge/discharge occurs (no profitable arbitrage).
2. Bimodal tariff (4 cheap slots at EUR 0.05, 4 expensive slots at EUR 0.40). Assert battery charges in cheap, discharges in expensive.
3. Skewed tariff (20 slots at EUR 0.15, 2 slots at EUR 0.50). Assert battery targets the 2 expensive slots for discharge.
4. Round-trip efficiency test: charge cost / efficiency > discharge value. Assert no arbitrage (correctly skipped).

---

#### S-03: No asset sharing / slot commitment tracking

**Location:** `planner.rs` (Phase 3 allocation loop)

**Problem:** The spec tracks `assetSlotCommitment[Asset, S]` — how much power from a given asset is already committed in a given slot. If two packets target the same asset (e.g., two EV charge requests on the same charger), both get the full `desired_power_kw` allocated in the same slot, exceeding the physical asset limit. The code tracks surplus and import capacity per slot but not per-asset commitment.

**Solution options:**

- **Option A (asset-slot commitment map):** Add `HashMap<(String, usize), f64>` tracking committed power per `(asset_id, slot_index)`. In the allocation loop, check `available_asset_power = max_power - commitment[asset, slot]`. After allocation, update the map. Matches spec.
- **Option B (packet-asset exclusivity):** Enforce that each asset can only serve one active packet at a time. Simpler but more restrictive than the spec (which allows partial sharing).
- **Recommendation:** Option A. Low risk, and the situation does occur (e.g., two user requests for the same EV at different deadlines).

**Complexity:** Low-Medium. ~15–20 lines. One new HashMap, one check + one update per allocation.

**Verification tests:**
1. Two packets on the same 7 kW asset in the same slot. Assert total allocation is 7 kW (not 14 kW).
2. Two packets on different assets in the same slot. Assert both get their full power (no interference).
3. One packet fills the asset in a slot; second packet gets 0 kW and moves to next slot. Assert correct behavior.

---

#### S-04: No STEPPED asset handling

**Location:** `planner.rs` (Phase 3 allocation loop)

**Problem:** Assets with `PowerAdjustability::Stepped` (e.g., heater with 0/3/6 kW) should only be allocated at one of their defined power levels. The code treats all assets as stepless — it allocates any fractional power value (e.g., 4.5 kW on a 0/3/6 kW heater). The simulator may then round or clip this, causing deviation from plan.

**Solution options:**

- **Option A (step selection in allocation):** When allocating power for a STEPPED asset, snap to the largest feasible step level: `feasible_steps = [s for s in power_steps where s <= available_capacity AND s <= needed]`, then use `max(feasible_steps)`. Requires access to `AssetProfile.power_range.power_steps_kw` during allocation.
- **Option B (post-allocation snap):** Let allocation run as-is, then round each allocation's `power_kw` down to the nearest valid step. Simpler but may leave capacity unused.
- **Recommendation:** Option A. Proper integration ensures correct energy accounting. Requires passing asset profiles (or a lookup function) into `allocate_consumption`.

**Complexity:** Medium. ~20–30 lines. Asset profile lookup in the allocation loop, step selection logic, adjusted energy calculation.

**Verification tests:**
1. Heater with steps [0, 3, 6] kW, available capacity 5 kW. Assert allocation at 3 kW (largest step <= 5).
2. Same heater, available capacity 7 kW. Assert allocation at 6 kW.
3. Same heater, available capacity 2 kW. Assert allocation at 0 kW (no feasible step — skip slot).
4. Stepless asset with same scenario. Assert allocation at full available capacity (no stepping).
5. ON_OFF asset (equivalent to [0, max_kw]). Assert allocation is either 0 or max_kw.

---

#### S-05: Battery uses Uuid::nil(), not separate packets

**Location:** `planner.rs:400`

**Problem:** Battery charge/discharge allocations use `Uuid::nil()` as the packet_id. The spec says battery operations should be modeled as separate EnergyPackets (charge packet with `CompletionPolicy::Stop` at discharge start, discharge packet covering the discharge window). This matters for: budget tracking per packet, packet-level cost attribution, UI display of battery schedule, and interaction with the tier/completion system.

**Solution options:**

- **Option A (auto-generate battery packets):** During Phase 1 or Phase 4, create synthetic `EnergyPacket` instances for battery charge and discharge. Charge packet targets `soc_target` by discharge start time. Discharge packet covers the discharge window. Both participate in normal allocation and tracking.
- **Option B (dedicated battery allocation struct):** Create a `BatteryAllocation` struct separate from `PacketAllocation` to avoid conflating battery scheduling with packet scheduling. Simpler but diverges from the unified packet model.
- **Option C (keep Uuid::nil(), improve attribution):** Keep synthetic allocations but add battery-specific cost/energy tracking outside the packet system. Pragmatic but increases code paths.
- **Recommendation:** Option A long-term (unified model), Option C short-term (minimal disruption). The current system works for simulation; proper packets matter when battery costs need to show up in user-facing budgets.

**Complexity:** High for Option A (~60–80 lines: packet creation, lifecycle management, integration with planner). Low for Option C (~10 lines: separate battery summary struct).

**Verification tests:**
1. (Option A) Battery charge packet visible in GET /packets with status, energy target, cost.
2. (Option A) Battery discharge packet's energy matches delivered kWh from discharge slots.
3. (Option A) Charge packet transitions: Pending -> Scheduled -> Active -> Completed as battery fills.
4. (Any) Battery cost attribution: total battery cost = charge cost - discharge savings. Verify in plan summary.

---

#### S-06: Envelope budget is always f64::MAX

**Location:** `planner.rs:497`

**Problem:** `FlexibilityEnvelope.budget_remaining_eur` is always set to `f64::MAX`. The spec says it should be `ActiveTier.MaxTotalCost - AccumulatedCost - FIRM_slot_costs`. Without a real budget, the envelope overstates flexibility — the VTN sees "this VEN can absorb unlimited cost" when in reality the user has a EUR 3 cap.

**Solution options:**

- **Option A (compute from tier):** In `build_envelopes`, look up the packet's active tier. Compute `budget_remaining = tier.max_total_cost_eur.unwrap_or(f64::MAX) - packet.accumulated_cost_eur - firm_cost`. Where `firm_cost` = sum of `cost_eur` from FIRM allocations for this packet.
- **Recommendation:** Option A. Straightforward, all data is already available.

**Complexity:** Very low. ~5–8 lines. One tier lookup, one sum, one subtraction.

**Verification tests:**
1. Packet with Tier 0 budget EUR 5.00, accumulated EUR 1.50, FIRM costs EUR 2.00. Assert `budget_remaining_eur = 1.50`.
2. Packet with no budget (`max_total_cost_eur = None`). Assert `budget_remaining_eur = f64::MAX`.
3. Packet fully allocated in FIRM (no envelope generated). Assert no envelope at all (existing behavior).

---

#### S-07: No surplus tentative tracking across packets in Phase 2

**Location:** `planner.rs` (Phase 2 scoring)

**Problem:** The spec tracks `surplusTentativelyClaimed[S]` during Phase 2 scoring. When packet A is tentatively assigned surplus in a slot, the surplus pool shrinks for packet B's scoring of the same slot. Without this, multiple packets all see the full surplus, leading to over-optimistic `EffectiveCost` computations. Phase 3 has correct surplus tracking (it decrements `slot.surplus_available_kw`), but Phase 2 scoring produces misleading MarginalValue entries.

**Solution options:**

- **Option A (per-slot surplus counter in scoring):** Add `HashMap<usize, f64>` tracking tentatively claimed surplus per slot index during the scoring loop. When a packet's entry is marked eligible, decrement the slot's remaining surplus. Use this adjusted surplus for subsequent packets' cost computations.
- **Option B (accept the approximation):** The spec acknowledges this is a "known approximation" (S6 note after Phase 2 pseudocode): "Acceptable for residential scale (3-10 packets rarely compete for surplus in the same slot)." Phase 3's authoritative surplus tracking corrects any errors. Document and defer.
- **Recommendation:** Option B for now. The spec itself calls this acceptable. Revisit if surplus contention becomes visible in test scenarios.

**Complexity:** Low for Option A (~10 lines), but unnecessary given the spec's own assessment.

**Verification tests (if implementing Option A):**
1. Two packets competing for 3 kW surplus in the same slot. First packet claims 2 kW, second sees 1 kW remaining. Assert second packet's EffectiveCost reflects reduced surplus.
2. No surplus in slot. Assert both packets see pure grid cost (no change from current behavior).
3. One packet, plenty of surplus. Assert full surplus available (no change from current behavior).

---

### P3 — Not Yet Needed (Future Capabilities)

| ID | Title | Notes |
|---|---|---|
| N-01 | **StaleRatePolicy implementation** | All 4 policies defined but only hardcoded defaults used |
| N-02 | **SITE_RESIDUAL derivation and learning** | No SiteMeter-based residual, no heuristic learning |
| N-03 | **Thermal TargetEnergy recomputation** | No thermal model integration for heater/heat pump |
| N-04 | **Asset availability windows** | No EV disconnection window checks |
| N-05 | **Early firm-up heuristic** | No rate variance analysis on FLEXIBLE slots |
| N-06 | **AssetHeuristics learning system** | Daily pattern learning not implemented |
| N-07 | **ExternalDataSource refresh** | Weather/irradiation data fetching not implemented |
| N-08 | **USAGE_FORECAST / CAPACITY_RESERVATION reports** | Reporter only does measurement + status |
| N-09 | **Report obligation fulfillment tracking** | `OadrReportObligation` not wired |
| N-10 | **DeviceSession management** | No real-time session tracking in dispatcher |
| N-11 | **User notification system** | `UserNotificationSeverity` defined but no delivery mechanism |
| N-12 | **Battery-consumption interaction** (Phase 4, S8.3) | No post-Phase-3 reallocation for battery displacing expensive consumption |
| N-13 | **Export capacity limit curtailment** (Phase 5) | No PV curtailment at export limit |
| N-14 | **Batch process scheduling risk** (S13) | No look-ahead for expensive windows after batch start |
| N-15 | **Asset Controller** | No device protocol integration; simulator acts as proxy |
