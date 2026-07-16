---
title: Asset Layer
type: component
created: 2026-07-04
updated: 2026-07-16
synced_commit: f08e469
sources: [VEN/src/assets/, VEN/src/simulator/mod.rs, VEN/src/controller/residual.rs, docs/architecture/VEN_ARCHITECTURE.md, docs/architecture/ven_asset_interface_spec.md]
tags: [assets, abstraction, ven]
---

# Asset Layer

The device abstraction between the HEMS controller and the physics that produces the
numbers (`VEN/src/assets/`). Three cooperating pieces:

- **`Asset` trait** (`assets/mod.rs:545`) — the physics contract:
  `step(state, setpoint_kw, dt) -> (new_state, actual_kw)`, `capability(state)`
  (point-in-time feasible power range), plus default-implemented `simulate_forward`,
  `simulate_free`, and `capability_trajectory`. Identity/history methods (`id`,
  `current_state`, `history`) are provided by `AssetHandle`, which wraps a
  config+entry pair.
- **`AssetConfig` / `AssetState` enums** — config (physics parameters) and mutable
  runtime state are separate enums with one variant per asset type (Battery, Ev,
  Heater, Pv, BaseLoad); `AssetConfig` dispatch methods (`state_values`,
  `control_schema`, `forecast`, `available_storage_kwh`, `build_milp_context`, …) are
  the single switchboard. Adding an asset type = one new variant + one module.
- **Per-asset history**: every `AssetEntry` in `SimState` carries a ring buffer of
  3600 `HistoryPoint`s (≈ 1 h at 1 s tick) with LOCF lookups and time-weighted
  averaging — this feeds `/timeline`, `/history/:id`, and obligation reports
  ([[openadr-interface]]).

A virtual **Grid asset** (`assets/grid.rs`, held as `SimState.grid_asset`) tracks net
site power plus the VTN capacity limits each tick and keeps its own history; it is
read-only — never dispatched.

A second read-only virtual asset, **`site-residual`** (`controller/residual.rs`,
Phase 5 WP5.1), is inserted into snapshots rather than living in `SimState`:
`residual_kw = grid meter − Σ modelled asset power`, the unmodelled background
load the planner budgets for and the learning pipeline trains on
([[heuristics-pipeline]]). Zero import/export capability marks it
point-reading-only.

**BaseLoad appliance noise** (Phase 5): a profile-configured `base_load.spikes`
list adds trapezoidal daily appliance pulses (plateau at `amplitude_kw`, linear
ramps, timing/magnitude jitter, optional weekday restriction, per-day firing
probability; empty by default). Trapezoids, not Gaussians, because a
trapezoid's energy is directly `≈ amplitude_kw × (duration_h − ramp_h)` —
settable to match a real appliance session ([[heuristics-pipeline]]).

> **DRIFT** `docs/architecture/VEN_ARCHITECTURE.md` §3.0 specifies a
> `trait AssetInterface { current(); forecast(horizon); past(window) }` with
> `SimulatedAsset`/`MeasuredAsset` implementations. None of these identifiers exist in
> the code — the shape above (`Asset` + `AssetConfig` + `AssetHandle`) is what was
> actually built. The *intent* survives (controller code consumes `SimSnapshot`s and
> forecasts, never physics internals), but the doc section reads as an API reference for
> an API that isn't there. See [[ven-code-vs-docs-audit]].

## Planning-side counterpart

For the [[milp-planner]], each controllable asset provides an `AssetMilpContext` —
its constraints and variables in solver terms. The trait is declared at the planner
boundary (`controller/milp_planner/asset_port.rs`) and implemented in `assets/battery.rs`,
`assets/ev.rs`, `assets/heater.rs` (cross-file inherent impls), so the solver only ever
sees trait objects ([[ven-hexagonal-architecture]]). PV and base load are not
MILP-controllable; their forecasts enter as per-slot input arrays.

Sign convention for all power values crossing this interface: positive = import,
negative = export/generation — see [[sign-convention]].
