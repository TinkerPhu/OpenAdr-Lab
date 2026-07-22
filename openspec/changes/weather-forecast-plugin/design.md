## Context

VEN's PV forecast today comes from two internal sources only —
`ForecastSource::Optimization` (the planner's own solved plan) and
`ForecastSource::Heuristic` (learned historical curves) — with no external
weather data anywhere in the pipeline. The full technical design (transport
architecture, data structures, transposition physics, sky-condition and
snow-cover models, and the exact MQTT wire contract) is written up in
`docs/architecture/weather_forecast.md`, which reflects the shipped
implementation. This design.md summarizes the key architectural decisions
for the OpenSpec record; it does not restate every formula — see that doc
for the full derivations (solar position, clear-sky-index transposition,
NOCT cell-temperature model, snow self-clearing state machine).

Existing patterns this change follows: the port-per-external-system shape
already used for `VtnPort`, `SolverPort`, `SimulatorPort`
(`VEN/src/controller/*_port.rs` — trait in `controller/`, concrete adapter
in infra, wired once at the composition root in `main.rs`); the injectable-
clock convention for anything date/time-dependent; the hexagonal
dependency rule (inner rings — `entities/`, `controller/` — never import
outer rings); and the 500-production-line file-size cap enforced by
`scripts/audit_file_sizes.py`.

## Goals / Non-Goals

**Goals:**
- Let VEN consume a real weather forecast (irradiance, temperature, plus
  richer optional signals) from any supplier without VEN code depending on
  that supplier directly.
- Turn a horizontal irradiance forecast into a physically-grounded
  predicted AC power for a fixed-tilt PV array, replacing the current
  sin-model assumption when a live feed is available.
- Model the two conditions that make an hourly-average irradiance number
  misleading on its own: intra-hour sky variability ("partly cloudy" ⇒
  fluctuation risk) and snow cover (⇒ near-total blackout regardless of
  irradiance).
- Define an unambiguous wire contract so a plugin author who has never
  seen VEN's source, and a VEN maintainer who has never seen a given
  plugin, can both implement against one document and interoperate.

**Non-Goals:**
- Multi-source fusion (more than one configured feed for the same site).
- Broker security hardening (TLS/auth) beyond what's needed to talk to the
  existing Mosquitto deployment.
- Telemetry-based cross-check for the snow model's initial state (ships
  with the forecast-only fallback only).
- Horizon/shading modeling, Perez/HDKR diffuse-sky model, module
  degradation — tracked as technical debt, not built.
- A second (e.g. REST-polling) adapter behind the same port — the port is
  designed to support one later, but this change implements MQTT only.

## Decisions

