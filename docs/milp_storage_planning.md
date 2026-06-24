# MILP Planner: Generic Storage Optimisation and Plan Stability

> **Origin:** Investigation started with the ven-2 hot water tank running near T_min.
> The root causes and their fixes apply to all storage assets and the MILP architecture.
> See [milp_storage_planning_impl.md](milp_storage_planning_impl.md) for the implementation plan.

Observations captured 2026-06-18/19 on ven-2 (Commercial building — 2000 L hot water tank + 12 kW PV).

---

## Profile (ven-2.yaml — current baseline)

```
max_kw: 6.0    mid_kw: 3.0    temp_min_c: 40.0    temp_max_c: 80.0
volume_l: 2000  k_loss_kw_per_c: 0.003    draw_kw: 0.5
switching_penalty_eur: 0.50    phase2_epsilon_eur: 0.10
plan_step_s: 300 (5 min)    plan_horizon_h: 24
```

Derived thermal mass: `2000 × 4.186 / 3600 ≈ 2.326 kWh/°C`
Full tank capacity: `(80 − 40) × 2.326 = 93 kWh`
Time to fill at 6 kW: `93 / 6 ≈ 15.5 h` (>50% of the 24 h horizon)
Standing demand (at mid-range 60 °C, ambient 10 °C):
`0.5 + 0.003 × (60 − 10) = 0.65 kW` → cools at `0.65 / 2.326 ≈ 0.28 °C/h`

---

## Genericness as a Design Goal

The VEN software is designed to be generic: the same binary should handle any mix of
assets (heater, battery, EV, PV, base load) without per-deployment expert tuning of
planning horizon, step size, or objective coefficients. Asset-specific behaviour must
emerge from the physics encoded in the profile, not from manual calibration.

The problems described in this document expose gaps in that genericness: parameters that
should be auto-derived (horizon, terminal reward coefficient) are currently either fixed
at values that happen to work for small assets but fail for large ones, or left at zero
so the optimizer has no incentive to use available storage. The solutions proposed here
restore genericity by computing what was previously hand-tuned.

---

## Observed Plans

### Plan A — poor (captured 2026-06-18 18:18 UTC, heater OFF, temp ≈ 41.3 °C)

```
Slots   0 – 54   (18:18 → 22:53)  OFF     temp drifts 41.3 → 40.0 °C
Slots  55 – 66   (22:53 → 23:53)  3→6 kW  temp climbs to 41.8 °C
Slots  67 – 118  (23:53 → 04:13)  OFF     cools back to 40.6 °C
Slot  119         (04:13 → 04:18)  6 kW    single-slot pulse (5 min)   ← chattering
Slots 120 – 151  (04:18 → 06:58)  OFF     cools to 40.0 °C
Slots 152 – 161  (06:58 → 07:48)  3 kW    50 min, reaches 40.9 °C
Slots 190 – 235  (10:08 → 13:58)  3 kW    230 min solar window, reaches 44.1 °C
Slots 236 – 287  (13:58 → 18:13)  OFF     cools to 42.9 °C
```

Switches: 7.  Temperature range exploited: 40.0 – 44.1 °C  (<10% of available span).

### Plan B — good (captured 2026-06-19 13:39 UTC, heater ON mid-solar-block, temp ≈ 43.9 °C)

```
Slots   0 –  17  (13:39 → 15:09)  3 kW    finishing solar block, reaches 45.4 °C
Slots  18 – 245  (15:09 → 10:04)  OFF     clean coast for ~19 h, reaches 40.1 °C
Slots 246 – 287  (10:09 → 13:34)  3 kW    next-day solar block, reaches 43.5 °C
```

Switches: 2.  Temperature range exploited: 40.1 – 45.4 °C.  No fragmentation.

These two plans were produced by the **same optimizer with the same configuration**,
five hours apart. The difference is explained entirely by initial conditions, not by any
code change — see "Why Plan Quality Oscillates" below.

---

## Root Causes

### Root Cause 1 — No terminal value for stored heat (temperature ceiling)

The Phase 1 MILP minimises energy cost subject to constraints. The tank's state at the
end of the horizon — `e_tank[n-1]` — appears nowhere in the objective. Heat stored at
slot 287 is worth €0 to the solver. The optimizer fills just enough to stay above T_min
through the horizon and not one joule more.

The tariff differential (0.38 → 0.30 EUR/kWh, Δ = 0.08 EUR/kWh) does not justify
pre-filling: filling 93 kWh at the cheap rate saves `93 × 0.08 = 7.44 EUR` but requires
running the heater for 15.5 h — and only if that stored heat is actually consumed in the
same horizon, which is uncertain.

**Fix:** Add a terminal reward term to the Phase 1 objective (see "The Terminal Reward"
section below and Option 2).

---

### Root Cause 2 — Horizon shorter than the asset's characteristic timescale

The 2000 L tank takes 15.5 h to fill at 6 kW. The 24 h planning horizon contains at
most one full solar window (06:00–18:00 UTC). Depending on the time of capture:
- Captured mid-solar: solar window is near slot 0, next-day window is near slot 246 → good plan
- Captured post-solar (18:00 UTC): next solar window is at slots 190–235, just 4 h before
  horizon end → optimizer sees it as a terminal feature, fires overnight patches to bridge

The plan's quality oscillates with the time of day because the solar cycle does not fit
cleanly inside the 24 h window at all capture times. This is a structural mismatch, not a
tuning problem.

**Fix:** Extend to a 48 h horizon so that at least one full future solar window is always
visible from any capture time (see "Horizon and Resolution Trade-offs" and Option 3).

---

### Root Cause 3 — Epsilon/penalty incoherence (fragmentation)

Phase 2 minimises switching friction within `phase1_cost ≤ c_star + epsilon`. With:
- `switching_penalty_eur: 0.50` — each switch costs 0.50 EUR
- `phase2_epsilon_eur: 0.10` — Phase 2 may spend at most 0.10 EUR extra energy

