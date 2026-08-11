---
title: Asset Layer
type: component
created: 2026-07-04
updated: 2026-08-09
synced_commit: 329444a
sources: [VEN/src/assets/, VEN/src/simulator/mod.rs, VEN/src/controller/residual.rs, docs/architecture/VEN_ARCHITECTURE.md, docs/architecture/ven_asset_interface_spec.md, VEN/src/entities/asset_params.rs, VEN/src/entities/sim_inject.rs]
tags: [assets, abstraction, ven]
---

# Asset Layer

The device abstraction between the HEMS controller and the physics that produces the
numbers (`VEN/src/assets/`). Three cooperating pieces:

- **`Asset` trait** (`assets/asset_trait.rs`, split out of `assets/mod.rs` by the R-08
  refactor below) — the physics contract: `step(state, setpoint_kw, dt) -> (new_state,
  actual_kw)`, `capability(state)` (point-in-time feasible power range), plus
  default-implemented `simulate_forward`, `simulate_free`, and `capability_trajectory`.
  Identity/history methods (`id`, `current_state`, `history`) are provided by
  `AssetHandle`, which wraps a config+entry pair.
- **`AssetConfig` / `AssetState` enums** — config (physics parameters) and mutable
  runtime state are separate enums with one variant per asset type (Battery, Ev,
  Heater, Pv, BaseLoad); `AssetConfig` dispatch methods (`state_values`,
  `control_schema`, `forecast`, `available_storage_kwh`, `build_milp_context`, …) are
  the single switchboard. Adding an asset type = one new variant + one module.
- **Per-asset history**: every `AssetEntry` in `SimState` carries a ring buffer of
  3600 `HistoryPoint`s (≈ 1 h at 1 s tick) with LOCF lookups and time-weighted
  averaging (`assets/history.rs`, split out of `assets/mod.rs` by the same R-08
  refactor) — this feeds `/timeline`, `/history/:id`, and obligation reports
  ([[openadr-interface]]).

## Dispatch macro refactor (R-08)

`AssetConfig`'s 14 uniformly-shaped dispatch methods (`step`, `capability`, `forecast`,
`state_values`, …) were hand-written 5-arm matches; two `macro_rules!` forwarders
(`delegate_asset!`, `delegate_asset_state!` in `assets/mod.rs`) now declare the
Battery|Ev|Heater|Pv|BaseLoad variant list once and generate them. The ~6
asset-specific methods that only apply to a subset of variants (`plan_trajectory`,
`build_milp_context`, …) are untouched — they aren't part of the `Asset` trait and have
no uniform signature to generalize. The macro alone didn't clear `assets/mod.rs`'s
500-line production cap, so the `Asset` trait/`AssetHandle` and the history ring buffer
moved to `assets/asset_trait.rs` and `assets/history.rs` respectively; `assets/mod.rs` is
no longer on `scripts/audit_file_sizes.py`'s allowlist.

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

## PV curtailment

`PvInverter` (`assets/pv.rs`) distinguishes `rated_kw` (DC nameplate, forecast ceiling) from
`inverter_max_kw` (AC ceiling — DC potential clamps to it everywhere before any commanded
limit). The live-simulator generation limit lives on per-tick `PvState.generation_limit_kw` /
`curtailment_source` (`none`/`plan`/`capacity`/`manual`, persisted via `tick_samples` schema v6 —
`RENAME COLUMN` from the pre-rename `export_limit_kw`) rather than on `PvInverter` itself, so a
historical reconstruction reports the limit that was actually active at that tick, not the
current one. `dispatcher::resolve_pv_generation_limit_kw` computes the live limit as the most
restrictive of four candidates — VTN/sim-inject capacity cap, the [[milp-planner]]'s own
`p_pv_used[t]` curtailment target, and a new **manual operator override**
(`pv_generation_limit_kw` sim-inject field, `PvCurtailmentSource::Manual`, wins exact ties as
the most deliberate/explicit source) — this is what makes VTN `EXPORT_CAPACITY_LIMIT` events
physically take effect, not just appear in the plan.

The field was renamed application-wide (`export_limit_kw` → `generation_limit_kw`) because
"export" is reserved in this project's vocabulary for net site-to-grid flow — a system-level
quantity the PV inverter has no visibility into; what the inverter actually enforces is a cap
on its own output. `OadrCapacityState.export_limit_kw` and `SimInjectState.grid_export_limit_kw`
are genuinely site-level and were left untouched. The manual override re-adds a capability from
a deleted branch that predated MILP-level PV curtailment, now built on top of it instead of
replacing it — surfaced automatically through the existing schema-driven `ControlDescriptor`
mechanism, no new frontend component needed (see [[ven-ui]]'s nullable-slider note for the UI
side).

