# VEN Planner — LP (Linear Program) Target Architecture

**Status:** Design proposal — forward-looking replacement for the greedy rules engine.
Describes the motivating analysis, full LP formulation, and phased migration path.

Related documents:
- [ven_planning_architecture.md](ven_planning_architecture.md) — current planning principles
- [VEN_ARCHITECTURE.md](VEN_ARCHITECTURE.md) — current implementation reference
- [Domain_definitions.md](Domain_definitions.md) — vocabulary

---

## 1. Motivation

The current planner (`VEN/src/controller/planner.rs`) is a **greedy, slot-by-slot rules
engine**: for each 5-minute slot, each asset is evaluated in sequence
(`pv → base_load → ev → battery → heater`) and one rule fires. Every rule is a
hand-coded approximation of a single underlying question:

> *"For this asset, in this slot, what power level maximises net value to the user?"*

This approach has three structural problems that worsen as the system grows:

### 1.1 Binary power decisions

Rules decide "charge at `desired_power_kw` or 0". The optimal answer is a **continuous
value** in `[0, max_power_kw]`. The EV/PV issue that revealed this: with 3.5 kW of PV
surplus and `desired_power_kw = 7.4 kW`, the planner computes `eff_cost` as a blend of
53% grid tariff + 47% export tariff — and rejects the slot. The EV could have absorbed
the 3.5 kW surplus directly at near-zero cost, but the rule offers no middle ground.
When no power is specified in the user request (SoC-only), `desired_power_kw` wrongly
defaults to `max_charge_kw`, fixing the setpoint at maximum even though the user
expressed no preference about rate.

### 1.2 Local (per-slot) decisions with no horizon view

The rules engine processes slot 0, then slot 1, each decision blind to the future.
Consequences:
- Battery charges from PV at 08:00 (Rule 8b), then discharges to help EV at 10:00
  (Rule 10) — paying ~8% round-trip efficiency loss for something EV could have done
  directly.
- The two-pass battery pre-planner (`plan_battery_grid_charges`) is a workaround that
  partially restores horizon awareness for the battery, but not for EV/battery
  coordination.
- Rule ordering (EV before battery) determines winner/loser dynamics for PV surplus.
  Swap the order and the plan changes completely.

### 1.3 Additive rule complexity

Each observed failure mode adds another rule. Rules 8, 8b, 9, and 10 all interact
around PV surplus allocation. There is no single coherent objective they all serve.
Predicting emergent behaviour requires knowing all rules and their interaction order.

---

## 2. What an LP is

A **Linear Program** finds values of continuous variables that maximise a linear
objective function subject to linear inequality constraints. For the HEMS planner:

| LP concept | HEMS meaning |
|------------|-------------|
| Variables | Power (kW) for each controllable asset in each slot |
| Objective | Minimise grid cost minus comfort value delivered (or another goal — see §4) |
| Constraints | Grid capacity, SoC/temperature bounds, physics (energy balance) |

For 288 slots × ~7 variables ≈ 2 000 variables and ~2 500 constraints, any LP solver
finishes in **under 5 ms**. The `build_grid` output maps directly onto LP input data.

---

## 3. Full LP Formulation

### 3.1 Inputs (known per slot, not variables)

| Symbol | Source | Unit |
|--------|--------|------|
| `p_pv[t]` | PV forecast from `pv_kw_map` | kW (generation ≥ 0) |
| `p_base[t]` | Base load from profile | kW |
| `tariff_import[t]` | Tariff time series (LOCF) | EUR/kWh |
| `tariff_export[t]` | Tariff time series (LOCF) | EUR/kWh |
| `co2_import[t]` | CO₂ intensity time series | gCO₂/kWh |
| `import_cap[t]` | OadrCapacityState | kW |
| `export_cap[t]` | OadrCapacityState | kW |
| `T_ambient[t]` | Ambient temperature (heater model) | °C |
| `dt` | Slot duration (5 min = 1/12 h) | h |

### 3.2 Decision variables (one set per slot `t = 0…N−1`)

**Storage assets (EV, stationary battery):**

| Variable | Domain | Meaning |
|----------|--------|---------|
| `p_ev[t]` | `[0, max_charge_kw_ev]` | EV charge power |
| `p_bat_c[t]` | `[0, max_charge_kw_bat]` | Battery charge power |
| `p_bat_d[t]` | `[0, max_discharge_kw_bat]` | Battery discharge power |

Battery charge and discharge are **separate non-negative variables** to handle
asymmetric round-trip efficiency without introducing nonlinearity. The LP naturally
sets at most one non-zero per slot because simultaneous charge+discharge is never
optimal under a cost-minimising objective.

**Thermal assets (heater):**

| Variable | Domain | Meaning |
|----------|--------|---------|
| `p_heater[t]` | `[0, max_power_kw_heater]` | Heater power |

The heater is thermally equivalent to a battery: it has a state variable (temperature)
that couples adjacent slots, comfort bounds instead of SoC bounds, and a bid value
that decreases as the room warms (see §3.4).