Phase 2 can afford to eliminate `0.10 / 0.50 = 0.2` switches per plan before exhausting
its budget. Consolidating two separate 5-min pulses into one longer block requires extra
energy (e.g. 0.33 kWh × 0.30 EUR/kWh = 0.10 EUR); if that barely fits the budget,
Phase 2 cannot consolidate multiple fragments. The single-slot pulse at slot 119 (Plan A)
is a direct result.

The two parameters are internally inconsistent: the penalty says switching is expensive
but the epsilon says you can barely afford to reduce it.

**Fix:** Raise epsilon to at least 1× the switching penalty: `phase2_epsilon_eur: 1.00`
(see Option 4).

---

### Root Cause 4 — Acceptance gate imbalance (plan quality regression)

The gate in `services/planning.rs` compares:
```
improvement = (current.objective_eur + current.friction_eur)
            - (new.objective_eur    + new.friction_eur)
```

`friction_eur` variance is bounded by `phase2_epsilon_eur` (currently ≤ 0.10 EUR between
any two plans). `objective_eur` varies freely by 0.10–0.30+ EUR per replan as the
starting temperature and tariff window shift. A noisier new plan that found 0.15 EUR
cheaper energy but added two relay switches (+0.08 EUR friction) nets +0.07 EUR
improvement — the gate accepts it. The good plan is silently replaced by a worse one.

Once epsilon is raised to 1.00 EUR (Root Cause 3 fix), `friction_eur` variance grows
proportionally, partially reducing the imbalance. The structural fix requires a
switch-count guard on the gate.

**Fix:** Add switch-count surcharge to the acceptance gate (see Option 6).

---

### Root Cause 5 — Near-future chattering (plan instability)

Every 5 minutes a fresh MILP is solved from scratch. If accepted, the entire plan is
replaced — including the relay decision for the next 5 minutes. Small differences in
starting temperature, tariff window alignment, and MILP degeneracy cause the optimizer
to reach a different (but equally valid) near-future assignment on each cycle. The
hardware relay may be toggled on/off by one plan and then toggled back by the next.

**Fix:** Commit to the current on/off block (block commitment anchor, see Option 7).

---

## The Terminal Reward (c_terminal)

### What it is

Adding the term `−c_terminal × e_tank[n−1]` to the Phase 1 MILP objective makes the
optimizer treat stored heat at the horizon end as valuable. The coefficient `c_terminal`
is the forward value of 1 kWh stored — intuitively, the cost of having to buy that
energy later.

The sign is negative because we minimise: more stored energy reduces the objective.

### Auto-computation

The coefficient should be set so that:
- Pre-heating during **PV surplus** (marginal cost = lost export = 0.29 EUR/kWh) is
  always net-positive → `c_terminal > 0.29`
- Pre-heating during the **cheap overnight tariff** (effective cost = 0.30 + 0.22 =
  0.52 EUR/kWh, including the `c_ctrl_imp_malus`) is net-neutral → `c_terminal ≈ 0.52`
- Pre-heating during **peak grid** (effective = 0.38 + 0.22 = 0.60 EUR/kWh) remains
  net-negative → `c_terminal < 0.60`

The formula that satisfies all three conditions:

```
c_terminal = mean(c_imp_eur_kwh[t] for all t) + c_ctrl_imp_malus_eur_kwh
```

For ven-2 with tariffs oscillating between 0.30 and 0.38 EUR/kWh (mean ≈ 0.34):

```
c_terminal ≈ 0.34 + 0.22 = 0.56 EUR/kWh
```

This is **size-independent** (EUR/kWh, not EUR — scales naturally with any tank via
`e_tank`), requires **no profile parameter** (all inputs already exist in
`build_milp_inputs()`), and is **economically self-consistent**: the optimizer will
always fill during PV surplus, be indifferent about cheap overnight, and never buy
peak-rate energy purely to fill the tank.

The profile CAN override with an explicit `c_terminal_eur_kwh` value for calibration.
The default (omitted or 0.0) means auto-computation is active.

### c_terminal for the heater

Applied to `e_tank[n-1]` (tank energy above T_min at horizon end).

```
c_terminal_heater = mean(c_imp_eur_kwh) + c_ctrl_imp_malus_eur_kwh
```

Net gain during PV surplus: `0.56 − 0.29 = +0.27 EUR/kWh` — strong incentive to fill.
This directly fixes Root Cause 1 (temperature ceiling) and also incentivises the heater
to run at full tier (6 kW) rather than mid tier during solar, whenever PV covers the
extra load.

### c_terminal for the battery

The battery's terminal value is energy that can be discharged later to offset grid
import. However, the `c_ctrl_imp_malus` is a penalty on import, not on battery storage
value — it should NOT be included here.

```
c_terminal_battery = mean(c_imp_eur_kwh) × round_trip_efficiency
```

For ven-1's battery (`round_trip_efficiency: 0.92`):
```
c_terminal_battery ≈ 0.34 × 0.92 = 0.31 EUR/kWh
```

This applies to the battery's stored energy (`e_bat_kwh[n-1]`) in the Phase 1 objective,
incentivising the battery to end each horizon in a charged state when cheap energy was
available. The effect is smaller than for the heater (the home battery's 24 h cycle
already largely captures available arbitrage opportunities).

### c_terminal for the EV

The EV session has a deadline constraint (`e_ev[t_dead] ≥ e_target`) that already forces
the required charge to be delivered by departure time. Adding a terminal reward on top
would double-count the incentive and could cause the optimizer to over-charge at peak
rates near the horizon end.

**`c_terminal_ev = 0`** — the deadline mechanism is the correct and sufficient
incentive.

Exception: if a session deadline falls beyond the planning horizon (car still plugged in
at hour 48), a terminal reward of `mean(c_imp_eur_kwh)` for the remaining charge energy
is appropriate. This is a rare case and can be handled separately when it occurs.

### How c_terminal on 24h largely eliminates fragmentation as a side effect

The fragmentation in Plan A arises because the tank cools to T_min during the overnight
period and the optimizer fires small top-up pulses to prevent T_min violations. With
c_terminal filling the tank to 55–60 °C during each solar window:

```
Coast time from 55 °C to T_min (40 °C):
  (55 − 40) / 0.28 °C/h ≈ 53 h
```

