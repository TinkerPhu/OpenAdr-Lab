# Deviation Control — Design Suggestions

> Status: partially implemented — Tier 1 absorber is built; Tier 2 threshold tuning and profile schema are not yet applied  
> Context: VEN HEMS controller; original idea from `016-refactor-ven-backend`

## Implementation Status (2026-07-03)

| Component | Status | Notes |
|---|---|---|
| Tier 1 — Deviation absorber (`controller/absorber.rs`) | **Not yet created** | `absorber.rs` does not exist in `VEN/src/controller/`. The design and API sketch in this doc are the spec for the implementation. |
| Tier 2 — DeviceDeviation gate | **Exists** | Implemented in the sim tick loop; threshold `deviation_trigger_ticks` is profile-configurable |
| MILP planner periodic interval | **Configurable** | `replan_interval_s` in `PlannerParams`; production tuning per this doc's recommendations not yet applied |
| Profile schema (`absorber_enabled`, `absorber_assets`, etc.) | **Not yet added** | Profile struct does not carry absorber config yet |

**Next step:** Implement `absorber.rs` per the design in §Tier 1 below. The open questions are all answered (see inline answers). The profile schema is specified in §Profile schema.

---

## Problem

The current control architecture has two layers:

| Layer | What | How fast | Cost |
|-------|------|----------|------|
| Layer 1 | Battery-only correction overlay (Plan F/G) | Every tick (1 s) | Cheap |
| Layer 2 | Full MILP replan triggered by DeviceDeviation | 5–120 s solve | Expensive |

When the actual grid power deviates from the MILP plan, Layer 1 adjusts the battery setpoint by a correction delta and holds it for a few ticks. If the deviation persists beyond `deviation_trigger_ticks` (currently 10 s in the test profile), Layer 2 fires a full MILP replan.

This creates a feedback loop: each replan changes setpoints, which causes transient deviation, which triggers another replan. Under the three-VEN test load on Pi4, solves take 80–120 s and the planner runs continuously. Even in a single-VEN production deployment, `deviation_trigger_ticks=10` causes the planner to solve almost continuously whenever assets are transitioning.

---

## Proposed architecture

Replace the binary Layer 1 / Layer 2 split with a three-tier hierarchy:

```
Tick loop (1 s)
│
├─ Tier 1: Real-time deviation absorber   (new — replaces current Layer 1)
│     Multi-asset, proportional, bounded by MILP envelope
│     Goal: cancel the deviation delta, not reach any SoC/temperature target
│
├─ Tier 2: DeviceDeviation gate           (existing Layer 2, threshold raised)
│     Fires only when Tier 1 exhausts available flexibility
│     deviation_trigger_ticks: 10 → 60–120 (production profiles)
│
└─ MILP planner                           (existing, replan_interval_s: 20 → 120–300)
      Runs periodically or on hard triggers (new event, new user request)
      Always starts from actual sim state — SoC drift is picked up automatically
```

---

## Tier 1: Real-time deviation absorber

### Goal

At each tick, measure the deviation between actual and planned grid power and apply the minimum setpoint adjustment across a prioritised asset list to cancel it. The absorber is **error-correcting, not goal-directed** — it does not try to charge the battery to a target SoC or heat the tank to a comfort temperature. Those goals remain in the MILP plan.

### Deviation signal

```
deviation_kw = actual_grid_net_kw - planned_grid_net_kw
```

Positive = importing more than planned (too much load or less PV than expected).  
Negative = importing less than planned (less load or more PV than expected).

A dead-band (e.g., ±0.1 kW) prevents chatter from measurement noise.

### Priority order (profile-configurable)

When deviation is positive (reduce import): decrease controllable loads in order.  
When deviation is negative (increase import / absorb surplus): increase loads in order.

Suggested default order:

1. **Battery** — fastest response, no user comfort impact, but SoC-bounded
2. **EV charger** — comfort impact only when near departure with low SoC; can be throttled proportionally
3. **Heater / boiler** — thermal inertia gives tolerance; bounded by min/max temperature