`assets.pv.rated_kw` had drifted from the weather feed's calibrated `weather_pv.rated_kwp` since
[[weather-forecast]] was wired up (a fix existed on an unmerged branch, so every worktree cut
from `main` kept regenerating the stale values — a branch-divergence bug, not a runtime revert).
Corrected per-VEN (ven-1 8.0→14.4 kW, ven-3 6.0→8.0 kW; ven-2 was already correct), and
`inverter_max_kw` — previously left defaulted to `rated_kw` "so existing profiles are
unaffected" — is now set explicitly per VEN (12.5/10.0/7.5 kW, each below its corrected
`rated_kw`). This surfaced a latent bug in `PvInverter::control_schema()`: the manual
`pv_generation_limit_kw` slider capped at `rated_kw` instead of `inverter_max_kw`, invisible
until the two values diverged — fixed to match what `step_inner` clamps against everywhere.

## Real-measurement ground truth (ven-1 only)

`PvInverter.measured_power_kw` / `BaseLoad.measured_load_kw` (both `Option<f64>`, set each
tick, not from YAML) let a real MQTT reading outrank the simulated/forecast value —
[[real-measurement-mqtt]]. PV: 3-tier `measured > weather-derived > sin-model`, folded into
the same `base_kw.or(...)` selection `step_inner` already used for weather. BaseLoad: simple
replace — `measured_load_kw.unwrap_or_else(natural_profile_plus_noise)`. Both still let the
manual-override offset blend additively on top, unchanged.

## Heater safety envelope

Beyond `temp_min_c`/`temp_max_c` (the **comfort band** a user configures and the planner respects
under ordinary objectives) sits a wider **physical safety envelope**: no floor on the low side
(ambient is harmless, the tank just drifts), but a real hard ceiling `temp_safety_max_c` above
`temp_max_c` (e.g. `ven-2.yaml`'s 40–80 °C comfort / 90 °C true ceiling). `HeaterEmergencyMode`
(`entities/sim_inject.rs`: `Normal`/`Curtail`/`Absorb`) reaches into that envelope — `Curtail`
suppresses the forced-on floor, letting the tank drift toward ambient; `Absorb` suppresses the
forced-off ceiling, allowing heating up to `temp_safety_max_c`. Each direction leaves the other
bound untouched. Settable today only via `SimInjectState` (manual/test/demo) — no VTN emergency
directive drives it yet, and the MILP itself still plans only within the comfort band. This is
the lever [[deviation-arbiter]]'s heater-emergency lever drives.

## Determinism: injectable clock and seedable RNG (R-24)

`AssetConfig::forecast()` (all 5 variants), `Asset::history()`/`simulate_free()`/
`capability_trajectory()`, `parse_capacity_state()`, and `SimState::from_params()` now take
an explicit `now: DateTime<Utc>` instead of calling `Utc::now()` internally, and `SimState`
carries a seedable `StdRng` so `power_model::random_voltage()` no longer draws from unseeded
`thread_rng()` — closing the last live gaps against this project's determinism rule
(`.claude/CLAUDE.md` §determinism: no code path depending on wall-clock/randomness without an
injectable seam). One `Utc::now()` call site was found and classified as dead rather than
fixed: `entities/site_meter.rs::SiteMeter` was never constructed anywhere. Rather than wire
it up, R-62 deleted the file outright — its only genuinely-referenced type, `PowerSnapshot`
(used by `OadrEventCache::dispatch_setpoints`, itself an unwired sketch — see
[[openadr-interface]]), moved to `entities/capacity.rs`, the module of its one real consumer.

## Planning-side counterpart

For the [[milp-planner]], each controllable asset provides an `AssetMilpContext` —
its constraints and variables in solver terms. The trait is declared at the planner
boundary (`controller/milp_planner/asset_port.rs`) and implemented in `assets/battery.rs`,
`assets/ev.rs`, `assets/heater.rs` (cross-file inherent impls), so the solver only ever
sees trait objects ([[ven-hexagonal-architecture]]). PV and base load are not
MILP-controllable; their forecasts enter as per-slot input arrays.

Sign convention for all power values crossing this interface: positive = import,
negative = export/generation — see [[sign-convention]].