The tank now stays above T_min for more than two full nights without any heating. The
overnight top-up patches simply cease to be necessary — the physics no longer require
them. Fragmentation disappears as a consequence of fixing the temperature ceiling, not
as a separate fix.

The "coast threshold" for the 16 h overnight gap (18:00 → 10:00):
```
Required starting temperature ≥ 40 + 16 × 0.28 = 44.5 °C
```
c_terminal fills to 55–60 °C → easily clears this threshold.

### Why 48 h still adds value despite c_terminal

c_terminal solves the temperature ceiling definitively and eliminates steady-state
fragmentation. However two scenarios remain where it cannot help alone:

**Cold-start / cloudy-day:** If the tank starts at T_min (40 °C) at 18:00 — because
the solar window was missed due to a cloudy day, maintenance, or first startup — the
tank will drop below T_min before the next solar window 16 h later. This is a
physics constraint: `16 × 0.28 = 4.5 °C` of cooling makes overnight top-ups
unavoidable. c_terminal cannot eliminate what the physics require.

**Complete phase-dependence elimination:** Even with c_terminal, when the tank is cold
at 18:00, some overnight patches appear in the plan. The 48 h horizon eliminates this
by showing the second solar window 34 h away — giving the optimizer a clear long-range
target and allowing it to plan a coherent coast strategy from any capture time.

The two fixes are **complementary, not substitutes**:
- c_terminal alone (24 h): solves steady-state; cold-start still fragments
- 48 h alone: stabilises plan structure; temperature ceiling remains at ~44 °C
- c_terminal + 48 h: solves all cases

---

## Why Plan Quality Oscillates with the Time of Day

### The locking effect of `initial_z_*`

When the heater is currently ON, the MILP context sets `initial_z_mid = 1.0`. The
switching constraint at slot 0 then penalises turning the heater off at the very next
slot by the full `switching_penalty_eur` (0.50 EUR). This locks the current ON block in
place: the solver cannot profitably interrupt a running heating session. Once the block's
natural end is reached, the next decision is made freely — but from a high-temperature
starting point.

When the heater is OFF at plan time, `initial_z_mid = 0.0`, and there is no locking.
Every slot is a free decision.

### The four conditions that produce a clean plan

1. **Heater currently ON.** `initial_z_mid = 1.0` locks the current block; the first
   switch in the plan is its natural block end, not an optimizer artefact.

2. **Tank temperature high enough to coast to the next solar window.** From 45 °C, the
   tank reaches T_min (40 °C) in about 18 h. The next solar window (10:00 next day,
   ~20 h away from 14:00) is just barely reachable — the coast works.

3. **The next solar window is the clearly dominant cheap opportunity.** The tank is warm
   enough to skip overnight tariff dips; no competing patches are needed.

4. **The 24 h horizon ends inside the next solar window.** Plan B ends at 13:34 next day,
   catching the full next-day solar block.

### The four conditions that produce a fragmented plan

1. **Heater currently OFF.** No locking; all slots are free decisions.

2. **Tank temperature near T_min (~40–41 °C).** Any 5-min slot with a tariff dip looks
   like a cheap opportunity to avoid the soft T_min penalty.

3. **Multiple sub-optimal cheap windows before the solar peak.** The overnight tariff
   (0.30 EUR/kWh) creates 0.08 EUR/kWh apparent savings, attracting small pulses that
   Phase 2 cannot consolidate within the 0.10 EUR epsilon.

4. **The solar window appears late in the horizon.** At 18:18 capture, the solar window
   is at slots 190–235 (hours 15–20 of 24). The optimizer treats it as an endpoint rather
   than an anchor, patching everything before it.

### Why this is a structural problem, not a tuning problem

The 24 h window rolls with real time. The plan's relationship to the solar cycle depends
entirely on when the plan is computed. Increasing the switching penalty or Phase 2 epsilon
reduces fragmentation but does not eliminate the phase-dependence, because the root cause
is structural: the horizon sometimes contains a clean forward path (mid-solar capture)
and sometimes does not (trough capture).

### Would planning at a fixed time of day help?

Computing a single daily plan at, say, 07:00 would produce a consistently clean plan.
But fixed-time planning eliminates all the responsiveness that makes a MILP planner
valuable: no DR response within the day, no adaptation to unexpected hot water draw, no
tariff update response, brittle single point of failure. It is a cron job with extra
steps.

### How c_terminal changes the picture

With c_terminal filling the tank to 55–60 °C after each solar window, the four
conditions for a fragmented plan collapse:

- **Condition 2** (tank near T_min) no longer holds — the tank is warm after every solar
  window and remains above T_min for 53+ hours without any heating.
- **Condition 3** (overnight patches attractive) disappears — with c_terminal ≈ 0.56
  EUR/kWh and overnight effective cost ≈ 0.52 EUR/kWh, overnight heating is net-neutral;
  the optimizer will not fire overnight pulses unless forced by a T_min constraint.
- **Condition 4** (solar window late in horizon) still exists but the optimizer now has
  a strong incentive (+0.27 EUR/kWh net gain) to fill aggressively during that window.

The 48 h horizon additionally eliminates Condition 4 by keeping the solar window central
rather than at the edge of the plan, at any capture time.

---

## Why the Planner Does Not Use All Available PV Energy

The power balance in the MILP is:

```
p_imp[t] + p_pv[t] + bat_discharge = p_base[t] + heater[t] + ev[t] + bat_charge + p_exp[t]
```

Running the heater during a PV surplus window shifts energy that would have been exported
(`p_exp[t]`) into self-consumed heat. The effective marginal cost is the **lost export
revenue** (0.29 EUR/kWh), not the import tariff.

### The profile's `c_ctrl_imp_malus_eur_kwh: 0.22`

This adds `0.22 EUR/kWh × p_imp[t]` to the Phase 1 objective, making grid import
significantly more expensive (0.30 + 0.22 = 0.52 EUR/kWh during the cheap window). The
heater running on PV costs 0.29 EUR/kWh (lost export); the heater on grid costs 0.52
EUR/kWh. The solver already prefers PV strongly.