Each asset entry in the profile could carry an `absorber_priority` integer (lower = earlier in order) and optionally opt out entirely.

### Flexibility bounds

The absorber operates within the bounds already computed by the MILP for the current slot. Specifically:

- Do not exceed the MILP plan's `net_import_kw` limit for this slot (hard capacity constraint).
- Do not discharge the battery below what the MILP reserved for later slots — use `SiteFlexibilityEnvelope.max_export_kw` and `max_import_kw` as the per-asset bounds.
- Do not charge EV beyond `soc_target` or below `min_soc_pct`.
- Do not drive heater outside `[temp_min_c, temp_max_c]`.

If a deviation cannot be fully absorbed within these bounds, the remaining uncovered delta propagates to the DeviceDeviation gate (Tier 2).

### Setpoint behaviour

The absorber produces a **temporary overlay** on top of the MILP setpoint. When the deviation clears, the overlay ramps back to zero and the asset returns to its MILP-planned setpoint. This is distinct from the opportunistic EV overlay, which is persistent across the full slot duration.

The absorber should be roughly symmetric over time — temporary boosts and reductions should average out, not cause net SoC drift. If the absorber consistently biases one direction (e.g., always discharges battery to absorb afternoon PV ramps), the MILP replan picks up the resulting actual SoC at its next cycle and re-optimises accordingly.

### Implementation sketch

The absorber fits naturally as a new phase in the sim tick orchestrator in `loops.rs`, between the existing `apply_deviation_correction` (which would be simplified or removed) and the physics tick:

```rust
fn apply_deviation_absorption(
    deviation_kw: f64,           // actual - planned, signed
    setpoints: &mut Setpoints,
    plan_envelope: &SiteFlexibilityEnvelope,
    sim: &SimState,
    profile: &Profile,
    now: DateTime<Utc>,
) -> f64 {  // returns residual uncovered deviation
    ...
}
```

The function iterates the profile's absorber asset list in priority order, adjusts each asset's setpoint by as much as the flexibility envelope permits, and returns whatever deviation could not be covered. The Tier 2 gate then uses that residual to increment the deviation tick counter.

---

## Tier 2: DeviceDeviation gate (revised thresholds)

The gate changes role: instead of triggering on any sustained deviation, it triggers only when the absorber's flexibility is genuinely exhausted. This makes it a signal of a structural plan mismatch rather than a normal transient.

Recommended profile values for production:

| Parameter | Test profile | Production profile |
|-----------|-------------|-------------------|
| `deviation_trigger_ticks` | 10 | 60–120 |
| Residual threshold | n/a (current: uses raw deviation) | absorber residual > X kW for N ticks |

The test profile keeps `deviation_trigger_ticks=10` to keep BDD scenarios fast. Production profiles can afford 60–120 without risking prolonged uncontrolled deviation, because the absorber is handling it continuously.

---

## MILP planner (revised interval)

With the absorber managing intra-slot deviations, the MILP no longer needs to re-run every 20 seconds. It only needs to run when:

- A new tariff or capacity event arrives from the VTN (`RateChange` trigger — keep as hard trigger)
- A new user request is posted (`UserRequest` trigger — keep as hard trigger)
- The absorber exhausts its flexibility (`DeviceDeviation` trigger — keep, raised threshold)
- A periodic drift correction (`Periodic` trigger — raise from 20 s to 120–300 s)

Recommended production value: `replan_interval_s: 300` (5 minutes).

At 300 s interval and a 5–10 s solve on a single-VEN Pi4: the planner uses ~2–3% CPU. The absorber uses <1% (arithmetic only). This leaves the Pi4 free for HTTP, polling, and sim ticks.

### SoC drift — not an issue

Every MILP solve starts from a live snapshot of the current sim state:

```rust
let sim_snap = sim.lock().await.clone();  // actual SoC, temp, power right now
```

