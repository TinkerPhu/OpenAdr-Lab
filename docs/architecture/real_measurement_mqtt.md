# Real-Measurement MQTT Feeds — Architecture

VEN can ingest real, live meter/inverter readings over MQTT for two signals —
PV power and baseline (house) load power — and use them as ground truth in
place of (or blended with) the simulated/forecast values, per VEN instance.
This is the integration point for connecting VEN to an actual site: without
it, every VEN instance is purely simulated. This document is the reference
for that feature: the transport, the two-gate enablement design, each
signal's precedence rule, the MQTT wire contract a publisher must implement,
and what a downstream deployer needs to customize to connect their own
hardware.

Implemented in `VEN/src/measurement.rs` (the generic MQTT adapter),
`VEN/src/measurement_translation.rs` (the per-device customization point),
`controller/measurement_port.rs` (`MeasurementPort` trait),
`entities/measurement.rs` (`resolve_measured_kw`, the staleness gate),
`profile/schema.rs` (`measurements:` YAML section), `assets/pv.rs` /
`assets/base_load.rs` (the per-signal precedence logic), `simulator/mod.rs`
/ `tasks/sim_tick/context.rs` (wiring into the live tick), `routes/measurement.rs`
(`GET /measurement`), and the VEN UI Diagnostics → Measurements page
(`VEN/ui/src/pages/Measurement.tsx`).

Related: [`docs/architecture/weather_forecast.md`](weather_forecast.md) — the
weather-forecast MQTT feed this feature's transport layer deliberately
mirrors (same adapter shape, same port-seam pattern), but is otherwise
independent: weather is a forecast series feeding the planner's forward
horizon, while a measurement is a single live reading for *right now*,
feeding only the current tick.

## Scope: PV and baseline load only

Two signals are supported. EV power and grid power were considered and
explicitly dropped:

- **EV power**: VEN doesn't yet control a real EV charger, so displaying a
  "measured" ground truth against a planner setpoint that has no real
  physical effect would be misleading rather than informative.
- **Grid power**: its main value was cross-validating against a
  controllable EV, which no longer applies once EV was dropped.

The user's own EV consumption is expected to be subtracted from the
baseline-load reading *externally* (outside VEN, before the reading reaches
the MQTT topic) — VEN's baseline-load signal is meant to be "everything
except EV," with heater/boiler load accepted as still lumped into it (a
deliberate simplification, not a bug).

## Architecture: generic transport, signal-specific physics

Two independent MQTT connections (own broker/host each, since a real
deployment's PV inverter and house-load meter are commonly separate
physical devices), sharing one generic adapter implementation — the same
"generic at the transport layer, specific at the physics layer" split this
project already uses for the weather feed:

```rust
// controller/measurement_port.rs
pub type MeasurementReading = (f64, DateTime<Utc>); // (value_kw, reading's own timestamp)

#[async_trait]
pub trait MeasurementPort: Send + Sync {
    async fn latest_kw(&self) -> Option<MeasurementReading>;
    fn is_alive(&self) -> bool;
}
```

`MqttMeasurementAdapter::spawn(config, translate)` (`measurement.rs`) is
generic over both the MQTT topic and a `translate: fn(&[u8]) -> Result<MeasurementReading, String>`
function pointer — it owns the `rumqttc` subscription loop, resubscribe-on-
`ConnAck`, and 5s backoff-on-error, but has zero knowledge of any device's
wire format. Two independent instances are constructed in `main.rs`, one per
signal, each behind its own env-var gate.

### The one file a downstream deployer needs to edit

`measurement_translation.rs` is deliberately isolated from the transport —
it's the single place that knows how to turn a raw device payload into
`(value_kw, reading_at)`:

```rust
pub fn parse_pv_measurement(payload: &[u8]) -> Result<MeasurementReading, String>;
pub fn parse_base_load_measurement(payload: &[u8]) -> Result<MeasurementReading, String>;
```

