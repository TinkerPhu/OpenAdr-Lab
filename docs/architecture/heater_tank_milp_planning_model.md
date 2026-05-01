# Heater + Thermal Tank Forecasting and MILP Planning Model

## Purpose

Design a two-layer control system for one or more thermal storage assets, such as a 2000 kg hot water heating tank and, later, an additional smaller 200 kg hot water boiler. The system plans the next 24 hours in 5-minute steps using forecasts and optimization.

Each resistive heater asset can have discrete electrical power levels, for example:

- 0 kW
- 3 kW
- 6 kW

The thermal tank supplies a secondary heating circuit for space heating. The optimizer should minimize economic cost under dynamic import/export tariffs while avoiding excessive mechanical relay switching.

The architecture must not be fixed to one heater only. It should support multiple assets in parallel, each with its own profile, thermal capacity, minimum/maximum temperature bounds, heater tiers, and switching model.

---

# Confirmed Assumptions

1. The heater is purely resistive.
   - Electrical input power maps approximately 1:1 to thermal power.
   - Heater efficiency is modeled as `η = 1`.

2. Export tariff is relevant.
   - When export tariff is high, PV/battery/other generators should preferably export instead of being consumed by the heater.
   - Therefore heater operation should consider opportunity cost of consuming locally available exportable energy.

3. Indoor comfort is not directly optimized in the MILP.
   - Indoor temperature requirement is used mainly to calculate forecast heat demand from indoor/outdoor temperature difference.
   - The MILP focuses on tank minimum/maximum temperature or equivalent energy bounds.

4. The 40°C minimum tank temperature is not an absolute hard minimum.
   - It may be violated briefly because the heating system reacts slowly.
   - Maximum tolerated violation duration: 1 hour.
   - After such an interruption, full-tier heating should be required to recover.
   - Actual minimum and maximum temperatures are read from the asset profile file.

5. Planning resolution can be 15 minutes for heater mode stability.
   - However, replanning may occur faster because PV, baseline assets, or forecasts can change quickly.
   - The system should support rapid receding-horizon replanning while still discouraging frequent relay switching.

6. Main hot water tank mass is 2000 kg.
   - Tank volume/mass is read from the asset profile file.
   - A future additional asset may be a 200 kg hot water boiler.
   - The model should support multiple thermal assets in parallel.

7. Switching penalty represents mechanical power relay wear.

8. The heater uses a delta-like resistor/relay schema.
   - Two power relays are involved.
   - One relay on gives 3 kW.
   - Both relays on gives 6 kW.
   - Both relays can switch at the same time.
   - Therefore direct 0 ↔ 6 transitions are allowed.
   - 0 ↔ 6 transitions are penalized 20% higher because two relays switch.

---

# Core Architecture: Split Into Two Models

The system is intentionally split into two parts.

## Layer A: Physical Heatflow Forecast Model

This model predicts how much thermal power the building or downstream process will require from each thermal asset over the next 24 hours.

Inputs:

- Outside temperature forecast
- Indoor temperature requirement plan
- Current tank temperature or tank energy
- Asset profile file
- Building heat-loss parameters
- Tank standby-loss parameters
- Optional measured baseline asset behavior

Outputs per 5-minute interval:

- Required thermal demand from asset `Q_dem[a,t]` in kW-th
- Optional standby tank loss `Q_loss[a,t]` in kW-th
- Optional forecast confidence band or scenarios

This layer answers:

> How much heat is expected to leave this thermal asset?

It does not decide heater switching.

## Layer B: MILP Planning Model

This model receives the heat demand forecast and chooses heater states.

Inputs:

- Import tariff forecast
- Export tariff forecast
- Forecast local generation / battery export opportunity, if available
- `Q_dem[a,t]`
- `Q_loss[a,t]`
- Initial energy state for each asset
- Asset profile constraints
- Heater relay/switching constraints

Outputs:

- Heater power plan for each asset
- Expected tank energy / temperature trajectory
- Expected import cost and export opportunity cost
- Expected relay switching count/cost

---

# Why This Split Is Good

The physical model and the optimizer solve different problems.

The physical model predicts demand:

> How much heat will the house draw from the tank?

The MILP planner schedules supply:

> When should the resistive heater add heat to the tank?

Benefits:

- Keeps the MILP mostly linear and robust
- Allows calibration of thermal physics separately
- Allows replacement of the heatflow model later
- Supports multiple assets
- Supports fast replanning
- Easier debugging and explainability

---

# Time Discretization

Forecast horizon:

- 24 hours
- 5-minute forecast/state steps
- `T = 288`

Step duration:

```text
Δt = 5/60 = 1/12 h
```

The optimizer may still enforce 15-minute planning blocks or dwell constraints, even if state simulation and forecasts remain at 5-minute resolution.