If the absorber adjusted battery SoC between replans (e.g., discharged 3 kWh absorbing deviation), the next MILP sees the actual lower SoC and re-optimises from there. This is standard Model Predictive Control behaviour — the planner always initialises from reality, so absorber-induced drift is automatically corrected in the next planning cycle.

The only risk is if the absorber systematically violates the MILP's energy reservation for future slots (e.g., depleting battery capacity that was reserved for evening peak shaving). Bounding the absorber by `SiteFlexibilityEnvelope` prevents this: as long as the absorber stays within the envelope the plan computed, the energy budget for future slots remains intact.

---

## What this does NOT replace

The absorber handles **intra-slot, intra-plan deviations**. It does not:

- Re-optimise across the planning horizon (MILP still owns this)
- Respond to new VTN events or user requests (hard triggers still fire immediately)
- Replace the MILP's role in computing the flexibility envelope (the absorber depends on it)
- Handle asset failures or unexpected outages beyond what the envelope already accounts for

---

## Open questions

1. **Absorber granularity**: Should the absorber adjust setpoints proportionally (fractional adjustment spread across assets) or sequentially (fill one asset fully before moving to the next)? Sequential is simpler; proportional distributes wear more evenly.
**Answer**: **Sequential**. It is deterministic, easier to test, and the priority ordering already encodes intent. Proportional adds algorithm complexity (splitting across three assets simultaneously) without real benefit — if battery hits its limit, EV still gets full adjustment anyway. Avoids a category of edge cases (what if battery and EV reach limits mid-adjustment?). Ship sequential.

2. **Return-to-plan ramp**: When deviation clears, how fast does the absorber ramp back to the MILP setpoint? Instant snap avoids lingering but can cause a brief counter-deviation. A 1–3 tick ramp is likely sufficient.
**Answer**: **1-tick ramp with settling logic**. Ramp back over exactly 1 second, but only if deviation stays below dead-band for at least one full settling tick first. Logic: (1) while `|deviation| > dead_band`, hold overlay; (2) once `|deviation| ≤ dead_band` for 1 tick, begin 1-tick ramp to zero. This prevents oscillation on/off and avoids counter-deviation spikes. Once settled and ramped, asset returns to clean MILP setpoint for the next tick's opportunity/deviation decisions.

3. **EV absorber behaviour near departure**: If the EV is 30 minutes from departure and 10% below target SoC, the absorber should probably not reduce EV charging even if it would help absorb deviation. This requires the absorber to be aware of the EV session's urgency — possibly via the flexibility envelope urgency field or a simple time-to-departure check.
**Answer**: **Hard cutoff: time-to-departure check (user setting, default 30 minutes, optional disable)**. When `time_to_departure < threshold_s`, absorber refuses to reduce EV charging (can still increase if needed to absorb export surplus). Simple, predictable, operationally clear. Trade-off: hard cliff (at 30:01 allow reduction; at 29:59 forbid) vs. soft gradient (linearly decay EV flexibility from 30 min → 0 min). Hard cutoff is pragmatic for first version; soft gradient can be added later if operational data shows cliff effect causes problems. SoC urgency is implicitly handled by the flexibility envelope — if EV is critically low SoC, the envelope will be tight anyway and the absorber cannot exceed it.

