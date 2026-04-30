# Deviation Control — Design Suggestions

> Status: idea / pre-spec  
> Context: VEN HEMS controller on `016-refactor-ven-backend`

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

2. **Return-to-plan ramp**: When deviation clears, how fast does the absorber ramp back to the MILP setpoint? Instant snap avoids lingering but can cause a brief counter-deviation. A 1–3 tick ramp is likely sufficient.

3. **EV absorber behaviour near departure**: If the EV is 30 minutes from departure and 10% below target SoC, the absorber should probably not reduce EV charging even if it would help absorb deviation. This requires the absorber to be aware of the EV session's urgency — possibly via the flexibility envelope urgency field or a simple time-to-departure check.

4. **Interaction with opportunistic EV**: The opportunistic EV overlay is already a persistent setpoint increase (not deviation-driven). The absorber needs to treat it as a fixed load (not an absorption target) when it's active, or the two can cancel each other.

5. **Profile schema**: Which fields are needed? Candidates: `absorber_enabled: bool`, `absorber_dead_band_kw: f64`, `absorber_assets: Vec<{id, priority, max_step_kw}>`.