---

# Asset-Based Model

Use an asset index:

```text
a ∈ A
```

Example assets:

- `space_heat_tank_2000kg`
- `dhw_boiler_200kg`

Each asset has its own profile:

```text
mass[a]
T_min[a]
T_max[a]
P_tiers[a]
initial_temperature[a]
relay_schema[a]
standby_loss_params[a]
```

For the current main tank:

```text
mass = 2000 kg
T_min = profile value, expected around 40°C
T_max = profile value, expected around 80°C
P_tiers = {0, 3, 6} kW
η = 1
```

---

# Thermal Units

Two power domains are tracked:

- Electrical power: kW
- Thermal power: kW-th

For a resistive heater:

```text
1 kW electric ≈ 1 kW-th thermal
η = 1
```

Thermal energy is stored as kWh-th.

---

# Tank Energy State

Use thermal energy above the asset minimum temperature as MILP state.

For each asset `a`:

```text
E[a,t] = usable stored thermal energy above T_min[a]
```

So:

```text
E[a,t] = 0              means T_tank[a,t] = T_min[a]
E[a,t] = E_max[a]       means T_tank[a,t] = T_max[a]
```

---

# Thermal Capacity

For water mass `m[a]`:

```text
E_max[a] = mass[a] * c_p * (T_max[a] - T_min[a]) / 3600
```

Where:

```text
c_p = 4.186 kJ/(kg K)
```

For the 2000 kg tank with 40°C to 80°C:

```text
E_max ≈ 2000 * 4.186 * 40 / 3600
E_max ≈ 93.0 kWh-th
```

For a future 200 kg boiler with the same temperature range:

```text
E_max ≈ 9.3 kWh-th
```

---

# Convert Energy to Temperature

For each asset:

```text
T_tank[a,t] = T_min[a] + (T_max[a] - T_min[a]) * E[a,t] / E_max[a]
```

---

# Physical Heatflow Forecast Model

## Simple Heat Demand Model

The physical forecast layer may start with:

```text
Q_dem[a,t] = max(0, α[a] + β[a] * (T_in_req[t] - T_out[t]))
```

Where:

- `T_in_req[t]` is the indoor temperature requirement plan
- `T_out[t]` is outside temperature forecast
- `α[a]` is base demand
- `β[a]` is heat-loss coefficient in kW/K

Indoor comfort itself is not optimized in the MILP. `T_in_req[t]` is primarily used to estimate thermal demand.

---

## Optional Tank Standby Loss

Tank standby loss can be approximated linearly:

```text
Q_loss[a,t] = c0[a] + c1[a] * E[a,t]
```

If this dependence on `E[a,t]` is included in the MILP, it remains linear:

```text
E[a,t+1] = (1 - c1[a]*Δt) * E[a,t] + P_heat[a,t]*Δt - (Q_dem[a,t] + c0[a]) * Δt
```

A simpler first implementation can precompute or ignore standby loss.

---

# MILP Planning Model

## Index Sets

```text
a ∈ A       thermal assets
t ∈ {0,...,T-1} time steps
```

---

## Parameters

```text
Δt                    step duration in hours
c_imp[t]              import tariff in €/kWh
c_exp[t]              export tariff in €/kWh
Q_dem[a,t]            forecast heat demand in kW-th
Q_loss[a,t]           forecast standby loss in kW-th
E_max[a]              usable storage capacity in kWh-th
E_init[a]             initial tank energy in kWh-th
η[a]                  heater efficiency, η[a]=1 for resistive heaters
λ_sw[a]               switching penalty base coefficient
```

For each asset, `T_min`, `T_max`, `mass`, and power tiers come from the asset profile file.

---

# Heater Mode Variables

For the current 0/3/6 kW heater:

```text
y3[a,t] ∈ {0,1}
y6[a,t] ∈ {0,1}
P[a,t]  ≥ 0
```

Power selection:

```text
P[a,t] = 3*y3[a,t] + 6*y6[a,t]
y3[a,t] + y6[a,t] <= 1
```

Modes:

```text
y3=0, y6=0  -> 0 kW
y3=1, y6=0  -> 3 kW
y3=0, y6=1  -> 6 kW
```

---

# Tank State Transition

Without state-dependent standby loss:

```text
E[a,t+1] = E[a,t] + η[a] * P[a,t] * Δt - (Q_dem[a,t] + Q_loss[a,t]) * Δt
```

For confirmed resistive heater:

```text
η[a] = 1
```

---

# Tank Bounds

Nominal operating range:

```text
0 <= E[a,t] <= E_max[a]
```

This corresponds to:

```text
T_min[a] <= T_tank[a,t] <= T_max[a]
```

However, the lower bound is not absolutely hard for the main heating tank.