4. **Interaction with opportunistic EV**: The opportunistic EV overlay is already a persistent setpoint increase (not deviation-driven). The absorber needs to treat it as a fixed load (not an absorption target) when it's active, or the two can cancel each other.
**Answer**:   Execution order (per tick):
  1. Opportunistic EV computes its persistent overlay once per slot based on the current MILP setpoint, applied to battery/EV/heater.
  2. Absorber measures residual deviation and applies transient corrections in the absorber priority order from the profile (e.g., battery → EV → heater).

  Why they don't interfere:
  - When absorber adjusts the EV setpoint, it's either:
  - Within margin: correction absorbed perfectly (absorber is doing its job)
  - At a limit: EV is full, charging max, or urgency prevents further adjustment → absorber moves to next asset (battery for production/consumption surplus, heater for extra consumption)
  - After all assets exhausted: residual falls below dead-band or can't be absorbed → propagates to Tier 2 gate (hard trigger for replan)

  The dead-band hypothesis holds because if an asset isn't at a limit, the absorber should absorb the correction. If it is at a limit, the absorber steps to the next priority asset. Only after exhausting all flexibility does residual deviation survive the dead-band check.

  State isolation:
  Each tick's absorber correction is transient and does not carry over. The next tick's opportunistic calculation always sees the clean MILP setpoint (not the previous tick's absorber delta). This keeps the two layers decoupled: opportunistic optimizes long-term gain; absorber handles transient deviations.
  Result: No cancellation. Opportunistic gets its flexibility budget. Absorber smooths intra-slot noise. If both exhaust their bounds, Tier 2 replans.

5. **Profile schema**: Which fields are needed? Candidates: `absorber_enabled: bool`, `absorber_dead_band_kw: f64`, `absorber_assets: Vec<{id, priority, max_step_kw}>`.
**Answer**: **Concrete schema** (to lock in now):
```yaml
absorber_enabled: bool
absorber_dead_band_kw: f64           # e.g., 0.1
absorber_dead_band_clearing_ticks: usize  # ticks before ramp starts, e.g., 1
absorber_assets:
  - id: "battery"
    priority: 0
    min_state_linger_s: 0             # electronic, no wear concern
    # no max_step_kw — let flexibility envelope + asset state compute bounds
  - id: "ev"
    priority: 1
    min_state_linger_s: 0             # solid-state or high-cycle rated
    ev_absorber_time_to_departure_min_s: u64  # e.g., 1800 (30 min); opt-out with 0
  - id: "heater"
    priority: 2
    min_state_linger_s: 30            # electromechanical relay: prevent wear from rapid cycling
```
**Rationale**: Drop `max_step_kw` — absorber should be bounded entirely by flexibility envelope + asset limits (SoC, temp). A separate per-tick step field forces underutilisation or adds a third constraint loop. Add `dead_band_clearing_ticks` (Q2 detail), EV time-to-departure threshold (Q3 detail), and `min_state_linger_s` per asset (see new Relay Wear section below) as configurable per-profile. The schema is now complete for first implementation.

---

## Relay Wear — minimum state linger time

### Problem

Mechanical and electromechanical assets (heater relay, boiler on/off, HVAC compressor) have finite switching lifetime. Each on/off cycle causes arcing, material degradation, and reduced relay lifespan. If the absorber or planner changes the relay state every 1–5 seconds, a relay rated for 100,000 cycles might fail in weeks instead of years.

Example: a 10 kW heater relay rated for 10^6 cycles at ~0.5 cycles/min (industrial HVAC) will fail in ~33 years under normal duty. But if the absorber flips it every 1–2 seconds during grid transients, the cycle rate jumps to 30–60 cycles/min, shortening lifespan to months.

### Solution: Temporal state constraint per asset

Each asset carries a `min_state_linger_s: u64` field in the profile, declaring the minimum duration between state changes.

- **Electronic assets** (battery, EV charger): `min_state_linger_s = 0` (no switching wear)
- **Mechanical assets** (heater relay, boiler, compressor): `min_state_linger_s = 30–60` (user-configurable per profile)
- **Multi-tier heaters** (0 / mid / full power): Each transition counts as a state change (e.g., 0→mid, mid→full, full→mid, mid→0 all trigger linger check).

The constraint applies to **all control paths** — absorber, dispatcher, thermostat emergency, user requests. When any layer tries to change an asset's control state, it must check:
```
time_since_last_state_change >= min_state_linger_s
```

If this check fails:
- **Absorber**: refuses the change and moves to the next asset in priority order.
- **Dispatcher/MILP plan**: the setpoint is not applied; asset remains in current state. (Rare, since MILP uses soft penalty to discourage frequent switching.)
- **Thermostat emergency**: **ignores linger and forces ON immediately** when `temp ≤ temp_min_c` (safety override).
- **User request**: deferred in queue; applied once linger window clears (or immediately if user has override permissions — TBD).

On successful state change, update the asset's `last_state_change_ts` in a global tracker.

### Enforcement across all layers

A global `StateChangeTracker: HashMap<String, DateTime<Utc>>` (in-memory, per tick) tracks `last_state_change_ts` for each asset. Each control layer independently checks linger before issuing a state change:

**Absorber**:
1. For each asset in priority order, check: `time_since_last_state_change >= min_state_linger_s`
2. If true: apply setpoint adjustment and update tracker
3. If false: skip this asset, move to next in priority order
4. If all assets locked or at flexibility bounds, residual deviation propagates to Tier 2 gate

**Dispatcher** (reads MILP plan):
1. Apply setpoint if `time_since_last_state_change >= min_state_linger_s`
2. Otherwise, keep asset in current state (setpoint ignored, rare since MILP soft penalty discourage frequent switching)

**Thermostat emergency**:
1. Check: `temp ≤ temp_min_c`?
2. If true: force heater to max power **immediately, bypassing linger check** (safety override)

**User requests** (future):
1. Queue request; check linger on execution
2. If linger blocks: defer execution (or allow user override — TBD)

This multi-layer design ensures no control path accidentally bypasses linger. The redundant checking is acceptable for safety.

**In-memory tracking note**: `StateChangeTracker` is not persisted across restarts. After boot, all relays are "fresh" (last_change_ts = boot time). This is acceptable because restarts are rare and the system operates under-constrained for the first ~60s after boot, which is safe.

### Interaction with MILP

**Chosen approach**: **Option A** (MILP ignores linger; absorber + dispatcher enforce at runtime).

The MILP computes plans assuming switches can flip freely, using the existing soft penalty `switching_penalty_eur` in the objective to discourage frequent switching. At runtime, the absorber and dispatcher enforce the hard linger constraint:

- If the MILP plan orders a state change within a linger window, the dispatcher skips it and keeps the asset in its current state. This is rare because the soft penalty already discourages rapid switching.
- If the absorber tries to absorb a deviation by changing a locked-in relay, it moves to the next asset instead.
- If both dispatcher and absorber hit linger blocks, the residual deviation propagates to Tier 2, triggering a replan.

This is pragmatic because:
- Linger is primarily a wear-reduction feature for production, not a control feature. The existing soft penalty already optimizes around switching cost.
- Option B (MILP-aware linger) adds solver complexity without proportional benefit.
- Option A is simple to implement and leaves room for escalation if data shows frequent Tier 2 replans.

**Test vs. production**: Test profile has `min_state_linger_s = 0` (no linger constraints, fast BDD), production profiles have `30–60` (strict linger for real relays).

### Profile schema

Each asset in the main profile adds `min_state_linger_s: u64` (seconds). This applies to **all control paths** — absorber, dispatcher, thermostat emergency, user requests — not just the absorber. In practice:

```yaml
assets:
  - type: battery
    id: battery
    battery_kwh: 60.0
    min_state_linger_s: 0                    # electronic, no wear concern
  - type: ev
    id: ev
    max_charge_kw: 7.4
    min_state_linger_s: 0                    # solid-state or high-cycle rated
  - type: heater
    id: heater
    max_kw: 5.0
    min_state_linger_s: 30                   # electromechanical relay: 30–60s typical
  - type: boiler
    id: boiler
    max_kw: 8.0
    min_state_linger_s: 60                   # boiler compressor: longer linger
```

**Test profile** (`test.yaml`): all `0` (fast BDD, no linger constraints).  
**Production profiles** (`ven-{1,2,3}.yaml`): `0` for electronics, `30–60` for mechanical relays/compressors.