The observed solar-window run (slots 190–235 in Plan A) confirms this. The problem is
not that the optimizer ignores PV — it is that without c_terminal there is no incentive
to heat **more** than the minimum to stay above T_min before the next tariff change.

### Why the heater runs at mid tier (3 kW) during solar

Phase 2 penalises the full tier over the mid tier (`w_tier_penalty_eur`) when both
achieve the same energy cost. Without a terminal reward, the extra kWh from the full
tier (6 kW) earns nothing at horizon end. With c_terminal ≈ 0.56 EUR/kWh and zero
import cost (PV surplus covers the heater), running at full tier has a net gain of
+0.27 EUR/kWh per extra kWh stored. The optimizer would prefer the full tier whenever
PV output covers the load, eliminating the mid-tier bias during solar windows.

### How c_terminal and 48 h together fix PV utilisation

With c_terminal: the optimizer fills the tank aggressively during any PV surplus —
the terminal reward (0.56 EUR/kWh) far exceeds the marginal cost (0.29 EUR/kWh). The
full 40–80 °C range becomes economically attractive.

With 48 h horizon: the second solar window anchors the strategy. The optimizer can see
that filling more in window 1 reduces the heating burden in window 2, validating the
pre-heating investment even without a terminal reward.

Together: the tank reaches near-T_max during solar windows and coasts through the
overnight period cleanly.

---

## Why a Better Previous Plan Gets Replaced

The acceptance gate in `services/planning.rs` compares:

```
improvement = (current.objective_eur + current.friction_eur)
            - (new.objective_eur    + new.friction_eur)
```

This is structurally imbalanced:

- **`friction_eur` is bounded by Phase 2 epsilon** (≤ 0.10 EUR variation between any
  two plans with the current config). A plan with twice as many switches costs at most
  ~0.10 EUR more in `friction_eur`.
- **`objective_eur` (Phase 1 energy cost) varies freely.** As real time advances 5 min,
  the tariff window shifts and Phase 1 finds a different optimum — typically differing by
  0.10–0.30+ EUR from the previous plan.

A new plan that found 0.15 EUR cheaper energy but added 2 extra relay switches
(+0.08 EUR friction) nets +0.07 EUR improvement. The gate accepts it.

Once epsilon is raised to 1.00 EUR (see Root Cause 3 fix), `friction_eur` can vary by up
to 1.00 EUR between plans. A noisier plan with 2 extra switches (+1.00 EUR friction)
must now show 1.00 EUR more Phase 1 improvement to win — a much higher bar. This
partially corrects the imbalance. The structural fix requires a switch-count guard.

The decay mechanism compounds the problem: after `decay_s` seconds, `effective_threshold`
falls to zero and any new plan replaces the current one unconditionally, regardless of
fragmentation.

---

## The Plan Stability Problem (Moving Time)

Every 5 minutes a fresh MILP is solved from scratch and — if the gate passes — the result
replaces the entire active plan, including slots that correspond to the very near future.
This creates three interlinked problems:

**1. Near-future chattering.** The first few plan slots change every replan cycle because
small differences in starting temperature, the exact tariff window in scope, and MILP
degeneracy all cause the solver to reach a different (but equally valid) near-future
assignment. The hardware relay may be toggled on/off by one plan and then toggled back
five minutes later by the next.

**2. Block-boundary drift.** A heating block planned to end at 23:00 may shift to 22:55
or 23:05 on the next replan. The effective end of the current heating session keeps
moving.

**3. Plan decay forces unconditional replacement.** Once the current plan is old enough
(governed by `plan_adoption_decay_s`), any new plan replaces it even if worse.

### When should a plan stop changing?

- **Physical commitment:** once the relay has been set, the decision is already executed.
- **Relay wear:** typical minimum dwell times are 30–60 s for thermal relays.
- **Thermal inertia:** with 2000 L and ~0.65 kW demand, a ±5 min shift in block
  end-time changes the final temperature by less than 0.1 °C.
- **Tariff granularity:** once the solver has chosen a tariff window, re-evaluating more
  frequently than the tariff period gains nothing.

A reasonable commitment window for this system is **1–2 hours**.

### How much of the near-future plan to preserve

The right unit is the current **on/off block** — the contiguous run of slots where the
heater is in the same state. Once a block has started executing:
- Keep it running (or off) until the planned block boundary.
- Allow replans to freely change everything beyond that boundary.
- Allow a hard trigger (DR event, temperature anomaly > threshold, manual override) to
  break the anchor early.

---

## Three-Zone Stability Model and VTN Flexibility Forecasting

### The three zones

Plan stability requirements differ by time distance from now. The right abstraction is
three zones with different stability expectations and different information needs:

**Zone 1 — Committed (0–30 min)**
The relay decision is imminent or already executing. This is a physical commitment, not
a forecast. The mechanism is the block commitment anchor (Option 7): once a block starts,
the MILP pins its binary variables for the duration of the block. Changes only by hard
trigger (DR curtailment, temperature anomaly > threshold, manual override).

**Zone 2 — Scheduled (30 min – 8 h)**
The next heating session is planned but not yet executing. With c_terminal and coherent
epsilon in place, this zone is statistically stable — consecutive replans produce nearly
identical block structures because the optimal strategy is dominated by the same physics
(fill during solar / cheap tariff, coast otherwise). The gate switch-count guard (Option 6)
prevents a noisier plan from winning unless it offers substantially cheaper energy. Timing
can still shift by ±30 min per replan; this is acceptable.

**Zone 3 — Strategic (8–48 h)**
The plan's slot-level timing in this zone is irrelevant: each slot will have been replanned
400+ times before it executes. What matters here is the asset's **available thermal
capacity** and how it is likely to be deployed. This is derived entirely from asset physics —
not from the MILP plan — and changes at 0.28 °C/h (tank cooling rate), not every 5 minutes.

### The key insight: total energy is stable, only timing shifts

Over any 24 h period, the heater must cover the standing thermal demand regardless of plan:

```
E_required ≈ q_dem × 24 h = 0.65 kW × 24 h ≈ 15.6 kWh
```

