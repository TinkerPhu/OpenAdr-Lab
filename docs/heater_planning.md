# Heater Planning Analysis

Captured 2026-06-18 on ven-2 (Commercial building — 2000 L hot water tank + 12 kW PV).

---

## Profile (ven-2.yaml)

```
max_kw: 6.0    mid_kw: 3.0    temp_min_c: 40.0    temp_max_c: 80.0
volume_l: 2000  k_loss_kw_per_c: 0.003    draw_kw: 0.5
switching_penalty_eur: 0.50    phase2_epsilon_eur: 0.10
planning_horizon: 24 h (288 × 5 min slots)
```

Derived thermal mass: `2000 × 4.186 / 3600 ≈ 2.326 kWh/°C`
Full tank capacity: `(80 − 40) × 2.326 = 93 kWh`
Time to fill at 6 kW: `93 / 6 ≈ 15.5 h`  (>50% of the 24 h horizon)

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

---

## Why the Temperature Stays Near the Floor

### Root cause: no terminal value for stored heat

In `MayRun` mode without a deadline (`/heater-target` not set), the MILP has exactly one
incentive to run the heater: avoid paying a higher tariff later than the current tariff.
There is **no reward** for having a full tank at the end of the 24 h horizon — heat stored
at slot 287 is worth €0 to the solver. The dominant strategy becomes: fire small
just-in-time pulses near T_min at the cheapest available moment.

### Small tariff differential relative to storage cost

The observed tariff swing is 0.38 → 0.30 EUR/kWh (Δ = 0.08 EUR/kWh). Filling 93 kWh at
the cheap rate instead of the expensive rate would save `93 × 0.08 = 7.44 EUR`. But doing
so requires running the heater for 15.5 h — more than half the planning horizon — and the
solver has no visibility into whether that stored heat will ever be used (no terminal value).
The math does not justify pre-filling.

### Phase 2 epsilon too small to consolidate

Phase 2 minimizes switching within `phase1_cost ≤ c_star + 0.10 EUR`. Consolidating two
separate 5-min pulses into one longer block may require heating slightly more energy (e.g.
0.15 kWh × 0.30 EUR/kWh = 0.045 EUR extra). When that cost plus other rounding exceeds
0.10 EUR, Phase 2 cannot consolidate and the fragments survive into the final plan.

### Single-slot pulse (slot 119)

Phase 1 finds that a single cheap-tariff slot (04:13) is marginally optimal. Phase 2 cannot
merge it with the surrounding zero-cost blocks because doing so would exceed the 0.10 EUR
epsilon. The pulse appears in the plan as-is.

---

## Why Plan Quality Oscillates with the Time of Day

The two plans above were produced by the **same optimizer with the same configuration**,
five hours apart. The difference is entirely explained by the initial conditions at plan
time — not by any change to the code.

### The locking effect of `initial_z_*`

When the heater is currently ON, the MILP context sets `initial_z_mid = 1.0`. The switching
constraint at slot 0 then penalises turning the heater off at the very next slot by the full
`switching_penalty_eur` (0.50 EUR). This locks the current ON block in place: the solver
cannot profitably interrupt a running heating session. Once the block's natural end is
reached, the next decision is made freely — but from a high-temperature starting point.

When the heater is OFF at plan time, `initial_z_mid = 0.0`, and there is no locking. Every
slot is a free decision. The solver must choose from scratch when (and how briefly) to heat,
guided only by tariff differences and the soft T_min penalty.

### The four conditions that produce a clean plan

1. **Heater currently ON.** `initial_z_mid = 1.0` locks the current block; the first
   switch in the plan is its natural block end, not an optimizer artefact.

2. **Tank temperature high enough to coast to the next solar window.** With 2.326 kWh/°C
   and ~0.65 kW standing demand, the tank loses ~0.028 °C per 5-min slot. From 45 °C, it
   reaches 40 °C in about `5 / 0.028 ≈ 179 slots = 14.9 h`. The next solar window (10:00
   next day, roughly 20 h away from 14:00) is just barely reachable. The coast works;
   no intermediate top-up pulses are needed.

3. **The next solar window is the clearly dominant cheap opportunity.** When captured
   mid-solar-block, the plan contains exactly one future cheap window (tomorrow's solar
   peak). There are no competing overnight tariff dips that could tempt the solver into
   adding a small top-up pulse, because the tank is warm enough to skip them.