---

# Soft Minimum Temperature Violation

Because the heating system is slow, the tank may temporarily fall below `T_min`, but not for more than 1 hour.

Introduce violation binary or slack variables.

## Slack Form

```text
E[a,t] + s_low[a,t] >= 0
s_low[a,t] >= 0
```

Then penalize `s_low[a,t]` strongly in the objective.

## Maximum Violation Duration

Let:

```text
v_low[a,t] ∈ {0,1}
```

indicate that asset `a` is below its nominal minimum at time `t`.

Use a big-M relation:

```text
E[a,t] >= -M_low[a] * v_low[a,t]
E[a,t] <= E_max[a]
```

Then limit rolling violation duration. For 5-minute steps, 1 hour = 12 steps.

For every rolling 13-step window:

```text
Σ_{τ=t}^{t+12} v_low[a,τ] <= 12
```

This prevents more than 12 consecutive below-minimum steps.

A stricter version can require recovery after at most 12 steps.

---

# Full-Tier Recovery After Violation

If the tank violates the minimum, the system should recover using the full tier.

For a 0/3/6 kW heater, full tier means:

```text
P[a,t] = 6 kW
```

A practical rule:

```text
if v_low[a,t] = 1 then y6[a,t] = 1
```

MILP form:

```text
y6[a,t] >= v_low[a,t]
```

This means whenever the tank is below its nominal minimum, the heater must run at 6 kW.

If more conservative behavior is wanted, add recovery until `E[a,t] >= E_recover[a]`, for example above 40°C plus a buffer.

---

# Import and Export Tariff Treatment

## Import Tariff

The normal heater electricity cost is:

```text
Cost_import = Σ_a Σ_t c_imp[t] * P_grid_to_heater[a,t] * Δt
```

If all heater power is imported from the grid:

```text
P_grid_to_heater[a,t] = P[a,t]
```

## Export Tariff / Opportunity Cost

Export tariff matters because local generation or battery discharge may be better exported than consumed by the heater.

If local exportable power is available, consuming it in the heater has an opportunity cost:

```text
opportunity cost = c_exp[t] * P_local_to_heater[a,t] * Δt
```

Thus heater energy cost can be modeled as:

```text
Cost_energy = Σ_t c_imp[t] * P_import_used[t] * Δt
            + Σ_t c_exp[t] * P_export_opportunity_used[t] * Δt
```

Where:

- `P_import_used[t]` is heater power supplied from grid import
- `P_export_opportunity_used[t]` is local generation/battery power consumed by the heater instead of exported

If the planner does not explicitly model PV/battery flows, a simplified effective heater price can be used:

```text
c_eff[t] = max(c_imp[t], c_exp[t])
```

or another site-energy-manager-provided marginal value of consuming electricity at time `t`.

Then:

```text
Cost_energy = Σ_a Σ_t c_eff[t] * P[a,t] * Δt
```

This reflects that when export value is high, heater consumption is economically unattractive.

---

# Switching Penalty Design

Switching penalty represents mechanical power relay wear.

The relay schema is:

- one relay active -> 3 kW
- two relays active -> 6 kW
- both relays may switch simultaneously
- direct 0 ↔ 6 is allowed

Base switching penalty:

```text
λ_sw[a]
```

Transition weights:

```text
0 -> 3 : 1.0 * λ_sw
3 -> 0 : 1.0 * λ_sw
3 -> 6 : 1.0 * λ_sw
6 -> 3 : 1.0 * λ_sw
0 -> 6 : 1.2 * λ_sw
6 -> 0 : 1.2 * λ_sw
no change : 0
```

The 20% higher penalty for 0 ↔ 6 approximates both relays switching.

---

# Transition Binary Formulation

Define mode binaries:

```text
m0[a,t], m3[a,t], m6[a,t] ∈ {0,1}
```

Exactly one mode:

```text
m0[a,t] + m3[a,t] + m6[a,t] = 1
```

Power:

```text
P[a,t] = 0*m0[a,t] + 3*m3[a,t] + 6*m6[a,t]
```

Transition binaries:

```text
u03[a,t], u30[a,t], u36[a,t], u63[a,t], u06[a,t], u60[a,t] ∈ {0,1}
```

For transition `i -> j`, activate:

```text
uij[a,t] >= mi[a,t-1] + mj[a,t] - 1
```

Example:

```text
u06[a,t] >= m0[a,t-1] + m6[a,t] - 1
```

Switching cost:

```text
Cost_switch = Σ_a Σ_t λ_sw[a]*(u03+u30+u36+u63)
            + Σ_a Σ_t 1.2*λ_sw[a]*(u06+u60)
```

---

# 15-Minute Planning Stability

Even though forecasts and states are 5-minute resolution, heater mode can be held for 15 minutes.