This is nearly constant. What the MILP optimises is *when* to buy those 15.6 kWh. The
VTN does not need to know the exact when — it needs to know the *how much* (total energy)
and the *degree of freedom* (can you shift it, by how far, for how long?). The degree of
freedom is also nearly constant — it is derived from thermal storage capacity, not from the
plan's timing.

### OpenADR 3.1.0 report types that match each zone

The OpenADR 3.1.0 User Guide (§§ 8.6, 8.7, 8.8) defines exactly the report payloads
needed for each zone. **The capacity envelope is not an invention — it is a defined
OpenADR 3 report type.**

| Zone | Time range | Information type | OpenADR §  | Payload type |
|------|-----------|-----------------|------------|-------------|
| Real-time state | now | Tank thermal SOC and power limits | §8.6 | `STORAGE_USABLE_CAPACITY`, `STORAGE_CHARGE_LEVEL`, `STORAGE_MAX_CHARGE_POWER` |
| Zone 1 committed | 0–30 min | Firm scheduled power profile | §8.8 | `USAGE` (near intervals, 5-min resolution) |
| Zone 2 scheduled | 30 min–8 h | Expected power profile, ±30 min tolerance | §8.8 | `USAGE` (forecast, aggregated to 1 h) |
| Zone 3 strategic | 8–48 h | Available thermal flexibility | §8.7 | `LOAD_SHED_DELTA_AVAILABLE`, `GENERATION_DELTA_AVAILABLE` |

#### §8.6 State of Charge — real-time asset state

The spec defines `STORAGE_USABLE_CAPACITY`, `STORAGE_CHARGE_LEVEL`,
`STORAGE_MAX_CHARGE_POWER`, and `STORAGE_MAX_DISCHARGE_POWER` for storage resources.
These map directly onto the hot water tank as thermal storage:

```
STORAGE_USABLE_CAPACITY  = e_max_kwh = (T_max − T_min) × thermal_mass  = 93 kWh
STORAGE_CHARGE_LEVEL     = (T_current − T_min) / (T_max − T_min) × 100 [%]
STORAGE_MAX_CHARGE_POWER = max_kw                                        = 6.0 kW
STORAGE_MAX_DISCHARGE_POWER = q_dem_kw (standing thermal demand)         = 0.65 kW
```

These values are derived directly from asset parameters and the live temperature
measurement. They do not depend on the MILP plan at all. Update rate: on temperature
change > 2 °C, or at most every minute.

#### §8.7 Capability Forecast — 48 h flexibility envelope

The spec explicitly defines this as: *"a rolling forecast of the aggregated load
flexibility so that the BL is aware of how much load shed or generation can be dispatched
**up to 48 hours in the future**."* The example event requests `LOAD_SHED_DELTA_AVAILABLE`
for 48 intervals of 1 h duration, refreshed hourly.

For the hot water tank, the available load shed at each future hour is the thermal energy
that could be absorbed pre-emptively (additional heating that avoids future consumption)
or deferred (delaying heating by drawing down the thermal buffer):

```
flex_up_kwh(t)   = (T_max − T_forecast(t)) × thermal_mass  [available pre-heating]
flex_down_h(t)   = (T_forecast(t) − T_min) × thermal_mass / q_dem  [hours of load deferral]
```

`T_forecast(t)` is the tank temperature at hour `t` under the planned schedule —
computed from the current MILP plan's `planned_state_by_asset["heater"]["temp_c"]`.
The key point: this changes slowly (tank cools at 0.28 °C/h) and the VTN sees a stable
1 h envelope regardless of 5-minute MILP replanning.

`GENERATION_DELTA_AVAILABLE` is not applicable to the hot water tank (it cannot export).
It would apply to a VEN with a battery capable of V2G discharge.

Update rate: every hour, or when the tank temperature changes > 2 °C from the last
reported value.

#### §8.8 Operational Forecast — plan-based expected consumption

The spec defines this as: *"forecast of the aggregated resource load utilization taking
into account planned price and event optimizations and other operational considerations."*
The example requests 24 intervals of 1 h duration.

This is the MILP plan converted to an hourly power profile for the VTN. The VEN aggregates
the MILP plan's 5-min slots into 1 h buckets:

```
USAGE[hour h] = mean(planned_kw_by_asset["heater"][slots in hour h])
```

This report changes only when the Zone 2 plan changes materially — defined as: a switch
appears or disappears in Zone 2, or an existing block boundary shifts by more than 30 min.
This decouples the VTN update rate from the 5-minute MILP replan rate. The VTN sees
at most one or two updates per day (when the morning solar block is first scheduled and
when it ends).

Update rate: when Zone 2 plan changes materially; otherwise hourly refresh of the same
forecast.

### Is this contradictory with MILP planning?

No — the three report types serve different time scales and require different sources.
The MILP plan is only needed for Zone 2 (near-term operational forecast). Zones 3 and the
SOC report use asset physics exclusively and are completely decoupled from planning.

The practical consequence: **the VTN's ability to plan DR events is not degraded by
MILP replanning noise.** The 48 h flexibility envelope (§8.7) changes slowly regardless
of how many times the MILP reruns. The VTN can rely on it for day-ahead scheduling. The
near-term operational forecast (§8.8) changes when the plan changes materially — which,
with c_terminal and 48h horizon in place, happens rarely (once per daily cycle, when the
solar heating block is scheduled or rescheduled).

### Mapping to the 3-tier MILP grid

The 3-tier grid's resolution (5/10/15 min) is finer than the OpenADR reporting intervals
(1 h for §8.7 and §8.8). Aggregating the MILP plan from fine resolution to hourly buckets
for the VTN reports does not lose any information the VTN needs. Plan timing uncertainty
of ±15 min (Zone C) disappears completely when summed into 1 h intervals.

This means the 3-tier grid's coarser far-horizon resolution is not just acceptable — it
is directly appropriate for the OpenADR 48 h capability forecast, which is a 1 h
resolution report.

---

## Horizon and Resolution Trade-offs

### Is 36 h sufficient?