4. **The 24 h horizon ends inside, not before, the next solar window.** Plan B ends at
   13:34 next day, catching the full next-day solar block. The solver can schedule a
   clean second block because the window is fully visible.

### The four conditions that produce a fragmented plan

1. **Heater currently OFF.** No locking; all near-future slots are free decisions.

2. **Tank temperature near T_min (~40–41 °C).** The thermal margin is tiny. Any 5-min
   slot with a tariff dip looks like a cheap opportunity to avoid the soft T_min penalty.
   The solver fires small pulses at each dip rather than consolidating.

3. **Multiple sub-optimal cheap windows before the solar peak.** The overnight tariff is
   0.30 EUR/kWh (vs 0.38 EUR peak). Each overnight slot looks like a 0.08 EUR/kWh saving.
   With a 0.10 EUR Phase 2 epsilon, Phase 2 cannot always consolidate these sub-optimal
   pulses into one block — they survive as fragments.

4. **The solar window appears late in the horizon.** Captured at 18:18 UTC, the solar
   window is at hours 15–20 of the 24 h plan (slot 190–235). The solver sees it as the
   endpoint, not the anchor. Everything before it is open for fragmented patches.

### Why this is the 24 h horizon problem, not a tuning problem

The core issue is that **the 24 h window rolls with real time**, so the plan's relationship
to the solar cycle depends entirely on when the plan happens to be computed. At 13:00 the
plan looks almost identical to Plan B because the solar block is slot 0 and the next-day
solar block is slot 246. At 18:00 the solar block is slot 190 and the plan looks like Plan A.

Increasing the switching penalty or Phase 2 epsilon reduces fragmentation but does not
eliminate the phase-dependence, because the root cause is structural: the horizon sometimes
contains a clean forward path (mid-solar capture) and sometimes does not (trough capture).

### Would planning at a fixed time of day help?

Computing a single daily plan at, say, 07:00 (before the solar window) would produce a
consistently clean plan: one solar block clearly visible within the next 3–7 h, one
overnight coast to the next day's window. The plan would look like Plan B every day.

However, fixed-time planning has severe costs:
- **No DR response.** A VTN demand-response event arriving at 14:00 cannot be incorporated
  until the next planned replan at 07:00 the following day.
- **No consumption adaptation.** Unexpected hot water draw that drains the tank below 40 °C
  triggers the emergency thermostat, not a planned response.
- **No tariff update response.** Dynamic tariff changes within the day are ignored.
- **Brittle.** A single solver failure at 07:00 means no plan until tomorrow.

Fixed-time planning trades away all the responsiveness that makes a MILP planner valuable.
It is essentially a cron job with extra steps.

### What a 48 h horizon changes

A 48 h horizon makes the plan's quality **independent of the time of day it is computed**:

- At any capture time, the horizon contains **at least two solar windows**.
- The solver can see that heating during the first solar window produces stored heat that
  defers consumption across the following evening and night, until the second solar window
  refills the tank.
- The inter-day thermal battery pattern (fill → coast → fill) becomes the dominant strategy
  at all times of day, not just when the heater happens to be ON at capture time.
- The fragmented overnight patches disappear because the solver no longer needs them: the
  tank is warm enough from the first solar window to reach the second without intermediate
  top-ups, and the solver can verify this across the full 48 h coast.

The phase-dependence is not fully eliminated — if the tank is at 40 °C and the next solar
window is 20 h away, the planner still cannot profitably bridge the gap without some
interim heating — but the worst-case fragmentation (Plan A) no longer occurs because the
second solar window gives the solver a clear long-range target to optimise toward.

In short: a 48 h horizon converts the solar window from a **local feature** (sometimes
visible, sometimes not, sometimes at the edge) into a **structural anchor** that is always
present and always dominant. This is why Option 3 (horizon extension) addresses the
fragmentation problem at its root, where increasing Phase 2 epsilon only treats its
symptoms.

---

## Why the Planner Does Not Use All Available PV Energy

The power balance in the MILP is:

```
p_imp[t] + p_pv[t] + bat_discharge = p_base[t] + heater[t] + ev[t] + bat_charge + p_exp[t]
```

Running the heater during a PV surplus window shifts energy that would have been exported
(`p_exp[t]`) into self-consumed heat. The effective marginal cost of that energy is the
**lost export revenue** (0.29 EUR/kWh), not the import tariff. This is genuinely cheap —
but it is not zero, and, crucially, it still produces no terminal value.

