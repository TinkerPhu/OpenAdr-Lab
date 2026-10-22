---
title: Asset Layer
type: component
created: 2026-07-04
updated: 2026-10-21
synced_commit: 650b626
sources: [VEN/src/assets/, VEN/src/simulator/mod.rs, VEN/src/entities/asset.rs, VEN/src/entities/asset_params.rs, VEN/src/entities/sim_inject.rs, VEN/src/controller/asset_milp_port.rs, docs/architecture/VEN_ARCHITECTURE.md, docs/architecture/ven_asset_interface_spec.md]
tags: [assets, abstraction, ven]
---

# Asset Layer

The device abstraction between the HEMS controller and the physics that produces the
numbers (`VEN/src/assets/`). Four cooperating pieces:

- **`Asset` trait** (`assets/asset_trait.rs`) — one trait, implemented by `Battery`, `EvCharger`,
  `Heater`, `PvInverter`, `BaseLoad` and `ShiftableLoadAsset`. The physics contract is
  `step(state, setpoint_kw, dt) -> (new_state, actual_kw)` and `capability(state)` (the feasible
  power range right now); `flexibility_floor` (the least the asset can still be forced to) has no
  default, so every kind states it. The rest is what an asset *declares about itself* so that no
  other module has to ask which kind it is: `kind() -> AssetKind`, `key_features`,
  `state_values`, `control_schema`, `forced_power_kw`, `surplus_claim_rank`, `arbiter_lever`,
  `request_kind`, `ac_ceiling_kw`, `storage_capacity_kwh`, `embodied_co2_g_kwh`,
  `default_comfort_rates`, `is_cancellable`, `is_removable`. `simulate_forward` and the
  max-effort pair (`max_effort_setpoint`, `max_effort_schedule`; `docs/architecture/VEN_ARCHITECTURE.md`
  §3.0a) are default-implemented on top of `step`/`capability`.
- **Optional capability traits** reached through `as_milp_participant()`, `as_request_resolvable()`,
  `as_thermostat()`, `as_tick_overridable()` (`assets/capability_traits.rs`): a kind implements only
  those it has real behaviour for. `MilpParticipant` builds the asset's planner context from one
  `MilpBuildInputs`.
- **`AssetState`** (state-only enum, one variant per kind plus `Grid`) paired with the physics struct;
  `assets/own_state.rs` declares once where each kind's state sits in the enum, and `own` is the one
  checked borrow (a mismatch is a bug in this binary and panics naming both kinds).
- **Per-asset history**: every `AssetEntry` in `SimState` carries a ring buffer of 3600
  `HistoryPoint`s (≈ 1 h at 1 s tick) with LOCF lookups and time-weighted averaging
  (`assets/history.rs`) — this feeds `/timeline`, `/history/:id` and obligation reports
  ([[openadr-interface]]).

`SimState` holds the roster as `asset_configs: Vec<Box<dyn Asset>>` beside `assets: Vec<AssetEntry>`
(state, last setpoint, energy counter, history); `AssetHandle` wraps one such pair to answer the
identity and history methods. There is no config enum: adding a kind is one module, one `AssetKind`
variant and one `AssetState` variant, and every exhaustive `match` learns of it.

## Kind is declared, never read from the id

`AssetKind` (`entities/asset.rs`: `Pv`, `Battery`, `Ev`, `Heater`, `BaseLoad`, `ShiftableLoad`) is the
one identifier of an asset's type, returned by `Asset::kind` and spelled `snake_case` on the wire and
in a profile's `type:`. What an asset can *do* (a state of charge, an arbiter lever, a request kind)
stays a question put to the asset, so a profile may hold several assets of one kind and none of the
controller, planner or UI looks one up by a fixed id. A shiftable load is the one dynamic member: it
is added when a request is accepted and removed when it finishes or is cancelled while pending
(`SimState::add_asset`/`remove_asset`, `Asset::is_removable`). See [[milp-planner]] for the planner
side and [[ven-ui]] for how the UI reads the kind.

## The site meter is not an asset