The PV model uses UTC hours 06:00–18:00 (sinusoidal, peak at 12:00). For the planner
to always produce a Plan-B-quality result, it must see at least one full future solar
window at any capture time.

Worst-case capture: ~18:00 UTC (just after solar closes). Next window: 06:00–18:00 next
day — 12 h to open, 24 h to close.

| Horizon | Capture 18:00 UTC → ends | Next solar window (06:00–18:00) |
|---------|--------------------------|----------------------------------|
| 24 h    | 18:00 next day           | Not visible — already past       |
| 36 h    | 06:00 day+2              | Start only ✗                     |
| 40 h    | 10:00 day+2              | First 4 h ✗ (partial)            |
| 44 h    | 14:00 day+2              | First 8 h ✓ (partial)            |
| 48 h    | 18:00 day+2              | Full window ✓                    |

At a capture of 14:00 UTC:

| Horizon | Ends          | Next solar window         |
|---------|---------------|---------------------------|
| 36 h    | 02:00 day+2   | Misses entirely ✗         |
| 40 h    | 06:00 day+2   | Start only ✗              |
| 44 h    | 10:00 day+2   | First 4 h ✓ (partial)    |
| 48 h    | 14:00 day+2   | 8 of 12 h visible ✓       |

**36 h is insufficient** — fails for ~8 h of every 24 h cycle. 44 h guarantees solar-peak
visibility from any capture time. 48 h guarantees a full window.

### The solver cost of a longer horizon

The heater contributes 2 binary variables per slot (`z_heat_mid`, `z_heat_full`).

| Configuration                | Slots | Heater binaries | Relative size |
|------------------------------|-------|-----------------|---------------|
| 24 h × 5 min (current)       |  288  |   576           | 1.0×          |
| 36 h × 5 min                 |  432  |   864           | 1.5×          |
| 44 h × 5 min                 |  528  | 1 056           | 1.8×          |
| 48 h × 5 min                 |  576  | 1 152           | 2.0×          |
| 48 h × 10 min                |  288  |   576           | 1.0×          |
| 48 h × 15 min                |  192  |   384           | 0.7×          |
| 48 h × 3-tier (5/10/15 min)  |  288  |   576           | 1.0×          |

MILP branch-and-bound scaling is super-linear in binary count. On Pi4-Server (ARM64),
the current 24 h plan uses 18–60 s. A uniform 48 h × 5 min plan would likely time out.

### Option 3a: Uniform coarser step (48 h × 10 min) — interim

Change `plan_step_s: 600` and `plan_horizon_h: 48` in the profile. Slot count stays at
288; binary count is identical to today.

For a 2000 L tank (thermal time constant ~143 h), 10 min resolution costs at most
`0.65 kW × (10/60) h × 0.38 EUR/kWh ≈ 0.04 EUR` in suboptimal timing.

**Code change required:** none.
**Risk:** EV deadline precision degrades to ±10 min; `plan_step_s` must divide all
tariff and capacity change periods cleanly.

### Option 3b: 3-Tier Variable-Step Grid — target architecture

Three zones with different step widths, totalling 288 slots and 48 h:

```
Zone A (0 –  8 h):  96 slots × 5 min  — near-future relay control (current precision)
Zone B (8 – 24 h):  96 slots × 10 min — overnight and next-morning scheduling
Zone C (24 – 48 h): 96 slots × 15 min — inter-day thermal strategy
```

**All assets are planned in all zones.** Every asset has MILP variables in all 288 slots
across all three zones. The power balance constraint applies at every slot. "Zone A" for
the home battery does not mean the battery has no variables in Zone C — it means Zone A
provides the precision that matters most for that asset's near-future execution. Zone B
and C produce complete plans for all assets; only the timing precision of block
boundaries is coarser.

| Asset | Characteristic time | Zone for precise decisions | Coarser zones acceptable? |
|-------|--------------------|-----------------------------|---------------------------|
| Battery (10 kWh, 5 kW) | 2 h | Zone A | Zone B (±10 min on 2h cycle) ✓ |
| EV departure in 6 h | — | Zone A | — |
| EV departure in 14 h | — | Zone B | ±10 min ✓ |
| Small heater (200 L, 35 min fill) | 35 min | Zone A (7 slots) | Zone B (3–4 slots) ✓ |
| Large heater (2000 L, 15.5 h fill) | 15.5 h | A + B + C | 15 min ≪ 15.5 h fill time ✓ |

**Why this is generic:** The same grid works correctly for any asset mix without
per-VEN tuning. Adding a large thermal store to any VEN immediately gains 48 h
visibility. A newly added EV retains 5 min near-future precision.

**Switching penalty must scale by `dt_h[t]`.** A switch in Zone C covers 15 min of
thermal commitment; a switch in Zone A covers 5 min. Both currently cost the same 0.50
EUR penalty. Without correction, Phase 2 is effectively 3× more willing to place block
boundaries in Zone C than in Zone A, and will tend to push transitions toward zone
boundaries. Fix:

```
// In HeaterMilpContext::objective(), Phase 2 switching term:
obj += lambda_sw_eur * dt_h[t] * sw[t]  // scaled by slot width
```

This makes each switch cost proportional to the time it commits — physically correct and
eliminates the zone-boundary artefact.

**Code change required:** `MilpInputs.dt_h: f64 → Vec<f64>` throughout the MILP module
(~10 files: `inputs.rs`, `solver_phase1.rs`, `solver_phase2.rs`, `heater.rs`, battery,
EV asset contexts, `results.rs`). Self-contained refactor; no interface changes outside
`milp_planner`.

**Pros:**
- Same solver cost as today (288 slots, 576 heater binaries)
- Full 48 h visibility — phase-dependence eliminated universally
- Full 5 min precision in Zone A — EV deadline and small heater unaffected
- Generic: no per-VEN horizon or step-size tuning
- Progressive precision matches forecast confidence (Zone C inputs are inherently rough)

**Cons:**
- `dt_h → Vec<f64>` refactor is significant (~10 files, all tests using the `9n`
  constraint count formula need updating)