### The profile's `c_ctrl_imp_malus_eur_kwh: 0.22` adds to the picture

This parameter adds `0.22 EUR/kWh × p_imp[t]` to the Phase 1 objective on every slot,
making grid import significantly more expensive (0.30 + 0.22 = 0.52 EUR/kWh during the
cheap window). That strongly incentivises self-consumption: the heater running on PV costs
0.29 EUR/kWh (lost export); the heater importing grid power costs 0.52 EUR/kWh. The solver
prefers PV by a wide margin whenever PV is available.

The observed slots 190–235 (10:08–13:58) confirm this: the heater does run during the solar
window. The problem is that it runs at **3 kW (mid tier)**, not 6 kW. With a 12 kW PV
array, there is likely surplus beyond the base load, so why not 6 kW?

- Phase 2 penalises the full tier over the mid tier (`w_tier_penalty_eur` in Phase 2
  weights) when both tiers achieve the same energy cost. The mid tier is preferred unless
  the extra kWh from the full tier produce a clear saving.
- More importantly: the heater has no incentive to use MORE energy than needed to keep the
  tank above T_min before the next tariff change. Heating to 70 °C during solar hours costs
  real money (even at 0.29 EUR/kWh marginal) and the extra stored heat is worth €0 at
  horizon end.

### Why a 48 h horizon changes this

With a 24 h horizon, the plan starting at 18:18 can see one solar window (the next day's
10:00–14:00). The tank heated to 44 °C during that window will cool back toward 40 °C by
the following evening — well before the horizon ends — but there is no second solar window
visible to reward pre-heating further.

With a 48 h horizon, the solver sees **two solar windows** and **two tariff cycles**. It can
charge the tank aggressively during the first solar window, knowing the stored heat defers
the need to buy expensive grid power during the first evening peak, and that the second
day's solar window provides a second charging opportunity. The thermal battery analogy
becomes fully visible: the 93 kWh storage capacity is large enough to serve as a meaningful
inter-day buffer, but only if the planner can see multiple days.

Concretely: to fill the tank from 40 °C to 80 °C takes 15.5 h at 6 kW. A 48 h horizon
gives the solver enough room to spread that filling across two solar windows (each ~4 h at
peak, ~7 h total) while still seeing the economic payoff on the other side.

**Practical constraint for 48 h:** The MILP problem size doubles. At 5-min resolution the
binary variable count goes from ~576 to ~1152 (for the heater alone). Solver time must be
benchmarked on Pi4-Server against the 60 s timeout. A practical compromise is to combine
a longer horizon with coarser resolution for the far half (e.g., 5 min for slots 0–144,
15 min for slots 145–240), but that requires variable-step support in the MILP inputs.

---

## Why a Better Previous Plan Gets Replaced

The acceptance gate in `services/planning.rs` compares:

```
improvement = (current.objective_eur + current.friction_eur)
            - (new.objective_eur    + new.friction_eur)
```

This is structurally imbalanced:

- **`friction_eur` is bounded by Phase 2 epsilon** (`≤ 0.10 EUR` variation between any two
  plans). A plan with twice as many switches costs at most ~0.10 EUR more in `friction_eur`.
- **`objective_eur` (Phase 1 energy cost) varies freely.** As real time advances 5 min, the
  tariff window shifts, starting temperature changes, and Phase 1 finds a different optimum
  — typically differing by 0.10–0.30+ EUR from the previous plan.

A new plan that found 0.15 EUR cheaper energy but added 2 extra relay switches (+0.08 EUR
friction) nets +0.07 EUR improvement. The gate accepts it — correctly by its own metric,
but from an operational standpoint the schedule degraded.

The decay mechanism compounds this: after `decay_s` seconds, `effective_threshold` falls to
zero and *any* new plan replaces the current one unconditionally, regardless of quality.

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

**2. Block-boundary drift.** A heating block that was planned to end at 23:00 may shift to
22:55 or 23:05 on the next replan. The effective end of the current heating session keeps
moving, making it impossible to commit to a definite run duration.

**3. Plan decay forces unconditional replacement.** Once the current plan is old enough
(governed by `plan_adoption_decay_s`), any new plan replaces it even if worse. This means
a well-structured plan computed an hour ago can be destroyed by a noisier plan at decay
time.

### When should a plan stop changing?

There is no single right answer, but the relevant signals are:

- **Physical commitment boundary:** once the relay has been set, the decision for the
  current slot is already executed. There is no benefit to changing it retroactively.
- **Relay wear threshold:** relay datasheets typically specify minimum on/off dwell times
  (commonly 30–60 s for thermal relays, longer for contactors). Switching more frequently
  than that is harmful regardless of the economic argument.
- **Thermal inertia:** with 2000 L and ~0.65 kW standing demand, the tank takes ~143 h to
  drain fully. A ±5 min change to a heating block end-time changes the final temperature by
  less than 0.1 °C. The optimisation gain is negligible relative to the schedule disruption.
- **Tariff granularity:** tariffs change on hourly or sub-hourly boundaries. Once the solver
  has identified which tariff window to use for a heating block, there is no reason to
  re-evaluate the boundaries of that block more frequently than the tariff changes.

A reasonable commitment window for this system is **1–2 hours** — long enough to span a
tariff period and absorb thermal uncertainty, short enough to respond to an unexpected DR
event or a large deviation from the forecast.

### How much of the near-future plan to preserve

The right amount to keep is the current **on/off block** — the contiguous run of slots in
the active plan where the heater is in the same state (ON or OFF). Once a block has started
executing:

- Keep it running (or off) until the planned block boundary.
- Allow the replan to freely change everything beyond that boundary.
- Allow a hard trigger (new DR event, temperature anomaly > threshold, manual override) to
  break the anchor early.

This avoids mid-block interruptions while still letting the solver adapt future blocks to
changing conditions.

---

## Horizon and Resolution Trade-offs

### Is 36 h sufficient?

The PV model uses UTC hours 06:00–18:00 (sinusoidal, peak at 12:00). For the planner to
produce a Plan-B-quality result from any capture time, it must **always see at least one
full future solar window** inside the horizon.

Worst-case capture time is just after the solar window closes: ~18:00 UTC. The next solar
window opens at 06:00 UTC the following day — 12 h later — and closes at 18:00 UTC — 24 h
later. For the full window to be visible, the horizon must reach at least 24 h past 18:00,
i.e. 18:00 + 24 h = 18:00 next day → **42 h minimum** from the worst-case capture.

| Horizon | Capture 18:00 UTC → horizon ends | Next solar window (06:00–18:00) |
|---------|-----------------------------------|---------------------------------|
| 24 h    | 18:00 (same day+1)                | Not visible — already past      |
| 36 h    | 06:00 (day+2) — window just starts| Start visible, rest cut off ✗   |
| 40 h    | 10:00 (day+2)                     | First 4 h visible ✗ (partial)   |
| 44 h    | 14:00 (day+2)                     | First 8 h visible ✓ (partial)   |
| 48 h    | 18:00 (day+2)                     | Full window visible ✓           |

The same analysis applied to a capture at **14:00 UTC** (mid-afternoon trough):

| Horizon | Horizon ends    | Next solar window (06:00 day+2) |
|---------|-----------------|----------------------------------|
| 36 h    | 02:00 (day+2)   | Misses entirely ✗                |
| 40 h    | 06:00 (day+2)   | Start only ✗                     |
| 44 h    | 10:00 (day+2)   | First 4 h visible ✓ (partial)   |
| 48 h    | 14:00 (day+2)   | 8 of 12 h visible ✓              |

**Conclusion:** 36 h is insufficient — it fails for roughly 8 h of every 24 h cycle
(afternoon captures between 12:00 and 20:00 UTC). The minimum horizon to always see some
of the next solar window is ~40 h; to always see a full window, 48 h is required. 44 h is a
reasonable compromise that guarantees visibility of the solar peak (10:00–14:00) from any
capture time, while cutting the binary variable count by ~8% vs 48 h.

### The solver cost of a longer horizon

The heater contributes 2 binary variables per slot (`z_heat_mid`, `z_heat_full`). The
battery adds 4 (charge, discharge, and direction indicators). EV adds 3 or more. At 288
slots (24 h × 5 min), the current heater-only binary count is 576.

| Configuration            | Slots | Heater binaries | Relative size |
|--------------------------|-------|-----------------|---------------|
| 24 h × 5 min (current)   |  288  |   576           | 1.0×          |
| 36 h × 5 min             |  432  |   864           | 1.5×          |
| 44 h × 5 min             |  528  | 1 056           | 1.8×          |
| 48 h × 5 min             |  576  | 1 152           | 2.0×          |
| 48 h × 10 min            |  288  |   576           | 1.0×          |
| 48 h × 15 min            |  192  |   384           | 0.7×          |