For 5-minute steps, 15 minutes = 3 steps.

Block hold constraint:

```text
P[a,3k] = P[a,3k+1] = P[a,3k+2]
```

or equivalently:

```text
m_i[a,3k] = m_i[a,3k+1] = m_i[a,3k+2]
```

for each mode `i ∈ {0,3,6}`.

However, replanning may occur faster than 15 minutes. In receding-horizon use, the controller can re-solve frequently but should respect relay wear and optional hold/dwell rules unless safety recovery requires override.

---

# Full Objective Function

A practical objective:

```text
Minimize J = Cost_energy
           + Cost_switch
           + Cost_low_temp_violation
```

Where:

```text
Cost_low_temp_violation = M_low * Σ_a Σ_t s_low[a,t]
```

or, if binary violation is used:

```text
Cost_low_temp_violation = M_v * Σ_a Σ_t v_low[a,t]
```

with `M_low` or `M_v` much larger than normal tariff savings.

---

# Complete MILP Summary

For each asset `a` and time step `t`:

## Variables

```text
m0[a,t], m3[a,t], m6[a,t] ∈ {0,1}
P[a,t] ≥ 0
E[a,t]
s_low[a,t] ≥ 0 optional
v_low[a,t] ∈ {0,1} optional
u03,u30,u36,u63,u06,u60 ∈ {0,1}
```

## Constraints

Mode selection:

```text
m0[a,t] + m3[a,t] + m6[a,t] = 1
P[a,t] = 3*m3[a,t] + 6*m6[a,t]
```

Tank dynamics:

```text
E[a,t+1] = E[a,t] + P[a,t]*Δt - (Q_dem[a,t] + Q_loss[a,t])*Δt
```

Upper bound:

```text
E[a,t] <= E_max[a]
```

Soft lower bound:

```text
E[a,t] + s_low[a,t] >= 0
```

or binary below-minimum state:

```text
E[a,t] >= -M_low[a] * v_low[a,t]
```

Full-tier recovery:

```text
m6[a,t] >= v_low[a,t]
```

Maximum below-minimum duration:

```text
Σ_{τ=t}^{t+12} v_low[a,τ] <= 12
```

Transition activation:

```text
uij[a,t] >= mi[a,t-1] + mj[a,t] - 1
```

Optional 15-minute hold:

```text
m_i[a,3k] = m_i[a,3k+1] = m_i[a,3k+2]
```

Objective:

```text
Minimize Σ_t c_eff[t] * Σ_a P[a,t] * Δt
       + Σ_a Σ_t λ_sw[a]*(u03+u30+u36+u63)
       + Σ_a Σ_t 1.2*λ_sw[a]*(u06+u60)
       + low-temperature violation penalty
```

---

# Why 5-Minute Heat Demand Is Enough

A 5-minute heat demand forecast is sufficient because the tank energy state couples all time steps.

The MILP can heat before demand occurs, store energy, and let the tank discharge later.

The forecast says:

> How much heat leaves the tank in each interval.

The planner decides:

> How much heat to put into the tank in each interval.

The tank state provides accumulation.

---

# Receding-Horizon Operation

Recommended operational loop:

1. Read current asset states
2. Read updated outside temperature forecast
3. Read updated import/export tariff forecast
4. Read PV/battery/baseline asset forecasts if available
5. Generate 24h heatflow forecast
6. Solve MILP
7. Apply first heater decision or current control block
8. Repeat frequently

Replanning can occur faster than the nominal 15-minute control block if forecasts or baseline asset behavior change significantly.

---

# Multiple Asset Extension

The same structure supports multiple assets in parallel.

Each asset has:

- own energy state `E[a,t]`
- own thermal capacity `E_max[a]`
- own heater tiers
- own heat demand forecast
- own switching penalty
- own min/max temperature profile

The objective sums across assets.

If assets serve different loads, each gets its own `Q_dem[a,t]`.

If assets can share thermal load, an additional allocation variable can be introduced:

```text
Q_supply[a,t]
Σ_a Q_supply[a,t] >= Q_total_dem[t]
```

This is a later extension.

---

# Open Clarifications Remaining

1. Should the 1-hour low-temperature violation limit apply to every asset, or only the 2000 kg space-heating tank?
2. Should low-temperature recovery require 6 kW until reaching exactly `T_min`, or until a higher recovery buffer such as `T_min + 2°C`?
3. Should 15-minute mode holding be a hard constraint, or only a preference via switching penalties?
4. Will the site energy manager provide an effective marginal electricity price `c_eff[t]`, or should the MILP explicitly model import/export/PV/battery allocation?
5. Is the 3 kW state physically either relay A or relay B, and should the model balance wear between relays, or is aggregate 3 kW sufficient?