**Grid (auxiliary variables):**

| Variable | Domain | Meaning |
|----------|--------|---------|
| `g_import[t]` | `[0, import_cap[t]]` | Grid import |
| `g_export[t]` | `[0, export_cap[t]]` | Grid export |

**State variables** (derived via continuity constraints, not free variables):

| Variable | Domain | Initial condition |
|----------|--------|-------------------|
| `soc_bat[t]` | `[min_soc_bat, 1.0]` | `soc_bat[0]` = current battery SoC |
| `soc_ev[t]` | `[current_soc_ev, soc_target_ev]` | `soc_ev[0]` = current EV SoC |
| `temp_room[t]` | `[T_min, T_max]` | `temp_room[0]` = current room temperature |

**Peak import (optional auxiliary, used only by the min-peak objective):**

| Variable | Domain | Meaning |
|----------|--------|---------|
| `peak_import` | `[0, import_cap_max]` | Maximum grid import across all slots |

### 3.3 Constraints

#### Power balance (one per slot)
```
g_import[t] − g_export[t]  =  p_base[t] + p_ev[t] + p_bat_c[t] − p_bat_d[t]
                               + p_heater[t] − p_pv[t]
```
This replaces the per-slot `site_ctx.planned_others_kw` bookkeeping in the rules loop.
New controllable assets are added by appending their power variable to the right-hand
side; no other structural change is needed.

#### Battery SoC continuity (couples adjacent slots — forces horizon-wide planning)
```
soc_bat[t+1] = soc_bat[t]  +  p_bat_c[t] × η_c × dt / cap_bat
                            −  p_bat_d[t] × dt / (η_d × cap_bat)
```
where both equal `sqrt(round_trip_efficiency)`. Note the asymmetric application:
charging *multiplies* by η_c (losses reduce what is stored); discharging *divides* by
η_d (losses mean more SoC is consumed per kW delivered to the site). Both being equal
to the same value is correct — their product η_c × η_d = round_trip_efficiency as
expected for a full charge-then-discharge cycle.

This single family of constraints is what enables the LP to see all tariffs
simultaneously and schedule battery charging at the globally cheapest moment, rather
than reactively (Rule 10) or via a separate pre-pass (Rule 9).

#### EV SoC continuity
```
soc_ev[t+1] = soc_ev[t]  +  p_ev[t] × eta_ev × dt / cap_ev
```
where `eta_ev` is the EV onboard charger efficiency (config field `eta_charge`,
default 1.0). The default preserves the current approximation; a measured value
(typically 0.88–0.95) can be set in the profile without structural change.

#### EV deadline (hard or soft)
```
soc_ev[T_deadline] ≥ soc_target_ev          (hard — infeasible if unachievable)
```
Preferred form: **soft constraint** via a slack variable `s_ev ≥ 0` with a large
penalty `M` in the objective, so the LP always produces a solution:
```
soc_ev[T_deadline] + s_ev ≥ soc_target_ev
+ M × s_ev   added to the objective (penalise shortfall)
```
`s_ev` in the solution reports exactly how much SoC was undeliverable.

#### Heater thermal continuity
```
temp_room[t+1] = temp_room[t]  ×  (1 − k_loss × dt / C_th)
              +  p_heater[t]   ×  dt / C_th
              +  k_loss × T_ambient[t] × dt / C_th
```
where `k_loss` is the thermal conductance (kW/°C) and `C_th` is the thermal capacity
(kWh/°C). Both are constants from the asset profile, so this constraint is fully
linear in `temp_room[t]` and `p_heater[t]`.

The heater's temperature bounds enforce comfort:
```
T_min ≤ temp_room[t] ≤ T_max   ∀t
```

#### Peak import bound (only when using min-peak objective — see §4)
```
g_import[t] ≤ peak_import   ∀t
```

### 3.4 Comfort bid parameterisation — bid_at(SoC)

The bid represents the marginal value of delivering energy to an asset *right now*.
For storage assets (EV, battery), the physically meaningful parameterisation is
**SoC**, not fill:

- `fill = (energy_delivered − energy_at_start) / energy_target` — depends on when the
  packet was created; changes meaning every replanning cycle.
- `soc` — the physical state of the device, independent of packet history.

`bid_at(soc)` has a stable, interpretable meaning: *"at this charge level, how much
is the user willing to pay per kWh?"*. It does not shift when the packet is recreated
after a replanning event or a completed charge cycle.

The bid function remains the same piecewise-linear form, but parameterised over the
asset's physical SoC range `[soc_min, soc_target]`:

```
bid_ev(soc) = bid_max   when soc = soc_min   (empty — urgent)
bid_ev(soc) = bid_min   when soc = soc_target (full — not worth paying more)
```

For the EV, `soc_min` is a configuration field in `EvConfig` with default `0.0`
(fully discharged = maximum urgency). A non-zero value (e.g. 0.1) can be used to
model range-anxiety protection, raising the bid floor so the EV always stays above
the minimum comfortable charge level.