The shipped default expects `{"power_kw": f64, "ts": rfc3339}`. A deployer
connecting their own inverter/meter (or a bridge translating a
manufacturer's own protocol into MQTT) only needs to rewrite the body of
these two functions — everything else in this feature (transport, gating,
precedence, UI) is device-agnostic and untouched.

## Two-gate enablement (per VEN, no `VEN_NAME` branching)

A signal takes effect only when **both** gates allow it — mirroring the
weather feed's design, extended with the profile-level gate weather didn't
originally have:

1. **Transport gate** (env var): `{SIGNAL}_MEASUREMENT_MQTT_HOST` set →
   adapter constructed; unset → `NoopMeasurementPort` (`latest_kw()` always
   `None`, `is_alive()` always `false`). No code anywhere branches on
   `VEN_NAME` — presence is purely config-driven.
2. **Profile gate** (`measurements:` YAML section):
   ```yaml
   measurements:
     pv_enabled: true
     base_load_enabled: true
   ```
   `Profile::pv_measurement_enabled()` / `Profile::base_load_measurement_enabled()`
   default to `false` when the section (or field) is absent. This lets a VEN
   instance have the MQTT connection configured (e.g. for diagnostics
   visibility) without yet trusting its readings for physics.

Env vars (mirroring `WEATHER_MQTT_*`): `{PV,BASE_LOAD}_MEASUREMENT_MQTT_HOST`
(required, gates presence), `_PORT` (default 1883), `_ROOT` (default
`openadr-lab`), `_SITE_ID` (default `default`). Topic:
`<root>/measurement/<site_id>/<pv|base_load>`.

## Per-signal precedence

Both signals resolve their measured reading once per tick, before the sync
lock (`tasks::sim_tick::context::resolve_tick_context`), via
`entities::measurement::resolve_measured_kw` — `None` unless a reading has
actually been received **and** is still fresher than
`MEASUREMENT_STALENESS_THRESHOLD` (5 minutes — much tighter than weather's
2 hours, since this is a live meter, not an hourly forecast). A stale or
never-received reading falls back to whatever the signal's next precedence
tier would otherwise use.

### PV: 3-tier (measured > weather > sin-model)

```rust
// assets/pv.rs — PvInverter::step_inner
let base_kw = self.measured_power_kw.or(self.weather_power_kw);
let dc_potential_kw = if self.irradiance_forced {
    self.rated_kw * self.irradiance          // manual override, exclusive
} else {
    match base_kw {
        Some(kw) => (kw.max(0.0) + self.irradiance_offset * self.rated_kw).max(0.0),
        None => self.rated_kw * self.irradiance,   // sin model
    }
};
```

A measured reading outranks the weather-derived estimate, which in turn
outranks the sin model — real ground truth beats a forecast, which beats an
unconditioned physics model. The manual-override blend mechanism (a
decaying `irradiance_offset` riding additively on top of whichever base
wins, only fully suppressed by an actively-forced override) is unchanged
from the weather feed and applies identically regardless of which base tier
is active.

### Baseline load: 3-tier (measured > learned heuristic > synthetic)

```rust
// simulator/mod.rs — SimState::tick's BaseLoad arm
bl.measured_load_kw = base_load_measured_kw;
let natural_base_kw = bl
    .measured_load_kw
    .or(base_load_heuristic_kw)
    .unwrap_or_else(|| bl.baseline_kw_profile + bl.appliance_noise_kw(now));
```

(BL-40) A measured reading outranks the site's own learned base-load
heuristic (`AssetHeuristics::sample_kw`, resolved once per tick in
`tasks/sim_tick/context.rs`'s `resolve_tick_context` and threaded into both
`SimState::tick` and `peek_base_load_kw` as `base_load_heuristic_kw`, so the
two never diverge for the same tick), which in turn outranks the synthetic
profile+noise model — the same measured > modeled-from-real-history >
invented-model precedence PV already uses. The heuristic tier is only
available once `learn_asset_heuristics` has cleared its cold-start gate at
least once for `ids::ASSET_BASE_LOAD`; before that, a dropout still falls
all the way to the synthetic model, matching prior behavior exactly. The
existing manual-override blend (`base_load_kw_override` /
`BaseLoadSmoothingState`) still rides on top of whichever base is
authoritative, unchanged.

**Planner scope**: neither signal feeds the planner's forward horizon
*directly* — a measurement is live ground truth for *now*, not a forecast
series with future slots. Both are only ever passed into the live tick
(`SimState::tick`/`peek_pv_kw`/`peek_base_load_kw`), never into
`services::planning`. There is, however, an *indirect* path for baseline
load — see below.

## Indirect path into the forecast: learned heuristics

A measured baseline-load reading, once substituted into
`SimState::tick`'s `BaseLoad` arm above, becomes that tick's
`entry.last_power_kw` — the asset's single "actual power" value, same as any
other asset. This value is what `tasks/history_sampler` downsamples into the
`tick_samples` SQLite history table every minute, unconditionally, whether
the tick's origin was measured or synthetic. It doesn't know or care which.

Separately (and pre-existing — this is `services/heuristics.rs`'s WP5.2/BL-14
`learn_asset_heuristics`, not part of this feature), a daily job
(`tasks/heuristics_job::spawn_heuristics_job`) fits an EWMA-recency-weighted
hour-of-day profile to `tick_samples` history and stores the result as
`AssetHeuristics`, which `controller/milp_planner/inputs.rs` samples to
build the planner's base-load forecast in place of the flat
`baseline_kw_profile` fallback.

The two features were built independently, but the effect is real: once
`base_load_enabled` measurement is live on a VEN instance, the planner's
base-load *forecast* — not just the live "now" value — converges toward real
measured behavior automatically, with no additional code. Convergence
timeline, from `HeuristicsConfig::default()`
(`ewma_halflife_days: 14.0`, `rolling_window_days: 42`), assuming
continuous, gap-free measurement uptime from the day the feed goes live:

- After **one EWMA half-life (~14 days)**: the most recent two weeks of real
  data already outweighs all older (pre-measurement, synthetic) history
  combined.
- After **the full 42-day rolling window**: zero pre-measurement samples
  remain in the learning window at all — the forecast is built entirely
  from real measurements.
- The forecast only updates once per UTC calendar day (the job's own
  cadence), not continuously.

**Caveat — no measured/synthetic provenance tag.** If the MQTT feed drops
out for longer than `MEASUREMENT_STALENESS_THRESHOLD` (5 min), the live tick
now falls back to the site's own learned base-load heuristic first (BL-40's
3-tier chain above) rather than straight to the synthetic model — so a
dropout re-mixes *real, previously-learned* behavior back into
`tick_samples` instead of an invented curve, once a heuristic has been
learned at least once. That fallback value is still recorded into
`tick_samples` indistinguishable from a directly-measured reading — there is
currently no column marking a sample's origin, and this change does not add
one. So "fully converged" above assumes uninterrupted feed uptime; an outage
before any heuristic has ever been learned (cold start) still falls all the
way to the synthetic model, and even once the heuristic tier is active, a
sustained outage means the learned profile is fitting past-heuristic-derived
samples rather than direct measurements — a strictly smaller drift than
before, but not eliminated, with no record of which samples came from which
tier.