MILP branch-and-bound scaling is super-linear in binary count. Doubling binaries typically
increases solve time by 3–10× in practice, not 2×, because the branching tree depth grows.
On Pi4-Server (ARM64), the current 24 h plan already uses 18–60 s against a 60 s timeout.
A uniform 48 h × 5 min plan would likely time out or return a significantly suboptimal
solution on that hardware.

### Option 3a: Uniform coarser step (48 h × 10 min)

The simplest path. Change `plan_step_s: 600` and `plan_horizon_h: 48` in the profile.
Slot count stays at 288; binary count is identical to today. The only cost is near-future
resolution: relay decisions are quantised to 10 min instead of 5 min.

For a 2000 L hot water tank, 10 min resolution is physically appropriate — the thermal time
constant is ~143 h and a relay decision at 10 min granularity costs at most
`0.65 kW × (10/60) h × 0.38 EUR/kWh ≈ 0.04 EUR` in suboptimal timing. EV charging may
care more about 5 min resolution near departure time, but that is a separate concern.

**Code change required:** none (profile-only).  
**Risk:** EV deadline constraints lose precision; `plan_step_s` must be an integer divisor
of all tariff and capacity change periods.

### Option 3b: Non-uniform time grid (5 min near, 15–30 min far)

Split the horizon into two zones with different step sizes:

```
Zone A  (0 – 8 h):   96 slots × 5 min    — precise near-future relay control
Zone B  (8 – 48 h): 80–160 slots × 15–30 min — rough far-future thermal trajectory
```

Total slots: 176–256 vs 576 for uniform 48 h × 5 min, a **50–70% reduction** in binary
variables while keeping full 48 h visibility.

The thermal physics are valid at any step size — the tank dynamics constraint is:
```
E[t+1] = E[t] + (P_heat[t] − q_dem) × dt_h[t]
```
where `dt_h[t]` is the step width of slot `t`. Energy is conserved regardless of whether
`dt_h` is uniform.

**Code change required:** significant. The current MILP uses a scalar `dt_h` throughout
`inputs.rs`, `solver_phase1.rs`, `solver_phase2.rs`, and all asset constraint/objective
functions (`heater.rs`, battery, EV). Every `× dt_h` term must become `× dt_h[t]`.
`MilpInputs` changes from `dt_h: f64` to `dt_h: Vec<f64>`. Plan slot start/end times must
be computed from a variable-step grid. This is a self-contained refactor (no interface
changes visible to callers outside the `milp_planner` module) but touches ~10 files.

**Risk:** test fixtures for exact constraint counts (`9n` formula etc.) must be updated for
variable-step test grids. Switching penalty is per-event (not multiplied by `dt_h`) so it
is unaffected. EV deadline index calculation must use the cumulative step-time grid.

### Option 3c: Two-stage coarse/fine solve

Solve a **coarse 48 h problem** (e.g. 96 × 30 min slots = 192 binaries for the heater) to
determine which hours to heat. Then solve a **fine 8 h near-horizon problem** (96 × 5 min)
constrained to agree with the coarse plan's block boundaries.

Stage 1 finds the "when to heat" block structure across two days; Stage 2 refines the
exact relay switching within the first 8 h. The two solves run sequentially; total time is
the sum of two small problems rather than one large one.

**Pros:** Both solves stay well within the 60 s timeout; full 48 h visibility for block
placement; full 5 min precision for near-future execution.  
**Cons:** Most complex option. Requires an inter-stage communication protocol (block
boundaries from Stage 1 become constraints in Stage 2). The coarse plan's block boundaries
may not align with optimal fine-grained timing. Stage 2 must handle the case where Stage 1
produces an infeasible hand-off. New orchestration code in `tasks/planning.rs`.

### Recommendation for horizon extension

**Start with Option 3a** (48 h × 10 min, profile-only change). Benchmark solver time on
Pi4-Server immediately — if it stays under 40 s, this is the complete solution. The
near-future precision loss is acceptable for the heater; EV should be monitored.

If solver time becomes a problem or EV precision is unacceptable, **proceed to Option 3b**
(non-uniform grid). The `dt_h: f64 → Vec<f64>` refactor is well-bounded, mechanical, and
testable. It is the correct long-term architecture regardless of the horizon length.

