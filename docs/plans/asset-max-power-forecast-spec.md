# Asset Max Power/Energy Forecast Function — Specification

## Purpose & Goals

The house Energy Management System (EMS) computes a 48h rolling plan for all
controllable assets (PV, EV, battery, time-shiftable loads, etc.) based on an
optimization objective. This plan is a *hypothetical forecast* — it represents
one particular scenario, not the physical limits of what each asset could do
if pushed.

For system-level decisions (demand response, flexibility markets, site
capacity/headroom reporting, contingency planning), the EMS needs to know,
**for each asset independently**, how far it could deviate from its plan if
asked to — i.e. its maximum import and export capability under a hypothetical
"engage at max power" scenario, at any point in the planning horizon, for any
duration.

This document defines a function that computes that capability as a
continuous surface over a 2D time domain, per asset, and the requirements
each asset implementation and the surrounding environment (planner, EMS core)
must satisfy to support it.

**Goals:**
- Provide a uniform, asset-agnostic interface for "what's the most this asset
  could import/export, starting at some future time, for some duration."
- Keep all asset-specific physics (SoC dynamics, weather dependence, time
  windows, degradation, efficiency) encapsulated inside each asset's own
  implementation.
- Produce output that can be aggregated across assets into site-level
  headroom, capability, and capacity forecasts.
- Recompute cleanly whenever the plan changes or as time advances (rolling
  horizon), without requiring structural changes to the function itself.

---

## Domain

Let:
- **t1** ≥ 0 — offset from *now* at which a hypothetical max-power engagement
  begins. The asset follows its existing plan un-modified from now until t1.
- **t2** ≥ 0 — sweep variable tracing the max-power trajectory forward from
  t1. It represents "how long the engagement has been running" at the point
  being evaluated.
- **t3** — fixed, the plan horizon duration (currently 48h). Not a function
  input in the usual sense; it defines the domain boundary: `t1 + t2 ≤ t3`.

The valid domain of (t1, t2) is therefore a right triangle:

```
t2
 ^
 |\
 | \
 |  \
 |   \  t1 + t2 = t3
 |    \
 |_____\___> t1
 0      t3
```

- **(0, 0)** is the only non-hypothetical point on the domain: it represents
  the asset's actual max import/export capability *right now*.
- **t2 = 0** for any t1 means the engagement scenario has not started yet —
  the result equals the plan's own (unmodified) max-power capability at t1.

---

## Function Definition

### Per-asset primitive

Each asset must implement a common interface:

```
assetMaxPower(planState, t1, t2, direction, limitTier)
  → (power, energy)
```

- **planState** — the asset's plan-forecasted state at time `now + t1`
  (the seed/handoff point for the hypothetical trajectory). This is
  asset-specific in meaning (e.g. SoC for battery/EV, remaining
  energy-to-deliver and remaining time-window for a shiftable load,
  effectively nothing/weather-index for PV) but generic in role: it is
  whatever internal state variable bounds the asset's future max power.
- **t1, t2** — as defined above.
- **direction** — `import` | `export`. Determines which side the asset
  optimizes for, and may imply different initial-condition assumptions
  per asset type (see Asset-Specific Requirements).
- **limitTier** — enum (e.g. `physical` | `contractual` | `userSet`),
  fixed for the whole call, forwarded uniformly. Determines which
  power/energy ceiling the asset applies internally.
- **power** (return) — the max power the asset can *still* deliver at
  time `now + t1 + t2`, having followed the max-power trajectory since
  t1. This is a trajectory-endpoint value, not a sustained/constant
  value over the interval.
- **energy** (return) — the cumulative max import/export energy
  integrated over `[t1, t1+t2]` along that same trajectory:
  `energy(t1, t2) = ∫ from t1 to t1+t2 of power(t1, τ) dτ`

  Since the asset already simulates the full trajectory internally to
  compute the power endpoint, it should return both values from the same
  simulation pass — no separate integration step is needed outside the
  asset.

- **Sign convention:** positive = import, negative = export.

### Triangle-builder function

The EMS core function that produces the full surfaces:

```
maxPower(t1, t2, limitTier)  → (maxImport, maxExport)
maxEnergyForecast(t1, t2, limitTier) → (maxImportEnergy, maxExportEnergy)
```