For the LP, use the bid evaluated at the **start-of-plan SoC** as a fixed coefficient.
This is exact when the SoC change over the horizon is small. For larger changes, use
the piecewise-linear segment approach: partition `[current_soc, soc_target]` into 2–3
segments, each with its own `p_ev_k[t]` variable and bid value (§3.5).

For the **heater**, the equivalent is `bid_heater(temp)`:
```
bid_heater(temp) = bid_max   when temp = T_min   (cold — urgent)
bid_heater(temp) = bid_min   when temp = T_max   (warm — not worth paying more)
```

### 3.5 Piecewise-linear bid (exact declining curve)

Split the remaining energy range into N segments. For EV with 3 segments:

```
p_ev[t]  =  p_ev_0[t] + p_ev_1[t] + p_ev_2[t]

0 ≤ p_ev_k[t] ≤ max_charge_kw_ev    ∀k, t
Σ_k p_ev_k[t]  ≤ max_charge_kw_ev   ∀t           (physical power cap — sum across segments)
Σ_t p_ev_k[t] × dt ≤ e_k            ∀k           (segment energy cap)
```

Objective contribution: `− (b_0 × p_ev_0[t] + b_1 × p_ev_1[t] + b_2 × p_ev_2[t])`

Because `b_0 > b_1 > b_2`, the LP fills segment 0 first (highest-value energy), then
segment 1, etc. — correctly modelling diminishing marginal value without any nonlinearity.

For the initial implementation, **skip this and use a single fixed bid coefficient**
(the bid evaluated at the current SoC). The improvement from segmentation is marginal
when the remaining SoC gap is small (< 10%).

---

## 4. Objective modes

The objective is a **coefficient vector over the decision variables**. Different
optimisation goals are different vectors — the LP structure is identical. The active
mode is selected once at startup (from the profile or an environment variable) and
does not change during a planning cycle.

### Mode A — Minimise cost (default)

```
minimise  Σ_t [  tariff_import[t] × g_import[t]
               − tariff_export[t] × g_export[t]
               − bid_ev(soc_ev_0) × p_ev[t]
               − bid_heater(temp_0) × p_heater[t]  ] × dt
```

Battery arbitrage and PV self-consumption emerge automatically: the LP charges the
battery when `tariff_import` is low and discharges when high; it prefers local PV
consumption over export because that avoids paying `tariff_import` elsewhere.

### Mode B — Minimise GHG emissions

Replace tariff coefficients with CO₂ intensity coefficients:

```
minimise  Σ_t [  co2_import[t] × g_import[t]
               − bid_ev_co2    × p_ev[t]       ] × dt
```

The LP shifts EV and battery charging to slots with low grid carbon intensity
(typically overnight wind, midday solar) rather than cheapest tariff slots.

Export CO₂ credit is intentionally omitted: correctly accounting for the carbon
displaced by exported PV requires marginal grid-mix data at export time, which is not
currently available. The term can be added later.

The comfort bid terms remain in EUR/kWh; the cost terms are in gCO₂/kWh — a
**mixed-unit objective**. Calibrate `bid_ev_co2` by converting the EUR bid using a
site-level carbon price: `bid_ev_co2 = bid_ev_eur / carbon_price_eur_per_gco2`. A
value of 0.05 EUR/kgCO₂ = 5×10⁻⁵ EUR/gCO₂ gives a reasonable default. Store
`carbon_price_eur_per_gco2` in the profile.

### Mode C — Minimise peak grid power (grid-friendly)

Introduce the `peak_import` auxiliary variable:

```
minimise  w_peak × peak_import
        − Σ_t [ bid_ev(soc_ev_0) × p_ev[t] + bid_heater(temp_0) × p_heater[t] ] × dt

subject to  g_import[t] ≤ peak_import   ∀t     (new constraint)
            + standard constraints from §3.3
```

`w_peak` is a weighting in EUR/kW (profile field, default 1.0). Setting it
significantly above typical bid values (e.g. 10.0 EUR/kW) makes peak reduction the
dominant term while still rewarding energy delivery — preventing the LP from simply
not charging the EV at all.

This linearises the `minimax` problem exactly via the auxiliary variable. The LP
spreads flexible loads evenly across the horizon rather than clustering in cheap slots.
Useful when the site has a capacity tariff component or the grid operator prioritises
demand flatness.

### DR compliance overlay (automatic, not a selectable mode)

OpenADR IMPORT_CAPACITY_LIMIT and EXPORT_CAPACITY_LIMIT events already impose hard
constraints via `import_cap[t]` and `export_cap[t]` bounds on the auxiliary variables.
This alone handles firm DR signals.

For advisory signals (PRICE, SIMPLE), an additional weight is applied **automatically
at each planning cycle** whenever a relevant DR event window is active — independent of
which base mode (A/B/C) is selected:

```
+ w_dr × Σ_{t ∈ DR_window} g_import[t] × dt
```