Option 3c (two-stage) is reserved for the case where even Option 3b cannot be solved within
the timeout, which is unlikely given the binary count reduction.

---

## Options

### Option 1: Set a daily heater target (no code change)
`POST /heater-target` with `target_temp_c: 70.0, ready_by: <tomorrow 06:00>`.

Switches mode from `MayRun` (no incentive) to `MustRun` with a deadline. The MILP
constraint `e_tank[t_deadline] ≥ e_target` forces a large pre-heating block at the cheapest
available window. Result: one or two long overnight/solar runs instead of many small pulses.

**Pros:** No code change; models real DHW tank operation correctly; immediate effect.  
**Cons:** Requires a daily external trigger (cron, automation). If `ready_by` is already
past, the planner cannot satisfy the constraint.

---

### Option 2: Add a terminal energy reward to Phase 1 objective
Add a term `−c_terminal_eur_kwh × e_tank[n−1]` to the Phase 1 MILP objective in
`heater.rs → objective()`. The coefficient should equal the average expected future tariff
(e.g. 0.30–0.35 EUR/kWh), representing the forward value of stored heat.

**Pros:** Economically correct; heater exploits full solar/cheap window; fixes the root
cause without API changes.  
**Cons:** Requires MILP code change. The terminal coefficient needs calibration — too high
overfills; too low has no effect. Creates a slight end-of-horizon bias in the final slots.

---

### Option 3: Extend the planning horizon to 48 h
See the "Horizon and Resolution Trade-offs" section above for the full analysis. Three
sub-options in order of implementation complexity:

- **3a — 48 h × 10 min** (profile-only, 288 slots, same binary count as today): change
  `plan_step_s: 600` and `plan_horizon_h: 48`. No code change. Benchmark solver time first.
- **3b — Non-uniform grid** (5 min near-horizon, 15–30 min far-horizon, 176–256 slots):
  significant but bounded refactor — `dt_h: f64 → Vec<f64>` throughout the MILP module.
- **3c — Two-stage coarse/fine solve** (coarse 48 h for block placement, fine 8 h for
  execution): most complex; reserved for cases where 3b still cannot meet the timeout.

36 h is not sufficient — it fails for ~8 h of every 24 h cycle (afternoon captures between
~12:00 and ~20:00 UTC). 44 h guarantees solar-peak visibility from any capture time and is
a viable compromise if 48 h proves too slow.

**Pros:** Fixes the phase-dependence at its root; plan quality becomes independent of
capture time; may eliminate the need for a terminal reward term (Option 2).  
**Cons:** Uniform 5 min × 48 h doubles binary count and will likely exceed the Pi4 solver
timeout — must use 3a or 3b. Step size coarsening (3a) reduces near-future EV precision.

---

### Option 4: Increase Phase 2 epsilon
Raise `phase2_epsilon_eur` from 0.10 → 1.5–2.0 EUR in `ven-2.yaml`. Phase 2 then has
enough budget to consolidate fragmented pulses into large contiguous blocks, even if it
costs somewhat more energy to do so.

**Pros:** Single config change; directly eliminates single-slot pulses and small fragments.  
**Cons:** Does not fix the temperature ceiling (~42 °C); Phase 2 may over-consolidate in
scenarios where fragmentation was intentional (non-monotonic tariff). Interaction with the
existing 0.50 EUR switching penalty needs validation.

---

### Option 5: Add minimum-on-time constraint to the heater MILP
Add a "minimum up time" constraint: if the heater turns on at slot `t`, it must stay on for
at least `min_on_slots` consecutive slots (e.g. 6 = 30 min). Implemented in
`heater.rs → constraints()` using auxiliary start-indicator binary variables `y_start[t]`:

```
y_start[t] ≥ on[t] − on[t−1]          (detect rising edge)
∑(on[t..t+min_on−1]) ≥ min_on × y_start[t]   (enforce dwell)
```

**Pros:** Directly enforces large blocks at the MILP level; eliminates single-slot pulses
regardless of tariff structure.  
**Cons:** Requires new MILP variables and big-M style constraints (added code complexity).
May prevent optimal response to very short cheap windows. Does not fix the temperature
ceiling.

---

### Option 6: Fix the acceptance gate for fragmentation
Add a switch-count comparison to `evaluate_acceptance_gate` in `services/planning.rs`.
Options:

- Count state transitions in both plans; reject a periodic replan if it has more switches
  **and** its cost improvement does not exceed a per-switch threshold (e.g. 0.50 EUR/switch).