For each (t1, t2) in the triangular domain:
1. Look up `planState(t1)` from the current plan.
2. Call `assetMaxPower(planState, t1, t2, import, limitTier)` and
   `assetMaxPower(planState, t1, t2, export, limitTier)`.
3. Assemble the four resulting scalar surfaces:
   max-import power, max-export power, max-import energy, max-export
   energy — each a function over the same (t1, t2) triangle.

The triangle-builder does **not** simulate the t1→t1+t2 trajectory itself;
that responsibility belongs entirely to the asset (see below). It only
provides the seed state and assembles results per grid point.

No general monotonicity is assumed across t2 for fixed t1 — capability can
rise or fall depending on asset-specific properties (e.g. a shiftable load's
import capability can *increase* once its earliest-start constraint has
passed; PV export capability can vary non-monotonically with the weather
forecast).

---

## Requirements on Assets

Each asset implementation is responsible for:

1. **Simulating its own trajectory** from `planState(t1)` forward, for
   duration t2, under the max-power engagement scenario — including all
   physics: state dynamics (e.g. SoC), efficiency, losses, degradation,
   weather dependence, time-window constraints, and the `limitTier`
   ceiling. The EMS core has no knowledge of these dynamics.
2. **Clamping behavior at physical limits**: if the trajectory drives the
   asset's internal state to a limiting bound (e.g. battery SoC full or
   empty), the corresponding capability drops to 0 from that point in the
   trajectory onward (import → 0 at full, export → 0 at empty). Other
   asset types define their own equivalent limiting behavior.
3. **Returning both power and energy** from a single simulation pass, per
   the `assetMaxPower` contract above.
4. **Direction-specific initial-condition assumptions**, where relevant.
   For example, for a **shiftable load**:
   - `import` direction assumes the load starts at its **earliest allowed
     start time** (maximizes remaining time-budget to draw power).
   - `export` direction — shiftable loads are consumption-only, so this
     represents *minimum forced import* rather than literal power
     feedback; it assumes the load starts at its **latest allowed start
     time** (minimizes power draw within the window).
   - **Open item:** confirm system-wide whether "export" for a
     consumption-only asset means literal negative power or "maximum
     downward flexibility toward zero import" — this affects how
     site-level aggregation should interpret the value.
5. **Not handling forecast uncertainty** (weather, driver behavior, usage
   patterns) inside this function — uncertainty is resolved upstream in
   the planning stage. The plan and `planState` handed to the asset are
   treated as given/deterministic inputs here.
6. **Not handling efficiency/loss modeling exceptions** — these are
   already part of the asset's own max-power physics (item 1) and are not
   a separate concern of the triangle-builder.

---

## Requirements on the Environment / EMS Core

1. **Provide `planState(t1)`** — the forecasted state of each asset at any
   offset t1 within the horizon, derived from the current plan.
2. **Trigger recomputation** of the full (t1, t2) surfaces whenever the
   plan changes or as time advances (rolling horizon). The recomputation
   frequency is an implementation/scheduling concern tied to planning
   interval frequency, not part of this function's definition.
3. **Supply `limitTier`** uniformly per call — one tier per full surface
   computation, not varying across (t1, t2) within a single call.
4. **Grid resolution**: the (t1, t2) domain is conceptually continuous;
   in practice discretized (nominal: 1-minute resolution) for
   computation and storage.
5. **Aggregate per-asset results** into site-level outputs:
   - site headroom
   - site capability
   - site capacity
   
   forecasts, by combining the max-import/max-export power and energy
   surfaces across all assets. The aggregation method itself is out of
   scope for this document.

---

## Open Items / Not Yet Decided

- Whether "export" for consumption-only assets (shiftable loads, fixed
  loads) should be represented as literal negative power or reserved to
  mean "flexibility toward reduced import" — needs a system-wide
  convention.
- Exact grid resolution/storage strategy for the discretized triangle in
  production (performance vs. fidelity trade-off).
- Whether `maxEnergyForecast` will typically be consumed at the domain
  boundary (t2 = t3 − t1, i.e. "integrate to horizon end") or as a general
  windowed function — both are supported by the definition above; usage
  patterns will clarify which matters most in practice.