## Wire contract

Same transport conventions as the weather feed
([`weather_forecast.md`](weather_forecast.md)#wire-contract): UTF-8 JSON,
RFC 3339 UTC timestamps, retained + QoS 1, forward-compatible (unknown keys
ignored). One topic per signal, no separate status/heartbeat topic — a
measurement's own freshness is judged purely by how recently a reading
arrived (`is_alive()`: last message within 2× the expected ~60s publish
interval), not a distinct "content vs. transport" split like weather's
forecast/status pair.

**Topic**: `<root>/measurement/<site_id>/<pv|base_load>`

**Default schema** (shipped `measurement_translation.rs` default — see
above for how to change it):

```json
{ "power_kw": 3.2, "ts": "2026-08-03T12:00:00Z" }
```

`power_kw` is validated finite and within `±1,000,000` before being
accepted; malformed or out-of-range messages are rejected and logged, the
previous cached reading (if any) stays in effect until it goes stale.

## VEN UI surface

`GET /measurement` returns both signals' `{status, is_fresh, source_alive,
raw_kw, raw_at}` (`status` ∈ `ok | stale | disabled | not_configured`) — the
`ui-transparency` requirement for this feature. The VEN UI's Diagnostics →
Measurements page (`VEN/ui/src/pages/Measurement.tsx`) surfaces both
signals' current reading, freshness, and source-alive state.

## Deployment note

This feature is intended for a single instance connected to a real site
(ven-1 in this project's own fleet) — the other fleet VEN instances
(ven-2/3, ven-5..13) simply never set the env vars, so they stay purely
simulated with zero code-path difference.