`assets/grid.rs` (`Grid`, held as `SimState.grid_asset`) tracks net site power plus the VTN capacity
limits each tick and keeps its own history. It implements no `Asset` and declares no `AssetKind`; the
simulator reads its state and history directly, and nothing dispatches it. It is the only virtual
quantity: the earlier `site-residual` pseudo-asset (`grid meter − Σ modelled asset power`) was removed
because `base_load` is itself derived as `grid_true − Σ(other asset measurements)`, which forces the
residual to zero by construction — the site's unmetered load is `base_load`'s job in full
([[heuristics-pipeline]], `docs/architecture/forecasting_model.md`).

**BaseLoad appliance noise**: a profile-configured `base_load.spikes` list adds trapezoidal daily
appliance pulses (plateau at `amplitude_kw`, linear ramps, timing/magnitude jitter, optional weekday
restriction, per-day firing probability; empty by default). Trapezoids, not Gaussians, because a
trapezoid's energy is directly `≈ amplitude_kw × (duration_h − ramp_h)` — settable to match a real
appliance session ([[heuristics-pipeline]]). `assets/load_window.rs` summarises the recorded power
over the trailing 14 days (mean, peak) for the Controller's flexibility panel: the samples come from
the history store, the interpretation is the asset's.

## PV

`PvInverter` (`assets/pv.rs`) distinguishes `rated_kw` (DC nameplate, forecast ceiling) from
`inverter_max_kw` (AC ceiling, `Asset::ac_ceiling_kw` — DC potential clamps to it before any commanded
limit; the manual-override slider caps at it too). Power resolves in one place
(`PvInverter::resolve_power_kw`, `assets/pv_schedule.rs`), shared by `step_inner` and the previewing
`SimState::peek_pv_kw`, with the precedence `measured > weather-derived > sine model`
(`PvPowerInputs`); a manual irradiance offset blends on top. A profile may hold several arrays: each
has its own weather forecast, measurement and feeds, and a site generation limit is shared among them
in proportion to their AC ceilings (`simulator::generation_limit_share`). Each array also concludes for
itself whether it is snow-covered, by comparing what it delivers against the snow-free weather forecast
(`observe_snow`, `PvSnowState`, R-55); a curtailed array is not mistaken for a covered one.

