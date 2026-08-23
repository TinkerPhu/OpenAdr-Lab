# Forecasting Model — Exogenous Drivers vs. Endogenous Response

How the VEN decides what it thinks the future looks like: where each forecast comes from,
which parts can be sourced externally, which parts must be generated inside the planning
loop, and how the system handles the certainty that reality will diverge from all of it.

This is the conceptual layer above the individual mechanisms. For the mechanisms themselves
see [weather_forecast.md](weather_forecast.md), [real_measurement_mqtt.md](real_measurement_mqtt.md),
[ven_milp_planner.md](ven_milp_planner.md), and [VEN_ARCHITECTURE.md](VEN_ARCHITECTURE.md).

## 1. The central split is per-driver, not per-asset

It is tempting to sort assets into two piles — "controllable ones we simulate" and
"uncontrollable ones we forecast externally." That split does not survive contact with the
code. **Every asset has both an exogenous driver and an endogenous response:**

| | Exogenous driver (from outside) | Endogenous response (decided by us) |
|---|---|---|
| PV | Weather forecast → `weather_pv_kw` | Curtailment (`p_pv_used` decision variable) |
| Heater | Ambient temperature, user comfort setpoints | Dispatch within the comfort envelope |
| EV | Plug-in/plug-out times, user energy request | Charge schedule within the deadline |
| Battery | — (no external driver) | Charge/discharge schedule |
| Base load | Occupant behaviour (unobservable) | — (non-controllable) |

PV is the clearest case and the reference implementation: the weather feed supplies the
natural generation trajectory (`controller/milp_planner/inputs.rs`, `weather_pv_kw`), and the
solver's own curtailment variable modifies it. Neither half alone is the forecast.

**Why the endogenous half can never be outsourced:** an external forecaster cannot know our
dispatch plan. The moment the planner decides to charge the EV at 13:00, the site's projected
load diverges from any externally-produced projection. That is why the forward trajectories
for controllable assets are produced *inside* the MILP as decision variables, not consumed as
inputs.

**Why the exogenous half should be outsourced where possible:** we have no privileged
information about tomorrow's cloud cover. Where a real external source exists, it beats any
internal model.

## 2. What actually arrives from outside

Three categories, and only these:

1. **Weather forecast** — the PV driver. See [weather_forecast.md](weather_forecast.md).
2. **VTN signals** — tariffs, CO₂ intensity, events, obligations, capacity limits. Forecasts
   of things the site cannot influence, delivered over OpenADR.
3. **Present measurements** — what the meters say *right now*, via `MeasurementPort`
   (see [real_measurement_mqtt.md](real_measurement_mqtt.md)).

**Known gap:** only PV and base_load currently have measurement feeds
(`assets/pv.rs::measured_power_kw`, `assets/base_load.rs::measured_load_kw`). EV, heater, and
battery have no `MeasurementPort` — their "now" state is the VEN's own simulated state, not a
measured one. Closing that gap would let each replan re-anchor those assets on reality rather
than on the accumulated result of its own physics model.

## 3. Heuristics are not second-best simulation

`base_load` is forecast by a learned statistical heuristic
(`services/heuristics.rs::learn_asset_heuristics`, WP5.2/BL-14), not by a physics model. This
is not a placeholder awaiting a proper simulator.

Base load's driver is **occupant behaviour, which is unobservable**. No amount of physics
modelling makes someone's decision to run the dishwasher predictable from first principles.
Learning the statistical regularity from history is the correct and permanent tool for that
category — the same way physics is the correct tool for a battery's state of charge.

The live precedence chain (`simulator/mod.rs`, `natural_base_kw`):

```
measured (fresh MQTT reading)  →  learned heuristic  →  synthetic profile + appliance noise
```

Each tier is a fallback for the one before: a real reading wins when the feed is alive; the
site's own learned profile covers a dropout (BL-40/R-60); the invented spike model is a true
cold-start last resort only.

## 4. Base load is the site's whole unmetered-consumption story

`base_load` is, by definition, everything the VEN does not model as an individual asset —
standby draw, plugged-in appliances, lighting, anything not wired up as its own `AssetConfig`.

Critically, in a real deployment it is **not** an invented number: it is derived externally as

```
base_load = grid_meter_true − Σ(other real asset measurements)
```

and fed to the VEN as `base_load`'s own measurement. This matters for what follows.

## 5. Why there is no separate "site residual" channel

A virtual `site-residual` asset existed until 2026-08 (BL-08 / Phase 5 WP5.1), defined as
`grid_meter_kw − Σ(modelled asset power)` and intended to surface consumption the planner
could not otherwise see. It was removed, for reasons that are structural rather than
incidental:

**It is algebraically forced to zero.** Substituting §4's definition of `base_load`:

```
residual = grid − (base_load + Σothers)
         = grid − ((grid − Σothers) + Σothers)
         = 0
```

This is a tautology. It holds regardless of which assets are simulated and which are really
metered — a fully-metered site produces the same zero. The residual cannot detect a gap that
`base_load` misses, because by construction no such gap is left over.

The only quantity that could still make it non-zero is disagreement between the VEN's internal
model of EV/heater/battery power and the values the external system used in its own
subtraction (see §2's measurement gap). But that is *tracking error in our own controllable-asset
models* — several assets' errors conflated into one unattributable number, which is neither
what the channel claimed to measure nor a usable diagnostic.

**In simulation it was doubly circular**, since `SimState::derive_grid_meter` computes the
meter *as* the sum of modelled assets — so the two terms could never disagree. See the WP5.1
entry in [../reference/KEY_LEARNINGS.md](../reference/KEY_LEARNINGS.md).

## 6. How divergence is actually handled

Reality always differs from the plan — not only through measurement error, but because
occupant intent is unknowable (someone feels cold and raises the heater setpoint). No model
fidelity addresses that. The system handles it with two distinct mechanisms, which is why a
third "correction channel" adds nothing:

- **Structurally — receding-horizon replanning.** Each planning cycle re-anchors on the
  measured NOW state and re-solves over a fresh horizon. Divergence is not corrected after
  the fact; it is dissolved by the next replan starting from where reality actually is.
  Faster deviations are absorbed between replans by the deviation arbiter
  (see [VEN_ARCHITECTURE.md](VEN_ARCHITECTURE.md)) — a different concept from this page's
  "residual", despite the shared word.
- **Measured — forecast-accuracy tracking.** `forecast_accuracy_samples` (schema v8) persists
  near- and far-lead predictions for PV and base_load and reconciles each against the actual
  value once its target time elapses, surfaced on the History page. This is where "how well
  did our forecasts hold up" is answered.

Divergence is a normal operating condition to be re-anchored and measured, not an error to be
explained away by a compensating term.