- Maintain a "best fragmentation seen" alongside the active plan; only replace with a more
  fragmented plan if the cost improvement exceeds a configurable multiplier.

**Pros:** Closes the structural imbalance in the gate; prevents decay from unconditionally
accepting worse schedules.  
**Cons:** Switch counting must be added to `Plan` (currently not stored); the per-switch
threshold is a new tuning parameter. Gate logic becomes more complex.

---

### Option 7: Block commitment anchor (plan stability)
After adoption, store `anchor_until: Option<DateTime<Utc>>` in `AppState`, set to the
planned end of the **current on/off block** (the boundary of the first state transition in
the active plan). On the next replan:

1. Build MILP inputs normally.
2. For all slots `t` where `slot.start < anchor_until`, pin `z_heat_mid[t]` and
   `z_heat_full[t]` to the values from the current plan (replace `binary()` variables with
   fixed `min(v).max(v)` bounds matching the current plan's decision).
3. Set `initial_z_*` from the last anchored slot rather than the live hardware state.
4. Hard triggers (DR event, temperature anomaly > threshold, manual override) clear
   `anchor_until` and force a full fresh solve.

This prevents mid-block reversals entirely while still allowing the solver to optimise all
future blocks freely.

**Pros:** Eliminates near-future chattering and block-boundary drift; no change to the
MILP formulation or acceptance gate; orthogonal to all other options.  
**Cons:** Requires storing anchor state across replan cycles (`AppState` change).  If the
physical system diverges from the anchored plan (e.g. unexpected hot water draw), the solver
must detect this via the anomaly threshold and clear the anchor; getting that threshold
right requires tuning.

---

### Option 8: Two-zone rolling commitment
Divide the planning horizon into a **committed zone** (first N hours, e.g. 2 h) and an
**optimisable zone** (remainder). Rules:

- The committed zone is drawn from the currently active plan and is **never changed** by
  periodic replans — only by hard triggers.
- Each periodic replan solves only the optimisable zone, warm-starting from the state at
  the committed zone boundary.
- When real time passes a committed slot, that slot is consumed; the next slot from the
  optimisable zone is promoted into the committed zone.

This is the classic MPC (Model Predictive Control) structure with an explicit commitment
window. It guarantees that the first 2 hours of the schedule are stable, regardless of
replanning frequency or gate behaviour.

**Pros:** Maximum operational stability; hardware sees a predictable schedule; naturally
limits the damage from the gate accepting a worse plan.  
**Cons:** Most significant architectural change of all options. Requires splitting the MILP
solve, managing the committed-zone boundary, and defining how the optimisable-zone initial
conditions are derived from the committed zone end state. The committed zone must still be
re-evaluated when hard triggers fire (otherwise a DR event mid-block would be ignored).

---

## Critical Assessment: What Each Combination Actually Fixes

No single option is sufficient. The problems form three independent axes and must be treated
separately.

### Axis 1 — Phase-dependence (plan quality oscillates with time of day)
**Root cause:** 24 h horizon sometimes contains a second solar window, sometimes does not.  
**Fixed by:** Option 3a or 3b (horizon extension).  
**NOT fixed by:** Options 2, 4, 5, 6, 7, 8 in isolation.

### Axis 2 — Temperature ceiling and low storage utilisation
**Root cause:** No terminal value — heat stored at horizon end is worth €0 to the solver.
This is independent of horizon length. Even with 48 h visibility, the lazy equilibrium
is: heat ~10°C per solar window, coast back to T_min, repeat. The solver has no reason
to fill the tank to 60–80°C unless the energy stored in window 1 is demonstrably cheaper
than what window 2 would require — which requires a terminal cost at hour 48 or a
user-supplied target.  
**Fixed by:** Option 2 (terminal reward) or Option 1 (explicit target).  
**NOT fixed by:** Options 3, 4, 5, 6, 7, 8 in isolation.

### Axis 3 — Fragmentation and plan instability
**Root cause:** Three distinct sub-causes, each requiring its own fix.

| Sub-cause | Fix |
|-----------|-----|
| Phase 2 epsilon (0.10 EUR) is far smaller than switching penalty (0.50 EUR) — self-contradictory: penalty says switching is expensive but Phase 2 has no budget to avoid it | Option 4: raise epsilon to ≥ 1× switching_penalty (≥ 0.50 EUR), ideally 1.0–1.5 EUR |
| Acceptance gate imbalance: `friction_eur` variance bounded by epsilon, `objective_eur` varies freely — a noisier plan wins if it found 0.15 EUR cheaper energy | Option 6: gate switch-count guard. Option 4 also reduces imbalance because larger epsilon makes `friction_eur` more variable |
| Near-future chattering: entire plan replaced every 5 min, current block can be interrupted | Option 7 (block anchor) or Option 8 (two-zone MPC) |

### Why 3b + 4 alone is insufficient

After implementing Option 3b (48 h non-uniform grid) and Option 4 (epsilon raised to 1.5 EUR):

- ✓ Plan quality no longer oscillates with time of day
- ✓ Overnight fragmented pulses eliminated
- ✓ Phase 2 can now consolidate 2–3 extra switches per plan (1.5 / 0.5 = 3)
- ✗ **Temperature ceiling is unchanged.** The heater will still reach ~44–48 °C at solar
  window peak and coast back to 40 °C. There is still no terminal value.
- ✗ **Gate imbalance partially improved but not fixed.** A new plan with 0.20 EUR cheaper
  energy and 2 extra switches (1.0 EUR extra friction) still nets −0.80 EUR and is rejected.
  But a plan with 1.60 EUR cheaper energy and 2 extra switches (1.0 EUR) still wins by 0.60
  EUR. The economic term can still dominate.
- ✗ **Near-future chattering unchanged.** Each replan still replaces all 288 slots including
  the relay decision for the next 5 minutes.

### The epsilon/penalty incoherence

The current config has `switching_penalty_eur: 0.50` but `phase2_epsilon_eur: 0.10`. This
is internally inconsistent:
- Phase 2 says: "each switch costs 0.50 EUR — I will minimise switches"
- Phase 2 epsilon says: "you have a budget of 0.10 EUR to do so"
- Together: "each switch costs 0.50 EUR but you can only afford to eliminate 0.10/0.50 = 0.2
  of a switch before exhausting your budget"

Phase 2 can consolidate two pulses into one block only if doing so costs less than 0.10 EUR
in extra energy. At 0.30 EUR/kWh that is 0.33 kWh — approximately one 5-min slot at 4 kW.
Any consolidation requiring more heating than that fails.

The epsilon should be set to at least **1× the switching penalty** to allow Phase 2 to
eliminate one extra switch per plan, and ideally **2–3× the switching penalty** for plans
with 3–6 switches. With `switching_penalty_eur: 0.50`, a coherent epsilon is **0.75–1.50 EUR**.

---

## Recommended Path

### Minimum necessary combination

These three together address all three axes:

1. **Option 3a** (`plan_step_s: 600`, `plan_horizon_h: 48` — profile-only, benchmark first)
   or **Option 3b** (non-uniform grid if 3a times out on Pi4). Fixes Axis 1.

2. **Option 2** (terminal energy reward `−c_terminal × e_tank[n−1]` in `heater.rs`).
   Fixes Axis 2. The coefficient should equal the average tariff (0.30–0.35 EUR/kWh).

3. **Option 4** (raise `phase2_epsilon_eur` to 0.75–1.50 EUR, coherent with
   `switching_penalty_eur: 0.50`). Fixes fragmentation sub-cause 1 (epsilon/penalty
   incoherence). Also partially improves the gate.

### Additional fixes worth doing

4. **Option 7** (block commitment anchor in `AppState`). Fixes near-future chattering.
   Small, self-contained change.

5. **Option 6** (gate switch-count guard in `services/planning.rs`). Prevents quality
   regression across replans. Small, self-contained change.

### What can be deferred

- **Option 1** (daily target via API) is a workaround for Axis 2 while Option 2 is not yet
  implemented. Remove it once Option 2 is in place.
- **Option 5** (minimum-on-time constraint) becomes unnecessary if Options 3+4 are in place,
  since the extended horizon already discourages short pulses and the epsilon provides budget
  to consolidate any that remain.
- **Option 8** (two-zone MPC) is the full plan stability solution. Defer until Options 3+4+7
  are in place and evaluated — Option 7 may be sufficient.
- **Option 3c** (two-stage solve) is a fallback only if 3b proves too slow on Pi4.

See **[heater_planning_impl.md](heater_planning_impl.md)** for the full step-by-step
implementation plan including sub-tasks, exact file locations, code sketches, and
test names for each of the five steps.
