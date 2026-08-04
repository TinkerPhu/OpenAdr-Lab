---
title: Real-Measurement MQTT Feeds
type: component
created: 2026-08-03
updated: 2026-08-04
synced_commit: 093fbd1
sources: [docs/architecture/real_measurement_mqtt.md, VEN/src/measurement.rs, VEN/src/measurement_translation.rs, VEN/src/controller/measurement_port.rs, VEN/src/entities/measurement.rs, VEN/src/assets/pv.rs, VEN/src/assets/base_load.rs, VEN/src/simulator/mod.rs, VEN/src/tasks/sim_tick/context.rs, VEN/src/routes/measurement.rs, VEN/ui/src/pages/Measurement.tsx, tests/features/real_measurement_mqtt.feature, VEN/docker-compose.yml, VEN/profiles/ven-1.yaml]
tags: [measurement, pv, baseload, mqtt, ven, real-hardware]
---

# Real-Measurement MQTT Feeds

VEN can ingest real, live meter/inverter readings over MQTT for two signals — PV power and
baseline (house) load power — and use them as live ground truth, per VEN instance, in place
of the simulated/forecast values. This is the connection point between the lab's simulation
and an actual site: without it, every VEN instance is purely simulated. Full architecture
(two-gate enablement, per-signal precedence, wire contract): `docs/architecture/real_measurement_mqtt.md`.
This page covers what that doc doesn't: how it interacts with the already-existing
[[weather-forecast]] plugin and [[simulator]], and what broke on first real E2E verification.

## Deliberately narrower scope than a full "real hardware" story

Only PV and baseline load — EV power and grid power were considered and dropped. EV: VEN
doesn't yet control a real EV charger, so a "measured" reading next to a planner setpoint
with no real physical effect would be misleading, not informative. Grid: its main value was
cross-validating against a controllable EV, which no longer applies once EV was dropped.
The user's own EV consumption is expected to be subtracted from the baseline-load reading
*externally*, before it reaches VEN's MQTT topic — VEN's baseline-load signal is "everything
except EV," with heater/boiler load accepted as still lumped in (a deliberate
simplification).

## Same port-seam pattern as weather, deliberately simpler transport