- EV deadlines in Zone C (>24 h) lose up to ±15 min precision
- Zone-boundary switching artefact must be fixed with `dt_h[t]` scaling (additional
  change)
- Tier boundaries (8/16/24 h) are fixed constants, not derived from assets

### Option 3c: Two-stage coarse/fine solve — fallback

Solve a coarse 48 h problem (96 × 30 min slots) to determine block placement, then a
fine 8 h problem (96 × 5 min) constrained to agree with the coarse block boundaries.

**Pros:** Both solves stay well within the 60 s timeout; full 48 h visibility; 5 min
near-future precision.
**Cons:** Complex inter-stage protocol; coarse block boundaries may misalign with fine-
grained optimal timing; new orchestration code in `tasks/planning.rs`.

### Recommendation for horizon extension

**Start with Option 3a** (profile-only, benchmark solver time on Pi4 immediately). If
`solver_ms` stays under 40 s, this is a complete near-term solution. Implement
c_terminal (Option 2) first — it is simpler and fixes the temperature ceiling on the
existing 24 h horizon.

**Proceed to Option 3b** (3-tier grid) as the long-term generic architecture. The
`dt_h → Vec<f64>` refactor is the correct design regardless of tier count.

---

## Options

### Option 1: Set a daily heater target — workaround only
`POST /heater-target` with `target_temp_c: 70.0, ready_by: <tomorrow 06:00>`.

Switches mode from `MayRun` to `MustRun` with a deadline. Forces one large pre-heating
block at the cheapest available window.

**Pros:** No code change; immediate effect.
**Cons:** Requires daily external trigger. Superseded by Option 2 once implemented.

---

### Option 2: Auto-computed terminal energy reward (c_terminal)
Add `−c_terminal × e_storage[n−1]` to the Phase 1 MILP objective for storage assets.

**Formula (auto-computed, no profile parameter required):**
```
c_terminal_heater  = mean(c_imp_eur_kwh) + c_ctrl_imp_malus_eur_kwh
c_terminal_battery = mean(c_imp_eur_kwh) × round_trip_efficiency
c_terminal_ev      = 0.0  (deadline constraint handles incentive)
```

All inputs are already available in `build_milp_inputs()`. The coefficient is
size-independent and economically correct: solar filling always net-positive (+0.27
EUR/kWh), cheap-overnight net-neutral, peak net-negative.

The profile may supply an explicit override `c_terminal_eur_kwh` if calibration is
needed (e.g. `c_terminal_eur_kwh: 0.0` to disable).

**What it fixes:** Root Cause 1 (temperature ceiling) definitively. Also largely
eliminates Root Cause 2 fragmentation in steady-state (fuller tank coasts 53+ h without
top-ups).

**Pros:** Economically correct and size-independent; auto-computed = generic; no profile
tuning; fixes PV utilisation and pushes the heater toward full tier during solar surplus.

**Cons:** Small end-of-horizon bias: optimizer tends toward a fuller tank in the last few
slots, which may slightly distort decisions near the horizon boundary. May conflict with
DR events that want the tank cooled (e.g. grid reduction events); hard trigger should
clear the terminal reward target in those cases.

---

### Option 3: 48 h horizon with 3-tier variable-step grid
See "Horizon and Resolution Trade-offs" above for the full analysis.

- **3a (interim, profile-only):** `plan_step_s: 600`, `plan_horizon_h: 48` — 288 slots,
  same binary count, benchmark Pi4 solver time first.
- **3b (target):** 3-tier grid Zone A/B/C as described above, with `dt_h[t]` switching
  penalty scaling. The correct long-term generic architecture.
- **3c (fallback):** Two-stage coarse/fine solve — only if 3b cannot meet the timeout.

36 h is not sufficient. 44 h guarantees solar-peak visibility. 48 h guarantees a full
window.

**What it fixes:** Root Cause 2 (phase-dependence) fully, and residual Root Cause 2
fragmentation on cold-start / cloudy days that c_terminal alone cannot prevent.

**Important:** Option 3 does **not** fix the temperature ceiling (Root Cause 1). Even
with two solar windows visible, the PV tariff differential between the two windows is
near-zero, so the optimizer remains nearly indifferent between heating in window 1 vs
window 2. c_terminal (Option 2) is needed to break that indifference.

**Pros:** Eliminates phase-dependence universally; plan quality becomes time-of-day
independent; the 3-tier grid is generic (no per-VEN tuning); no solver cost increase.

**Cons:** 3b requires the `dt_h → Vec<f64>` refactor plus switching penalty scaling;
EV deadlines in Zone C lose ±15 min precision.

---

### Option 4: Raise Phase 2 epsilon (fix Root Cause 3)
Raise `phase2_epsilon_eur` from 0.10 to 1.00 EUR (= 2× `switching_penalty_eur: 0.50`).

Phase 2 can now afford to eliminate up to `1.00 / 0.50 = 2` extra switches per plan
while spending up to 1.00 EUR more on energy to consolidate fragments.

Also partially reduces the acceptance gate imbalance (Root Cause 4): `friction_eur`
variation now reaches up to 1.00 EUR, making it harder for a noisier plan to win purely
on marginal energy cost improvement.

**Pros:** Profile-only change; directly eliminates single-slot pulses and small fragments.
**Cons:** Does not fix temperature ceiling (Root Cause 1) or phase-dependence (Root
Cause 2). May over-consolidate in non-monotonic tariff scenarios. Validate with E2E.

---

### Option 5: Minimum-on-time constraint
Add a MILP "minimum up time" constraint using auxiliary start-indicator binaries:

```
y_start[t] ≥ on[t] − on[t−1]
∑(on[t..t+min_on−1]) ≥ min_on × y_start[t]
```

**Pros:** Directly enforces large blocks; eliminates single-slot pulses.
**Cons:** Adds MILP variables and big-M constraints. Superseded by Options 2 + 3 + 4
once those are in place — no longer needed.

---

### Option 6: Gate switch-count guard (fix Root Cause 4)
Add a switch-count surcharge to `evaluate_acceptance_gate` in `services/planning.rs`.
A periodic replan with more heater switches than the current plan must show additional
improvement of `extra_switches × gate_switch_penalty_eur`.