**Generation limit.** The live limit sits on per-tick `PvState.generation_limit_kw` /
`curtailment_source` (persisted in `tick_samples`), not on `PvInverter`, so history reports the limit
that was active at that tick. `dispatcher::resolve_pv_generation_limit_kw` takes the most restrictive of
the candidate limits and tags the winner with a `PvCurtailmentSource`: `Plan` (the planner's own
curtailment, the PV's used-generation variable), `Capacity` (a VTN/sim-injected cap — this is what makes
a VTN `EXPORT_CAPACITY_LIMIT` physically take effect, not just appear in the plan), `Arbiter` (the
[[deviation-arbiter]]'s last-resort lever), `Manual` (`pv_generation_limit_kw` sim-inject) and
`CommsLoss` (the R-59 fail-safe, a debounced percentage of `inverter_max_kw`). An exact tie goes to the
later-listed source, so a deliberate manual cap beats the automatic ones and `CommsLoss` beats all
(a comms-loss safety fallback must not lose to a stale manual override). The field is named for what the
inverter enforces — a cap on its own output; "export" is reserved for net site-to-grid flow
(`OadrCapacityState.export_limit_kw`, `SimInjectState.grid_export_limit_kw` are site-level). The manual
slider reaches the UI through the schema-driven `ControlDescriptor` (`nullable`, see [[ven-ui]]).

## Real-measurement ground truth

`PvInverter.measured_power_kw` / `BaseLoad.measured_load_kw` (both `Option<f64>`, set each tick, not from
YAML) let a real MQTT reading outrank the simulated or forecast value — [[real-measurement-mqtt]].
PV: `measured > weather-derived > sine model`. BaseLoad: simple replace —
`measured_load_kw.unwrap_or_else(natural_profile_plus_noise)`. Both still let the manual-override
offset blend additively on top.

## Heater

Beyond `temp_min_c`/`temp_max_c` (the **comfort band** a user configures and the planner respects) sits a
wider **physical safety envelope**: no floor on the low side (ambient is harmless), but a hard ceiling
`temp_safety_max_c` above `temp_max_c` (e.g. `ven-2.yaml`'s 40–80 °C comfort / 90 °C ceiling).
`HeaterEmergencyMode` (`assets/heater_emergency.rs`: `Normal`/`Curtail`/`Absorb`) reaches into it:
`Curtail` suppresses the forced-on floor, `Absorb` suppresses the forced-off ceiling up to
`temp_safety_max_c`; each leaves the other bound untouched. It is settable through `SimInjectState`
(manual, test, demo) and by the deviation arbiter, which chooses it per heater
(`controller/arbiter/emergency.rs`); no VTN directive drives it, and the MILP plans only within the
comfort band.

The thermostat rule lives once, in `assets/heater_thermostat.rs`: safety ceiling first (reachable only
in `Absorb`), then the emergency heat at `temp_min_c` (comfort beats relay protection), then a ceiling
deadband. `thermostat_delta_c` (default 3 °C) applies at both ends — the emergency runs until
`temp_min_c + delta`, and after reaching `temp_max_c` the relay stays open until `temp_max_c − delta`,
tracked by `HeaterState.ceiling_latched` beside `emergency_latched`. The planner never contradicts the
latch: `Heater::forced_kw_ahead` replays the same rule slot by slot and
`assets/heater_pins.rs` fixes those slots' stages before the solve ([[milp-planner]]). The same rule
feeds `capability`, so the capacity projection sees a stable relay rather than a sawtooth.

## EV

`EvCharger` answers its own availability question once: `assets/ev_schedule.rs` makes the departure
time and a profile-configured daily trip (`daily_trip`, generated deterministically per calendar day so
the live tick and a multi-day forecast agree) one projection that `simulate_forward` honours, and
`assets/ev_usage_forecast.rs` is where the EV's own prediction of "what will this car do next" reaches the
planner. Static nameplate facts for display come from `key_features`; comfort bids from
`default_comfort_rates` ([[hems-planning]]).

## Determinism: injectable clock and seedable RNG (R-24)

Asset methods that depend on time take an explicit `now: DateTime<Utc>` instead of calling `Utc::now()`
(`Asset::history`, `SimState::from_params`), and
`SimState` carries a seedable `StdRng` so `power_model::random_voltage()` is reproducible — the
`.claude/CLAUDE.md` §determinism rule. The architecture audit now refuses `Utc::now()` in `assets/` and
`simulator/` outright ([[ven-hexagonal-architecture]]). The dead `site_meter.rs` was deleted (R-62);
its one live type, `PowerSnapshot`, moved to `entities/capacity.rs`.

## Planning-side counterpart

For the [[milp-planner]], each controllable asset provides an `AssetMilpContext` — its variables,
constraints, cost and friction, and its own plan result, in solver terms. The trait is declared in the
domain ring (`controller/asset_milp_port.rs`) and implemented per kind in `assets/<kind>_declared.rs`
(battery, EV, heater, shiftable load), with the model maths in `assets/<kind>_milp.rs`; each context is
built from one `MilpBuildInputs` and carries its roster id, so the solver only ever sees trait objects.
PV and base load take part as forecast participants (`assets/forecast_declared.rs`): never dispatched,
their forecasts enter as fixed net power (curtailable for PV).

**Asset competence.** Interpretation of an asset's current state or forecast has exactly one authority:
the asset (`.claude/CLAUDE.md` §asset-competence-assurance; `docs/architecture/VEN_ARCHITECTURE.md`
§3.0d). Controllers see only the flattened `AssetSnapshot` — capability, available energy, forced
power, response, rank, lever kind — never raw values they would interpret themselves.

Sign convention for all power values crossing this interface: positive = import,
negative = export/generation — see [[sign-convention]].

> **OPEN QUESTION** `docs/architecture/VEN_ARCHITECTURE.md` §3.0 still lists a future `MeasuredAsset`
> (a real-hardware `impl Asset` behind the same `Box<dyn Asset>`); no such type exists. Real readings
> today enter only as `measured_*_kw` fields on the simulated PV and base load. See
> [[ven-code-vs-docs-audit]].