`MeasurementPort` (`controller/measurement_port.rs`) mirrors `WeatherForecastPort`'s seam —
see [[ven-hexagonal-architecture]]'s port table — but the concrete adapter
(`MqttMeasurementAdapter`, `measurement.rs`) is generic over *both* the topic and a
`translate: fn(&[u8]) -> Result<MeasurementReading, String>` function pointer, so one adapter
implementation serves both signals; only `measurement_translation.rs` (two functions,
`parse_pv_measurement`/`parse_base_load_measurement`) knows the wire format. That file is
the explicit, sole customization point for a downstream deployer connecting their own
inverter/meter — a design constraint the user stated up front, mirroring how [[weather-forecast]]
already isolates supplier-specific translation (SRF Meteo's sky-icon lookup table) from the
generic MQTT plumbing.

Unlike weather, there's no separate status/heartbeat topic: a measurement's own freshness is
judged purely by how recently a reading arrived (5 min staleness, vs. weather's 2 h — a live
meter is expected to publish roughly every ~60 s, not hourly).

## Two-gate enablement, extending a gap weather had

Weather is gated by env var (`WEATHER_MQTT_HOST` presence) alone at the transport layer, plus
a *separate*, uncoordinated `weather_pv` profile section that happens to also gate the
planner-input math. This feature's `measurements:` profile section
(`pv_enabled`/`base_load_enabled`) was designed explicitly as a second, deliberate gate
alongside the env var — both must allow a signal for it to take effect — after the user
pointed out mid-design that env-var-only gating doesn't match how `weather_pv:` already
works. No `VEN_NAME` branching anywhere; presence is purely config-driven, same principle as
weather.

## Precedence reuses (and depended on) the weather-blend fix

PV: `measured > weather-derived > sin-model` (3-tier); baseline load: `measured` replaces
the synthetic profile+noise outright (no intermediate tier). Both feed into
`PvInverter`/`BaseLoad`'s existing manual-override-offset blend mechanism
(`assets/pv.rs::step_inner`, `simulator/mod.rs`'s `BaseLoad` tick arm) — the same additive
blend, not a binary null/keep switch, that a same-day fix
([[weather-forecast]]'s "found 2026-08-03" note doesn't cover this one; see that component's
git history around commit `c93556e`) made correct for weather immediately before this
feature was built on top of it. Building the 3-tier precedence directly on that already-fixed
mechanism, rather than before it, avoided reintroducing the same class of bug (a lingering
offset silently suppressing the new measured tier).

Neither signal feeds the planner's forward horizon *directly* — a measurement is live ground
truth for *now*, not a forecast series with future slots. Both are only ever resolved into
the live tick (`tasks::sim_tick::context::resolve_tick_context`, called once per tick before
the sync lock, mirroring how weather's own tick-time resolution works), never into
`services::planning`.

## Indirect path into the forecast: composes with [[heuristics-pipeline]] (found 2026-08-04)

Two features built independently turned out to compose. A measured baseline-load reading
substituted into `SimState::tick`'s `BaseLoad` arm becomes that tick's `entry.last_power_kw`
— the asset's one "actual power" value, same as any other asset — which `tasks/history_sampler`
downsamples into `tick_samples` every minute unconditionally, with no awareness of whether the
value's origin was a real MQTT reading or the synthetic fallback. [[heuristics-pipeline]]'s
daily `learn_asset_heuristics` job trains the planner's `base_load` forecast from exactly that
history. So once `base_load_enabled` measurement goes live on a VEN instance, the planner's
base-load *forecast*, not just its live "now" value, converges toward real measured behavior
automatically — verified on ven-1 (2026-08-04): `tick_samples` rows for `base_load` matched
the live MQTT feed within the same minute of enabling it, and `GET /forecast`'s `base_load`
entry was already `"source":"HEURISTIC"` (heuristics job fires on its first check after a
restart, not just at UTC midnight — see [[heuristics-pipeline]]).

Convergence timeline from `HeuristicsConfig::default()` (`ewma_halflife_days: 14.0`,
`rolling_window_days: 42`), assuming continuous feed uptime: the most recent 14 days already
outweighs all older (pre-measurement) history after one half-life; the pre-measurement history
ages out of the window entirely after the full 42 days. No measured-vs-synthetic provenance
tag exists on `tick_samples` — a feed dropout during that window silently re-mixes synthetic
fallback samples into the learned profile with no record of which rows they were. Full writeup:
`docs/architecture/real_measurement_mqtt.md`'s "Indirect path into the forecast" section.

## First full E2E run found two real test-isolation bugs (2026-08-03)

Neither was a product bug — both were flaws in the new BDD scenarios' assumptions about a
shared, long-lived VEN-1 instance across dozens of unrelated scenarios in the same suite run:

1. **MQTT retain vs. "never published."** Mosquitto retains the last message per topic; a
   scenario asserting `/measurement` reports `not_configured` must run *before* any other
   scenario in the suite publishes to that exact topic, not after. Fixed by reordering it
   first in `real_measurement_mqtt.feature` (the topic is unique to this feature, so no
   cross-file ordering risk).
2. **Lingering manual-override offset bleeds in.** Earlier `phase_a_physics.feature`
   scenarios force `pv_irradiance` overrides; the resulting `irradiance_offset` decays very
   slowly in real time (the per-tick factor is computed against a 300 s plan-step, so at the
   default `pv_alpha=0.1` it barely moves within tens of real seconds) and — correctly, by
   the blend design above — additively rides on top of whatever base wins next, including a
   freshly-measured reading. `/sim/inject/reset` (the existing, already-used-elsewhere reset
   step) only stops *re-forcing* the override; it doesn't accelerate the existing offset's
   decay. Fixed with a new step that flushes the offset to exactly zero first
   (`pv_irradiance_alpha=1.0` forces full decay within one tick, then reset again) —
   `tests/features/steps/real_measurement_mqtt_steps.py`.

Both point at the same underlying fact worth remembering for any future scenario touching
PV physics: this suite's VEN-1 instance is *not* reset between ordinary (non-`@isolated`)
scenarios, and the manual-override blend mechanism is deliberately slow-decaying by design —
a scenario needs to either tolerate that or explicitly flush it, `/sim/inject/reset` alone is
not sufficient for exact-value assertions.

## UI surface

`GET /measurement` (`{pv, base_load}` each carrying `status ∈ {ok, stale, disabled,
not_configured}`, `is_fresh`, `source_alive`, `raw_kw`, `raw_at`) plus a VEN UI Diagnostics →
Measurements page (`VEN/ui/src/pages/Measurement.tsx`) — the `ui-transparency` rule
(`.claude/CLAUDE.md`) applied the same way [[weather-forecast]]'s Weather page and
[[deviation-arbiter]]'s diagnostics card were.

## Deployment

Intended for a single instance connected to a real site — ven-1 in this lab's own fleet. The
rest of the fleet (ven-2/3, ven-5..13) simply never sets the env vars, so they stay purely
simulated with zero code-path difference; this mirrors how [[fleet-tooling]]'s bulk
registration already treats most fleet VENs as interchangeable simulated instances.