`w_dr` is a large constant (default: 100.0 EUR/kWh, profile-configurable) that
dominates all other objective terms during the DR window. The `DR_window` slot set is
recomputed from active events at every planning cycle. This overlay is not a
standalone mode — it activates automatically and stacks on top of the base mode.

### Combining modes (weighted sum)

Modes A and B can be blended by converting GHG terms to EUR-equivalent via the
site-level carbon price (see Mode B):

```
minimise  α × cost_objective  +  (1−α) × ghg_objective_in_eur_equivalent
```

`α ∈ [0, 1]` is a profile field expressing the site owner's preference between cost
and carbon reduction. α = 1.0 is pure cost (Mode A); α = 0.0 is pure GHG (Mode B).

---

## 5. What the LP eliminates

| Current component | Replaced by |
|-------------------|-------------|
| Rule 6 (comfort bid gate) | Objective term `−bid(soc) × p[t]` |
| Rule 7 (deadline pressure override) | Soft deadline constraint + penalty `M × s_ev` |
| Rule 8b (battery absorbs surplus) | Power balance + objective (emerges naturally) |
| Rule 9 (two-pass battery pre-planner) | Battery SoC continuity over full horizon |
| Rule 10 (battery discharge at expensive tariff) | Same SoC continuity + objective |
| `desired_power_kw` as fixed setpoint | Variable `p[t]` is continuous `[0, max]` |
| Asset processing order as implicit priority | Objective coefficient determines priority |
| `bid_at(fill)` — packet-relative parameterisation | `bid_at(soc)` — physical state |

Rules that **remain** (physics and hard constraints, not optimisation heuristics):
- Rule 1 (FIRM reservation blocks headroom) → variable upper bound: `p[t] ≤ headroom[t]`
- Rule 4a/4/5 (SoC ceiling/floor) → variable domain bounds on `soc[t]`
- Rule 2 (site import cap) → `g_import[t] ≤ import_cap[t]`

---

## 6. Limitations

### 6.1 Minimum charge power

Some EV chargers require a minimum power level (e.g. ≥ 1.4 kW or 0 — no partial
charging). This is a **binary on/off decision**, which makes the problem a
Mixed-Integer LP (MILP). MILPs are harder to solve but still fast at this scale
(~50–200 ms with HiGHS). For the initial implementation: assume EVs accept any
power ≥ 0 (pure LP). Introduce the integer constraint only if hardware requires it.

### 6.1.a Heater, Boilers are ON or OFF, three tiers max.

TODO: how to handle them? Add a pre-stage to make block reservations?

### 6.1.a Washing machines have set profiles, once started can not be switched off.

TODO: how to handle them? include in forecast / boundaries?

### 6.2 Infeasibility

If a deadline is too tight or grid capacity too constrained, the LP has no feasible
solution. Mitigation: replace all deadline hard constraints with soft constraints
(slack variable + large penalty). The LP always produces a solution; the slack value
reports exactly how much energy was undeliverable and why.

### 6.3 Forecast uncertainty

The LP optimises against the PV forecast and tariff schedule. Forecast errors (cloudy
day, tariff revision) cause the plan to be suboptimal relative to reality. Mitigation:
replanning every 20 s (already the current behaviour) limits the damage window.
Robust optimisation (optimise against worst-case scenario) is a future extension.

---

## 7. Phased migration

The LP planner is implemented as `VEN/src/controller/lp_planner.rs` with **the same
function signature** as the existing `planner.rs`:

```rust
pub fn run_planner(
    now:          DateTime<Utc>,
    assets:       &SimState,
    tariffs:      &TariffTimeSeries,
    capacity:     &OadrCapacityState,
    packets:      &mut Vec<EnergyPacket>,
    reservations: &ReservationLayer,
    profile:      &Profile,
) -> Plan
```

The caller (`loops.rs`) does not change. The active implementation is selected by a
field in the profile YAML:

```yaml
planner: lp        # or: rules (default during transition)
```

Once the LP is validated against the full BDD test suite, `planner.rs` is deleted.

### Phase 1 — Battery only (low risk)

Replace `plan_battery_grid_charges` (the two-pass pre-planner) with an LP over
`{p_bat_c[t], p_bat_d[t], soc_bat[t]}` treating EV and heater as fixed loads equal
to their current rules-engine setpoints. Keep all other rules intact.

Eliminates Rules 9 and 10. Battery now has globally optimal horizon-wide scheduling.
EV and heater dispatch are unchanged.

### Phase 2 — EV as LP variable

Extend the LP to include `p_ev[t]` and `soc_ev[t]`. Remove Rules 6, 7, and the
`desired_power_kw = max_charge_kw` default fallback. Switch bid parameterisation from
`bid_at(fill)` to `bid_at(soc_ev[0])`.

PV surplus allocation between EV and battery now emerges from the objective with no
asset-ordering dependency. The EV naturally charges at partial power from PV surplus
rather than all-or-nothing at max rate.

### Phase 3 — Heater and multi-objective