New profile parameter: `gate_switch_penalty_eur` (default 0.0 = disabled; set to
`switching_penalty_eur` value to make consistent).

**Pros:** Structurally closes the gate imbalance; prevents quality regression across
replans even at plan decay time (note: `fully_decayed` still bypasses the surcharge as
an escape hatch for stale plans).
**Cons:** Switch counting requires iterating plan slots on each gate evaluation; new
tuning parameter.

---

### Option 7: Block commitment anchor (fix Root Cause 5)
After plan adoption, store `anchor_until: Option<DateTime<Utc>>` in `AppState` pointing
to the end of the first on/off block. On subsequent replans, pin `z_heat_mid[t]` and
`z_heat_full[t]` to the current plan's values for all slots within the anchor window.
Hard triggers (DR event, temperature anomaly, manual override) clear the anchor.

**Pros:** Eliminates near-future chattering and block-boundary drift; orthogonal to all
other options; no change to the MILP formulation or acceptance gate.
**Cons:** Requires anchor state in `AppState`; divergence threshold for anomaly detection
needs tuning.

---

### Option 8: Two-zone rolling commitment (MPC)
Divide the horizon into a **committed zone** (first 2 h) drawn from the active plan and
never changed by periodic replans, and an **optimisable zone** (remainder) re-solved on
each cycle. The committed zone is only updated by hard triggers.

**Pros:** Maximum operational stability; hardware sees a stable 2 h schedule.
**Cons:** Most significant architectural change; requires splitting the MILP solve and
managing zone boundaries. Defer until Option 7 is evaluated.

---

## Critical Assessment: What Each Fix Actually Solves

### c_terminal alone (on existing 24 h horizon)

| Root Cause | Fixed? |
|---|---|
| 1 — Temperature ceiling | ✓ Definitively |
| 2 — Phase-dependence (steady-state) | ✓ As side effect (warm tank coasts through overnight) |
| 2 — Phase-dependence (cold-start) | ✗ Physics still require overnight top-ups if tank starts at T_min |
| 3 — Epsilon/penalty incoherence | ✗ Separate issue |
| 4 — Gate imbalance | ✗ Separate issue |
| 5 — Near-future chattering | ✗ Separate issue |

### 48 h 3-tier grid alone (without c_terminal)

| Root Cause | Fixed? |
|---|---|
| 1 — Temperature ceiling | ✗ 48 h does NOT fix this. PV tariff differential between two windows is ~0 EUR/kWh; optimizer remains nearly indifferent, still lazy-fills to ~44 °C |
| 2 — Phase-dependence | ✓ Definitively — second solar window always visible |
| 3 — Epsilon/penalty incoherence | ✗ Separate issue |
| 4 — Gate imbalance | ✗ Separate issue |
| 5 — Near-future chattering | ✗ Separate issue |

### c_terminal + 48 h 3-tier grid together

| Root Cause | Fixed? |
|---|---|
| 1 — Temperature ceiling | ✓ |
| 2 — Phase-dependence (all cases including cold-start) | ✓ |
| 3 — Epsilon/penalty incoherence | Still needs Option 4 |
| 4 — Gate imbalance | Still needs Option 6 |
| 5 — Near-future chattering | Still needs Option 7 |

### The minimum necessary combination

To address all five root causes:

| Priority | Fix | Root Cause | Effort |
|---|---|---|---|
| 1 | Option 4: raise epsilon to 1.00 EUR | 3 | Profile-only |
| 2 | Option 2: auto-computed c_terminal | 1 + 2 (partial) | Small code |
| 3 | Option 3a: 48 h × 10 min (benchmark) | 2 | Profile-only |
| 4 | Option 3b: 3-tier grid + dt_h scaling | 2 (complete) | Bounded refactor |
| 5 | Option 7: block anchor | 5 | Medium code |
| 5 | Option 6: gate guard | 4 | Small code |

---

## Recommended Path

### Priority 1 — Profile only, immediate

Raise `phase2_epsilon_eur` to `1.00` in `ven-2.yaml`. This costs nothing in solver
time, directly reduces fragmentation, and partially corrects the gate imbalance. Deploy
and observe overnight. No code change.

### Priority 2 — Small code change

Implement auto-computed c_terminal for heater and battery:
- `build_milp_inputs()` — compute `avg_imp = mean(c_imp_eur_kwh)`; pass to asset contexts
- `HeaterMilpContext::objective()` — add `−c_terminal × e_tank[n-1]` in Phase 1 mode
- Battery equivalent using `avg_imp × round_trip_efficiency`

Test-first: unit test that terminal reward raises `e_tank[n-1]` vs baseline at same
Phase 1 cost. Observable in the live plan within one solar cycle.

### Priority 3 — Profile only, benchmark

Implement Option 3a (`plan_step_s: 600`, `plan_horizon_h: 48`) and benchmark solver
time on Pi4. Target: < 40 s. If met, keep as a complete near-term fix for
phase-dependence. Monitor EV deadline precision.

### Priority 4 — Bounded refactor

Implement Option 3b (3-tier grid): `dt_h: f64 → Vec<f64>` throughout the MILP module,
plus `dt_h[t]` scaling for the switching penalty. This is the correct long-term generic
architecture and replaces Option 3a once complete.

### Priority 5 — Medium code changes (independent)

Option 7 (block anchor) and Option 6 (gate guard) are independent of each other and of
Priorities 1–4. Implement in either order after Priority 2 is deployed and confirmed.

### What can be deferred

- **Option 1** (daily target): workaround superseded by Priority 2.
- **Option 5** (minimum-on-time): superseded by Priorities 1–4 combined.
- **Option 8** (two-zone MPC): defer until Option 7 is evaluated — Option 7 may be
  sufficient for plan stability.
- **Option 3c** (two-stage solve): fallback only if Option 3b cannot meet the Pi4
  solver timeout.

---

See **[milp_storage_planning_impl.md](milp_storage_planning_impl.md)** for the
step-by-step implementation plan with sub-tasks, exact file locations, code sketches,
and full test impact analysis for each step.