- **MQTT pub/sub over dlopen/subprocess-RPC/gRPC/WASM plugin models**:
  weather data is push-friendly (irradiance/temperature change on an
  hourly timescale, not something worth tight polling), suppliers are
  numerous and sometimes purely local (an on-site MQTT weather station),
  and the project's existing Mosquitto broker already provides retained-
  message staleness recovery for free. A plugin is then nothing more than
  "any process that publishes valid JSON to an agreed topic" — no process
  supervision, no ABI concerns, no VEN-side code change per new supplier.
  Alternatives considered and rejected: dynamically loaded native
  libraries (no stable Rust ABI, crash risk in-process); subprocess +
  JSON-RPC or gRPC (VEN would own process lifecycle — unnecessary
  complexity for a value fetched hourly); WASM sandboxing (solves an
  untrusted-third-party-code problem this project doesn't have).
- **Two MQTT topics, not one** (`.../forecast` data topic +
  `.../status` heartbeat topic): an hourly-cadence data topic alone cannot
  distinguish "no new forecast due yet" from "the plugin crashed an hour
  ago." The status topic is also registered as the plugin's MQTT Last Will
  and Testament, so an ungraceful disconnect is caught immediately rather
  than after a heartbeat timeout.
- **A single `WeatherForecastPort::latest() -> Option<WeatherForecast>`
  trait**, backed by a `tokio::sync::watch` channel that a background MQTT
  task writes into: keeps `services/` and `controller/` (and the planner)
  completely unaware of MQTT, JSON parsing, or any supplier detail. This
  is the same "in-process seam" shape as the existing ports.
- **Clear-sky-index transposition** (`clear_sky_index = ghi_forecast /
  ghi_clearsky_model`, then `poa_irradiance = clear_sky_index ×
  poa_clearsky_model`) over porting the existing flux dashboard's
  hand-tuned scale constant: the flux script's `* 64.0 * 0.16`-style
  constants conflate unit conversion and site-specific losses into one
  undocumented, unverifiable number. Running the same clear-sky physics
  model twice (once horizontal, once at the panel's own tilt/azimuth) and
  taking the ratio isolates the actual cloud-cover signal and applies it
  to the panel's real geometry — the standard approach used by commercial
  PV-forecast services.
- **`irradiance_variability` as a continuous 0–1 signal, not just a
  sky-condition enum**: an hourly average can't distinguish uniform thin
  overcast from a genuinely broken sky (sun/cloud alternating), even
  though both may average to the same kWh — but the two have very
  different PV ramp-rate risk. SRF's own `SUN_MIN` field (sunshine minutes
  within the hour) gives this directly and continuously; the
  `SkyCondition` enum (translated per-adapter from the supplier's own
  icon/description code) is a secondary, coarser signal. Feeds directly
  into the *existing* `AssetForecast.confidence` field — no new consumer
  needed.
- **Near-binary snow-cover state machine over a continuous melt-depth
  integrator**: real tilted, dark PV panels shed a snow layer as a sheet
  once melting starts (self-clearing), rather than melting slowly like
  ground snow — a two-state model (`covered: bool`, triggered by fresh
  snowfall, cleared once temperature crosses a near-0°C threshold) matches
  that behavior and is far simpler to reason about for hourly planning
  than a depth integrator would be.
- **Weather-sourced PV forecast is additive, not a replacement**: the
  existing sin-model (`PvInverter`/`PvParams::forecast_kw`) stays for
  simulator/demo use and as the fallback when no weather feed is
  configured or the cached forecast has gone stale. Two independent
  consumption points get the weather data: the planner's own MILP input
  (`PvInverter::build_milp_context`) and the API-visible forecast
  (`services::forecast`, tagged `ForecastSource::WeatherModel`) — these
  are not the same integration and must both be wired, per the
  implementation plan's Phase 7 distinction.
- **Staleness policy**: a cached `WeatherForecast` is rejected for
  planning purposes once `now - fetched_at` exceeds a configurable
  threshold (initial default: 2 hours, given the wire contract's "hourly,
  never faster than 5 min" cadence) — mirrors the shape of
  `StaleRatePolicy` (`controller/milp_planner/stale_rates.rs`); whether it
  can literally reuse that mechanism is a call made during implementation
  once this phase is reached, not fixed here.

## Risks / Trade-offs

- **[Risk]** A malformed or malicious MQTT publisher on the same topic
  could feed VEN bad data (wrong units, out-of-range values, wrong sign).
  → **Mitigation**: strict JSON Schema validation at the adapter boundary
  (required fields, numeric `minimum`/`maximum` bounds per the wire
  contract) with reject-and-log (never panic) on any violation; the
  existing sin-model/no-data fallback means a rejected message degrades to
  "no weather forecast," not "wrong forecast."
- **[Risk]** The existing Mosquitto deployment allows anonymous
  connections on its plaintext listener — anyone on the local network can
  currently publish to any topic. → **Mitigation**: explicitly out of
  scope for this change (see Non-Goals); flagged as a prerequisite to
  revisit before any broker exposure beyond the local network.
- **[Risk]** The clear-sky-index transposition needs a clear-sky GHI model
  as an internal building block that doesn't exist yet in this codebase —
  a nontrivial piece of new physics code, not just wiring. → **Mitigation**:
  Phase 2 of the implementation plan budgets dedicated golden-value unit
  tests (known solar positions at a known site/time) specifically because
  this is the highest-consequence-if-wrong code in the whole change.
  Snow-cover self-clearing thresholds are empirical approximations (not
  measured against this specific installation) → **Mitigation**: exposed
  as configurable `PvSnowParams`, not hard-coded constants, so they can be
  tuned per-site without a code change.
- **[Trade-off]** No multi-source fusion means a site with both a local
  MQTT station and a desire for a cloud-API supplier can't have both
  active at once yet. Accepted for v1 — the port design doesn't preclude
  adding it later (see Non-Goals).

## Migration Plan

Purely additive — no existing behavior changes for a VEN instance that
never configures a weather MQTT broker (`WeatherForecastPort::latest()`
returns `None`, sin-model fallback applies everywhere, identical to
today's behavior). No data migration, no breaking API changes, no
rollback complexity beyond reverting the change. Suggested rollout order
follows the implementation plan's phases 0–9 (dependencies → domain types
→ physics → snow model → port/mock → MQTT adapter → composition-root
wiring → planner integration → BDD coverage → docs/debt bookkeeping),
each independently reviewable and mergeable behind the fact that nothing
downstream activates until Phase 6 wiring is in place.

## Open Questions

- Exact staleness threshold value (2 hours is a starting default, not a
  measured one) — tune once real data is flowing.
- Whether the snow-cover staleness/initial-state problem should eventually
  read live PV telemetry deviation (`AssetState.power_deviation_kw`) as a
  cross-check, and if so, where that wiring lives.
- Whether `StaleRatePolicy` can be literally reused for weather-forecast
  staleness or needs a parallel, weather-specific enum — deferred to
  Phase 7 implementation.