Add `p_heater[t]` and `temp_room[t]` with thermal continuity constraints. Introduce
the objective mode selector (§4) in the profile. Add piecewise-linear bid segmentation
(§3.5) for assets with large SoC swings. At this point `planner.rs` can be deleted.

---

## 8. Implementation notes

### LP library

[`good_lp`](https://crates.io/crates/good_lp) provides a clean Rust-native LP
frontend with pluggable solver backends. Recommended backend: **HiGHS** (open-source,
handles both LP and MILP, consistently the fastest open solver at this problem scale).

```toml
good_lp = { version = "1", features = ["highs"] }
```

### Mapping from existing data structures

| Existing struct / field | LP role |
|-------------------------|---------|
| `PlanTimeSlot.import_tariff_eur_kwh` | `tariff_import[t]` coefficient |
| `PlanTimeSlot.export_tariff_eur_kwh` | `tariff_export[t]` coefficient |
| `PlanTimeSlot.co2_g_kwh` | `co2_import[t]` coefficient (Mode B) |
| `PlanTimeSlot.pv_forecast_kw` | `p_pv[t]` input |
| `PlanTimeSlot.import_cap_kw` | upper bound on `g_import[t]` |
| `AssetState::Battery(b).soc` | `soc_bat[0]` |
| `AssetState::Ev(e).soc` | `soc_ev[0]` |
| `packet.value_curve.bid_at(soc_ev[0])` | `bid_ev` objective coefficient |
| `packet.latest_end()` → slot index | deadline slot `T_deadline` |
| `BatteryConfig.round_trip_efficiency` | `η_c`, `η_d` |
| `HeaterConfig.{k_loss, thermal_mass_kwh_c}` | thermal continuity constants |

### Mapping LP output back to Plan

| LP solution variable | Plan struct field |
|----------------------|-------------------|
| `p_ev[t]` | `PlanStep.setpoint_kw` for EV |
| `p_bat_c[t] − p_bat_d[t]` | `PlanStep.setpoint_kw` for battery |
| `p_heater[t]` | `PlanStep.setpoint_kw` for heater |
| `g_import[t]` | `PlanTimeSlot.net_import_kw` |
| `g_export[t]` | `PlanTimeSlot.net_export_kw` |
| `soc_bat[t]`, `soc_ev[t]` | state trajectory (diagnostics / trace) |
| `s_ev` (slack) | undeliverable energy — surfaced in envelope |

### PlanReason simplification

The `PlanReason` enum collapses to a single informative variant:

```rust
LpOptimal {
    marginal_value_eur_kwh: f64,   // bid coefficient that drove this allocation
    marginal_cost_eur_kwh:  f64,   // effective cost at this power level
    objective_mode:         ObjectiveMode,
}
```

This is strictly more transparent than the current rule labels: every allocation is
explained by two numbers (value vs. cost) and the active objective mode, with no
implicit rule-ordering context required to interpret it.

---

## 9. Two-layer execution architecture

The LP produces a plan in 5-minute steps. A 5-minute step is a long time: PV output
can swing by several kW within seconds as clouds pass. The plan setpoint for a slot is
the *average* the LP decided is optimal — it cannot know the second-by-second profile
within that slot.

This is handled by separating concerns into two control layers with different
timescales and different purposes.

```
┌─────────────────────────────────────────────────────────────┐
│  Planning layer  (LP, every ~20 s)                          │
│                                                             │
│  Input:  forecast PV, tariffs, SoC, horizon                 │
│  Output: target setpoint per asset per 5-min slot           │
│  Goal:   globally optimal energy scheduling                 │
└───────────────────────┬─────────────────────────────────────┘
                        │  planned setpoints  (feedforward)
                        ▼
┌─────────────────────────────────────────────────────────────┐
│  Reactive execution layer  (dispatcher, every 1 s)          │
│                                                             │
│  Input:  actual sensor readings (PV, grid, SoC)             │
│  Output: adjusted setpoints sent to physical devices        │
│  Goal:   track plan intent under real-time deviations       │
└─────────────────────────────────────────────────────────────┘
```

The plan provides **intent** ("import roughly 3 kW this slot"). The reactive layer
provides **execution** ("actual PV is 2 kW higher than forecast right now — absorb it").

### 9.1 What belongs in each layer

| Concern | Layer | Reason |
|---------|-------|--------|
| When to charge EV (which slots) | Planning | Requires horizon view and tariff forecast |
| How much to charge over the slot | Planning | LP produces optimal average setpoint |
| Absorbing a PV spike within a slot | Reactive | No forecast needed — react to live reading |
| Battery smoothing (grid fluctuation) | Reactive | Purely local, second-to-second |
| Staying within import cap in real-time | Reactive | Hard constraint, cannot wait for replan |
| Responding to OpenADR FIRM signal | Planning | Encoded as constraint in LP |
| Responding to a sudden capacity curtailment | Both | LP replans; reactive layer enforces immediately |

**Simple rules are appropriate in the reactive layer** precisely because it has no
forecasting requirement: every decision is made on current sensor readings alone with
no look-ahead.

### 9.2 Reactive opportunities (within a slot)

**R1 — Surplus absorption**

When `actual_pv_kw > planned_pv_kw` for the current slot, there is unplanned surplus.
Route it to flexible assets in priority order:

1. EV (if plugged and below `soc_target`) — up to `max_charge_kw_ev`
2. Battery (if below full) — up to `max_charge_kw_bat`
3. Heater (if below `T_max`) — up to `max_power_kw_heater`
4. Grid export (fallback — always available)

This is purely local: no packet commitment, no VTN report entry. Surplus absorbed here
is counted in the physical SoC/temperature at the next LP run, which will naturally
produce a lower setpoint for the following slot.

**R2 — Deficit response**

When `actual_pv_kw < planned_pv_kw`, the current slot is under-producing relative to
the plan. The grid import rises above the LP's expected value. Response:

1. Reduce the current flexible setpoint proportionally (shave the excess import).
2. If import cap would be breached: reduce EV charge first (most flexible), then
   battery, then heater. Never reduce base load (uncontrollable).

The LP will correct allocation in the next replan cycle (20 s). The reactive layer
only prevents constraint violations and excessive grid draw in the interim.

**R3 — Battery as real-time grid buffer**

The battery can smooth second-to-second grid import fluctuations regardless of what
the LP scheduled. Apply a simple proportional controller:

```
error_kw         = actual_grid_import_kw − target_grid_import_kw
bat_adjustment_kw = clamp(−k_p × error_kw, −max_discharge_kw, max_charge_kw)
```

`target_grid_import_kw` is the LP's planned `g_import[t]` for the current slot.
`k_p` is a gain constant (e.g. 0.5 — respond at half the error magnitude per tick).

This keeps grid import close to the planned value even as PV and base load fluctuate,
without the reactive layer needing to know anything about tariffs or deadlines.

**R4 — Partial-slot startup**

When the LP replans mid-slot (e.g. a new packet arrives), the new plan's setpoint for
the current slot applies from `now` to the slot end, not from the slot start. The
dispatcher prorates: if 2 of 5 minutes remain, the physical device receives the new
setpoint for only those 2 minutes. The energy credited to the packet for this slot
is measured from actual power, not from the plan — so the next LP run starts from
the correct SoC.

### 9.3 Boundaries the reactive layer must not cross

The reactive layer operates **within the constraints already established by the LP**.
It must not:

- Exceed `import_cap[t]` — this is a hard grid constraint.
- Charge EV beyond `soc_target` — the LP already accounts for this.
- Discharge battery below `min_soc` — physical bound.
- Accumulate R1 surplus into `past_power_profile` — surplus absorbed by the reactive
  layer is not packet-attributed delivery. Only power delivered under a planned
  non-zero LP setpoint flows into `past_power_profile` and hence into VTN reports.
  The SoC gain from R1 surplus is visible to the LP at the next replan cycle through
  the updated physical state.

### 9.4 Interaction with the LP on the next cycle

The reactive layer changes physical state (SoC, temperature) within a slot. The LP
reads current state at the start of each replan cycle. This closes the feedback loop:

```
LP decides slot setpoints
    ↓
Dispatcher executes + reactive adjustments
    ↓
Physical state evolves (SoC, temperature, actual PV)
    ↓
LP replans from updated state  ← loop closes here
```

No explicit communication is needed between the two layers beyond the plan setpoints
(feedforward, planning → dispatcher) and the physical state (feedback, sim → planning).
The reactive layer does not need to inform the LP of what it did — the LP observes the
resulting state at the next cycle and plans accordingly.

---

## 10. Implementation plan

The four phases below map directly onto the architecture sections above. Each phase
is independently releasable: the BDD test suite must pass in full before the next
phase begins. Phases 1–2 touch only `planner.rs` and its dependencies. Phases 3–4
extend to dispatcher and new assets.

### Phase 1 — Battery LP (replaces two-pass pre-planner)

**Goal:** replace `plan_battery_grid_charges` with an LP. EV and heater dispatch
are unchanged. All existing BDD scenarios continue to pass.

| Step | File(s) | Task |
|------|---------|------|
| 1.1 | `VEN/Cargo.toml` | Add `good_lp = { version = "1", features = ["highs"] }` |
| 1.2 | `VEN/src/controller/lp_planner.rs` | Create file; export `pub fn run_planner(...)` with identical signature to `planner.rs::run_planner` |
| 1.3 | `VEN/src/profile.rs` | Add `planner: PlannerBackend` field (`enum PlannerBackend { Rules, Lp }`); default = `Rules` |
| 1.4 | `VEN/src/loops.rs` | Branch on `profile.planner` to call `lp_planner::run_planner` or `planner::run_planner` |
| 1.5 | `lp_planner.rs` | Implement `build_lp_inputs()`: translate `Vec<PlanTimeSlot>` into coefficient vectors (`tariff_import`, `tariff_export`, `p_pv`, `import_cap`, `dt`) |
| 1.5b | `lp_planner.rs` | Before solving battery LP: run the rules engine for `pv`, `base_load`, `ev`, `heater` in the normal asset order to obtain their per-slot setpoints; compute `net_other_kw[t] = p_base[t] + p_ev_rules[t] + p_heater_rules[t] − p_pv[t]`; pass this fixed offset as an input to the battery LP so the power balance is complete |
| 1.6 | `lp_planner.rs` | Implement battery sub-LP: variables `p_bat_c[t]`, `p_bat_d[t]`, `soc_bat[t]`; power balance: `g_import[t] = net_other_kw[t] + p_bat_c[t] − p_bat_d[t]`; SoC continuity constraints; bounds from `BatteryConfig`; objective = minimise `Σ tariff_import[t] × g_import[t] × dt` |
| 1.7 | `lp_planner.rs` | Translate LP solution into per-slot battery setpoints; inject alongside the already-computed other-asset setpoints to build the full `Plan` |
| 1.7b | `planner.rs` | Remove Rule 8b from `rules_choose` — battery PV surplus absorption is now handled by the LP power balance; keeping Rule 8b would double-allocate surplus in the same slot |
| 1.8 | `VEN/profiles/ven-1.yaml` | Add `planner: lp` for testing; leave `ven-2.yaml`, `ven-3.yaml` on `rules` |
| 1.9 | BDD suite | Run full suite; verify battery slots change to globally optimal; verify EV/heater slots unchanged |

**Exit criterion:** BDD suite green with `planner: lp`; battery no longer uses
two-pass pre-planner; Rules 9 and 10 still present in `rules_choose` (removed in
Phase 2).

---

### Phase 2 — EV as LP variable (main planning change)

**Goal:** EV scheduling moves fully into the LP. `desired_power_kw` becomes a cap,
not a fixed setpoint. Bid is parameterised by `soc` not `fill`. Rules 6, 7, 8b, 9,
10 are removed from `rules_choose`.

| Step | File(s) | Task |
|------|---------|------|
| 2.1 | `lp_planner.rs` | Add EV variables `p_ev[t]`, `soc_ev[t]` to LP; domain `[0, max_charge_kw_ev]` and `[current_soc, soc_target]`; if `EvState.plugged == false`, set upper bound to 0 for all t (no EV charging while unplugged) |
| 2.2 | `lp_planner.rs` | Add EV SoC continuity: `soc_ev[t+1] = soc_ev[t] + p_ev[t] × dt / cap_ev` |
| 2.3 | `lp_planner.rs` | Add helper `deadline_slot(latest_end, now, dt_s) -> Option<usize>`: returns `None` if `latest_end ≤ now` (overdue — skip constraint, use max-urgency bid); returns `N−1` if `latest_end > now + horizon` (beyond window — charge economically, no deadline constraint); returns `floor((latest_end − now) / dt_s)` otherwise. Add soft deadline constraint: slack variable `s_ev ≥ 0`, `soc_ev[T_deadline] + s_ev ≥ soc_target`, penalty `M × s_ev` in objective (M = 10.0 EUR/kWh) |
| 2.4 | `lp_planner.rs` | Add EV bid term to objective: `− bid_at(soc_ev[0]) × p_ev[t]`; read bid from `packet.value_curve` evaluated at current SoC using `[soc_min, soc_target]` range (not `fill`) |
| 2.4b | `assets/ev.rs`, `profile.rs` | Add `soc_min: f64` (default 0.0) and `eta_charge: f64` (default 1.0) to `EvConfig`; `soc_min` anchors the lower end of the bid interpolation; `eta_charge` is used in the SoC continuity formula |
| 2.5 | `assets/ev.rs` | In `resolve_request_target`: change `desired_power_kw.unwrap_or(self.max_charge_kw)` to return `(kwh, desired_power_kw)` where `desired_power_kw` is `Option<f64>` — the LP uses it as an upper bound, not a fixed setpoint |
| 2.6 | `lp_planner.rs` | Use `packet.desired_power_kw` as the LP upper bound `p_ev[t] ≤ desired_power_kw` when set; do not rename the field in any struct or serialisation |
| 2.7 | `lp_planner.rs` | Full LP now includes both battery and EV; solve jointly; translate solution to `PlanStep` setpoints for both assets. Pre-solve: detect concurrent non-terminal packets targeting the same asset; merge into a single LP power variable bounded by `max_charge_kw`; compute merged bid as energy-weighted average. Post-solve: attribute delivered energy to packets in descending bid order |
| 2.8 | `planner.rs` | Remove Rules 6, 7 from `rules_choose` (EV scheduling now fully in LP); remove Rule 8b (battery surplus absorbed via LP power balance); remove Rules 9, 10 (battery arbitrage via LP); remove `plan_battery_grid_charges` call |
| 2.9 | `planner.rs` | Keep Rules 1, 2, 4a, 4, 5 (physics / hard constraint enforcement — these remain in the execution path as safety guards) |
| 2.10 | `entities/energy_packet.rs` | Add `LpOptimal { marginal_value_eur_kwh: f64, marginal_cost_eur_kwh: f64 }` variant to `PlanReason`; use it for all LP-decided allocations |
| 2.11 | BDD suite | Run full suite; verify EV charges directly from PV surplus without battery mediation; verify split-charge scenario resolves into continuous charging |

**Exit criterion:** BDD suite green; EV no longer defaults to `max_charge_kw`
setpoint; no battery-mediated EV charging in PV-surplus scenarios; `plan_battery_grid_charges` deleted.

---

### Phase 3 — Heater and multi-objective

**Goal:** heater scheduling enters the LP. Objective mode is selectable from the
profile. Piecewise-linear bid segments added for assets with large SoC swings.

| Step | File(s) | Task |
|------|---------|------|
| 3.0 | `VEN/src/assets/heater.rs` | Verify field names: confirm `k_loss` (thermal conductance, kW/°C) and `thermal_mass_kwh_c` (thermal capacity, kWh/°C) exist in `HeaterConfig`; update thermal continuity formula in §3.3 if names differ before any LP code is written |
| 3.1 | `lp_planner.rs` | Add heater variables `p_heater[t]`, `temp_room[t]`; domain bounds from `HeaterConfig` |
| 3.2 | `lp_planner.rs` | Add thermal continuity constraint (see §3.3); add comfort bounds `T_min ≤ temp_room[t] ≤ T_max` |
| 3.3 | `lp_planner.rs` | Add heater bid term: `− bid_heater(temp_room[0]) × p_heater[t]`; bid evaluated at current temperature (analogous to `bid_at(soc)`) |
| 3.4 | `profile.rs` | Add `objective: ObjectiveMode` field (`enum ObjectiveMode { Cost, Ghg, MinPeak, OpenAdr }`); default = `Cost` |
| 3.5 | `lp_planner.rs` | Implement objective mode switch: `Cost` uses tariff coefficients; `Ghg` uses `co2_g_kwh` coefficients; `MinPeak` adds `peak_import` auxiliary variable + per-slot bound; `OpenAdr` adds `w_dr` weight on DR-window slots |
| 3.6 | `lp_planner.rs` | Implement 3-segment piecewise-linear bid for EV and battery (split remaining SoC gap into thirds; see §3.5) |
| 3.7 | `planner.rs` | Remove heater rules from `rules_choose`; heater is now fully LP-scheduled |
| 3.8 | BDD suite | Add scenarios for GHG mode (charge shifts to low-CO₂ slots) and min-peak mode (load spread evenly); run full suite |

**Exit criterion:** BDD suite green including new objective-mode scenarios;
`rules_choose` contains only physics guards (Rules 1, 2, 4a, 4, 5); `planner.rs`
can be deleted and replaced entirely by `lp_planner.rs`.

---

### Phase 4 — Reactive execution layer (dispatcher)

**Goal:** formalise the dispatcher's sub-slot behaviour as explicit reactive
controllers R1–R4 (see §9.2). Replace the ad-hoc surplus EV overlay with a
systematic multi-asset surplus router.

| Step | File(s) | Task |
|------|---------|------|
| 4.1 | `controller/dispatcher.rs` | Extract current `apply_surplus_ev_overlay` into a general `route_surplus(surplus_kw, assets, plan, sim)` function that follows the R1 priority order: EV → battery → heater → export |
| 4.2 | `controller/dispatcher.rs` | Implement R2 deficit response: when `actual_pv < planned_pv`, compute import excess and trim flexible setpoints in reverse priority order (heater first, then battery, then EV) |
| 4.3 | `controller/dispatcher.rs` | Implement R3 battery buffer: proportional controller `bat_adj = clamp(−k_p × (actual_import − planned_import), −max_discharge, max_charge)`. Bound to prevent unplanned discharge: `bat_new = clamp(bat_lp_setpoint + bat_adj, max(0, bat_lp_setpoint − slack_down), min(max_charge_kw, bat_lp_setpoint + slack_up))` where `slack_down = bat_lp_setpoint` (cannot cross zero into discharge) and `slack_up = max_charge_kw − bat_lp_setpoint` (cannot exceed physical max); add `k_p` to profile with default 0.5 |
| 4.4 | `controller/dispatcher.rs` | Implement R4 partial-slot proration: when LP replans mid-slot, scale setpoint by `remaining_slot_fraction`; energy attribution continues from actual power measurement |
| 4.5 | `controller/dispatcher.rs` | Add boundary guards: assert reactive adjustments never exceed `import_cap`, never push SoC beyond bounds, never create negative export where not planned; verify that R1 surplus absorption does not push energy into `past_power_profile` or create new VTN report payloads |
| 4.6 | BDD suite | Add reactive-layer scenarios: PV spike mid-slot absorbed without replan; PV drop mid-slot curtails EV charge; battery smooths grid import; verify no VTN report entries for surplus-only charging |

**Exit criterion:** BDD suite green including reactive-layer scenarios; dispatcher
has no ad-hoc special cases — all sub-slot behaviour flows through R1–R4 handlers.

