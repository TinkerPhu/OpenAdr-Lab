# Weather Forecast Plugin — Implementation Plan

Status: **planning only — no implementation started.** This is the phased
build plan for the design fully specified in
`docs/plans/weather-forecast-plugin.md` (architecture, data structures,
transposition physics, sky-condition/variability signal, snow-cover model,
and the MQTT wire contract). This plan sequences that design into shippable
steps; it does not repeat the reasoning behind any formula or field —
that document is the source of truth for *what*, this one is the order of
*how*.

Each phase below is meant to be independently reviewable and, per this
project's `test-first` workflow rule, built test-first: write the failing
test, confirm it fails, implement until green, at both the unit level (each
new function) and — for phases that touch a user-visible behavior — the BDD
level. Follow the existing VEN Rust test pyramid (Domain → Use-case →
Adapter-contract → Integration) for every phase that adds testable code.

## Phase 0 — Dependencies and scaffolding

- Add an MQTT client crate to `VEN/Cargo.toml`, pinned to a semver range
  per the `dependencies` rule (`rumqttc` is the natural choice — pure-Rust,
  async/tokio-native, no C bindings to cross-compile for the Pi4 target).
  Run `cargo audit` and a license check against the project's acceptable-
  license list before committing to it.
- Confirm `async-trait` (already a dependency, used by `VtnPort`) covers
  the new port trait without additions.
- No behavior yet — this phase only makes the crate compile with the new
  dependency present and unused, so the dependency addition itself can be
  reviewed in isolation from any logic.

## Phase 1 — Domain entities

Add the wire-adjacent-but-pure types from `weather-forecast-plugin.md`'s
"Rust data structure" section: `SkyCondition`, `WeatherForecastSample`,
`WeatherForecast`, `GeoPosition`.

- These currently have no home; `entities/design_vocabulary.rs` already
  holds the placeholder `ForecastSource::WeatherModel` and
  `ExternalDataSourceType::Weather`/`Irradiation` variants this work fills
  in, so start there for consistency with how `AssetHeuristics` graduated
  from sketch to shipped type in the same file. Split into a dedicated
  `entities/weather.rs` only if it would push `design_vocabulary.rs` past
  the file-size cap — check with `scripts/audit_file_sizes.py` once the
  types are in.
- No `use crate::profile` in these types (entities-layer rule) — every
  field is either supplier-reported data or geometry, no profile coupling.
- Unit tests: serde round-trip for each type (JSON in the wire contract's
  example messages should deserialize into these structs without loss —
  this is the cheapest possible regression guard that the Rust types and
  the documented wire schema haven't drifted apart), plus construction/
  equality tests for `SkyCondition`.

## Phase 2 — PV geometry and physics (pure domain functions)

Add `PvArrayGeometry`, `PvForecastParams` to `entities/asset_params.rs`
(alongside the existing `PvParams`, additive per the "parallel, additive
capability" note in the design doc — the sin-model path stays for
simulator/demo use).

Add the physics functions, likely their own module
(`entities/solar.rs` or similar — keep `PvParams`'s home file under its
size cap):

- `solar_position(pos, t) -> SolarPosition`
- `poa_irradiance_w_m2(ghi_w_m2, sun, panel) -> f64` (the clear-sky-index
  transposition method from the design doc — needs a clear-sky GHI model
  function as an internal building block, run once for horizontal and once
  for the panel plane)
- `cell_temperature_c(air_temp_c, poa_w_m2, noct_c) -> f64` (NOCT model)
- `forecast_ac_kw(params, sample, t) -> f64` (composes all of the above,
  in the order the design doc specifies: transposition → DC power →
  cell-temp derate → performance ratio → AC clamp; snow override lands in
  Phase 3, applied last)

This is the highest-value test target in the whole plan: every function
here is pure, deterministic, and has no I/O, so test-first is cheap and
the payoff is large (a wrong sign or a swapped angle convention here
silently mispredicts every future PV forecast). Test-first plan:

- `solar_position`: golden values at known date/time/location — noon on
  the summer/winter solstice at Zunzgen's coordinates (47.4491, 7.8081,
  the same site used throughout the design doc) has a known, calculable
  expected elevation; assert within a fraction of a degree.
- `poa_irradiance_w_m2`: a south-facing, moderately-tilted panel at solar
  noon should read higher than a flat horizontal one for the same GHI at
  the same instant near the summer solstice at this latitude (sun
  passes closer to the panel's normal); a panel facing directly away
  from the sun (incidence angle > 90°) should get zero *direct* component
  and only the diffuse term.
- `cell_temperature_c`: at `poa=0` (night), cell temp must equal air temp;
  at NOCT reference conditions (800 W/m² POA, 20°C air, per definition of
  NOCT itself) cell temp must equal the configured `noct_c`.
- `forecast_ac_kw`: zero at night; monotonically non-decreasing in GHI
  holding time fixed; clamps at `ac_limit_kw` when uncapped DC power would
  exceed it; respects `performance_ratio` as a pure multiplier (doubling it
  doubles output, holding everything else fixed).

## Phase 3 — Snow-cover state model

Add `PvSnowParams`, `PvSnowState`, `PvSnowState::step`, and
`snow_coverage_trajectory` exactly as specified in the design doc.

- Unit tests: fresh snowfall above trigger sets `covered=true` regardless
  of prior state; temperature at/above `clear_threshold_c` clears a
  covered state; temperature below the threshold with no new snowfall
  holds the covered state; a `snow_coverage_trajectory` fed a known
  sequence (snow → cold → cold → warm → clear) produces the expected
  per-hour `covered` sequence.
- Wire `forecast_ac_kw` to accept the trajectory's per-slot `covered` flag
  as its final override multiplier, per the design doc's "applied last, not
  blended in" rule. Add a regression test: a covered slot's forecast must
  be `× covered_output_fraction` regardless of how high the transposition
  math alone would put it.
- Leave the `initial` state's source (telemetry cross-check vs.
  forecast-only fallback) as a composition-root wiring decision — not this
  phase's problem, per the design doc's own conclusion. Implement the
  forecast-only fallback first (it needs nothing external); the telemetry
  cross-check is a later phase once there's a live PV feed to compare
  against.

## Phase 4 — Port trait and mock adapter

Add `controller/weather_port.rs`: the `WeatherForecastPort` trait exactly
as specified (`async fn latest(&self) -> Option<WeatherForecast>`).

- Add a mock adapter under `services/test_support/` (mirrors
  `mock_vtn.rs`, `mock_solver_port.rs`) — a trivial `Arc<Mutex<Option<WeatherForecast>>>`-backed
  implementation any test can seed and swap without touching MQTT.
- Adapter-contract test: any two implementations of the trait (the mock,
  and later the real MQTT adapter) must satisfy the same behavioral
  contract — `latest()` returns `None` before any data has arrived, and the
  most recently set value afterward. Write this as a shared test helper
  function taking `&dyn WeatherForecastPort`, callable against both.

## Phase 5 — MQTT adapter

Add the concrete adapter — likely `VEN/src/weather.rs` at the top level,
mirroring how `vtn.rs` is the infra-layer adapter implementing `VtnPort`
today (same ring: infra, same shape: one file owns the wire
protocol/client for one external system).

- Subscribes to `<root>/weather/<site_id>/forecast` and
  `<root>/weather/<site_id>/status` per the wire contract (QoS 1,
  clean-session semantics compatible with retained-message delivery on
  (re)connect).
- On a `forecast` message: parse against the JSON Schema's field set,
  reject (log + drop, do not panic) any message missing a `required`
  field or failing a documented `minimum`/`maximum` bound — these are
  malformed-producer bugs, not VEN bugs, and must never crash the
  process. On success, write into the shared `tokio::sync::watch` channel
  that `latest()` reads (per the "in-process seam" section of the design
  doc).
- On a `status` message: track `last_status` for a future health/diagnostics
  surface (out of scope to expose anywhere yet — just don't discard it).
- Config surface: broker host/port, `<root>` prefix, `site_id` — follow
  the existing env-var-driven config pattern used for the VTN adapter
  (e.g. analogous to however `VTN_BASE_URL` is threaded through today)
  rather than inventing a new config mechanism.
- Adapter-contract tests: feed the adapter's parsing function (factor the
  "parse incoming MQTT payload bytes → `WeatherForecast`" step out as a
  standalone function, independent of the actual `rumqttc` event loop, so
  it's unit-testable without a broker) every example payload from the wire
  contract doc, plus deliberately malformed variants (missing required
  field, out-of-range value, wrong type) and assert each malformed case is
  rejected without panicking.
- Integration test: a real (or `testcontainers`-launched) Mosquitto
  instance, publish a message, assert `latest()` returns it — this is the
  one test in this phase that needs Pi4/Docker rather than running purely
  locally; matches how E2E/resilience suites already run there.

## Phase 6 — Composition root wiring

In `main.rs`, construct `Arc<dyn WeatherForecastPort>` (real adapter or a
no-op that always returns `None` if no broker is configured) and add it to
`AppState`, following the exact pattern already used for `VtnPort` and
`SolverPort` (`Arc<dyn Trait>` field, constructed once at startup, cloned
into whatever task needs it).

- No behavior change yet for anything that isn't explicitly wired to read
  it — this phase is purely "the port exists and is reachable," not "the
  planner uses it."

## Phase 7 — Planner integration (the actual behavior change)

This is where a live weather feed starts affecting the plan, not just an
API-visible forecast. Two integration points, and they are **not** the
same thing:

1. **Planner input**: `PvInverter::build_milp_context` currently always
   uses the sin-model (`natural_irradiance_at`) for the planner's own
   `p_pv_kw` input. Add a variant that, when `WeatherForecastPort::latest()`
   returns a fresh-enough forecast (see staleness policy below), computes
   `forecast_ac_kw` per planning slot instead and uses *that* as the
   planner's input. Fall back to the sin model when no weather feed is
   configured or the cached forecast has gone stale — this is the
   "parallel, additive" property from the design doc, not a replacement.
2. **API-visible forecast**: `services::forecast::build_asset_forecasts`
   (or a new sibling function, since PV isn't necessarily in
   `planned_kw_by_asset` the same way as controllable assets — needs
   checking once Phase 7 is reached) tags the PV forecast with
   `ForecastSource::WeatherModel` and folds `irradiance_variability` into
   `confidence` exactly as the design doc specifies:
   `confidence = base_confidence(age_h) × (1.0 − irradiance_variability.unwrap_or(1.0))`.

**Staleness policy** (an open question in the design doc, resolved here
for the sake of shipping something): reject a cached `WeatherForecast` for
planning purposes once `now - fetched_at` exceeds some multiple of the
expected fetch cadence (the wire contract's topic-1 cadence is "hourly,
never faster than 5 min" — a reasonable first threshold is 2 hours, tune
once real data is flowing). This mirrors `StaleRatePolicy`'s existing
shape (`controller/milp_planner/stale_rates.rs`) closely enough that it's
worth checking whether it can literally reuse that enum/mechanism rather
than inventing a parallel one — a call to make once this phase starts,
not now.

- Tests: planner-input tests asserting the MILP context uses the
  weather-sourced forecast when a fresh one is available and the sin model
  when it isn't (fresh vs. stale vs. absent — three cases); confidence
  formula tests as already described in the design doc's own worked
  example.

## Phase 8 — BDD / E2E coverage

Add a Pi4 BDD scenario (per the `testing` rule's 4-suite structure): given
a weather forecast message published to the test Mosquitto broker, when a
plan cycle runs, then the resulting plan's PV allocation reflects the
weather-sourced forecast rather than the sin model. This is the first test
in the whole plan that exercises the real MQTT transport, the real parser,
the real port, and the real planner together — everything before this
phase tests one layer at a time by design.

## Phase 9 — Documentation and debt bookkeeping

- Record the build in `docs/history/project_journal.md` per the project's
  `workflow` rule: what was built, why, and what was learned (the
  clear-sky-index transposition choice over porting the flux script's
  hand-tuned constant is exactly the kind of decision worth capturing).
- Add anything discovered mid-build to `docs/reference/KEY_LEARNINGS.md`.
- Record known-deferred accuracy gaps from the design doc's "what's
  missing" list (horizon/shading obstructions, Perez/HDKR diffuse model,
  module degradation) in `docs/reference/TECHNICAL_DEBTS.md` as new debt entries
  rather than letting them sit undocumented, per the `refactoring` rule.
- Run `scripts/audit_file_sizes.py` across every new/touched file before
  considering any phase done — several of the modules sketched above
  (physics functions, the MQTT adapter) are plausible candidates to brush
  up against the 500-line cap once fully implemented with their test
  modules; split earlier rather than retrofitting a split later.

## Explicitly out of scope for this plan

Carried over from the design doc's own "open questions," not resolved
here because they don't block a first working version:

- Multi-source fusion (more than one weather feed configured for the same
  site).
- Broker security hardening beyond what Phase 5 needs to function (TLS,
  auth) — note the design doc's callout that the existing Mosquitto
  deployment currently allows anonymous connections on its plaintext
  listener.
- The telemetry-based snow-cover initial-state cross-check (Phase 3 ships
  the forecast-only fallback only).
- Horizon/shading modeling, Perez/HDKR diffuse model, module degradation —
  tracked as technical debt (Phase 9), not built now.

## File layout

None of this lands in one Rust file. Two independent constraints force the
split: the hexagonal dependency rule (inner rings — `entities/`,
`controller/` — never import outer rings, so pure domain code cannot share
a file with an MQTT client), and the file-size audit
(`scripts/audit_file_sizes.py`, 500 production-line cap on `VEN/src/`
files) — several of the pieces below are plausible candidates to approach
that cap on their own once fully implemented with tests.

| File | Layer | Contents |
|---|---|---|
| `entities/design_vocabulary.rs` (split into a new `entities/weather.rs` if size forces it — not decided until Phase 1 actually adds the types and the audit script is run) | Domain | `SkyCondition`, `WeatherForecastSample`, `WeatherForecast`, `GeoPosition` |
| `entities/asset_params.rs` | Domain | `PvArrayGeometry`, `PvForecastParams` (alongside the existing `PvParams`) |
| `entities/solar.rs` (new) | Domain | `solar_position`, `poa_irradiance_w_m2`, `cell_temperature_c`, `forecast_ac_kw` |
| new file for the snow model (Phase 3 named the types but not a file — a placeholder name like `entities/pv_snow.rs` is a reasonable guess, not yet decided) | Domain | `PvSnowParams`, `PvSnowState`, `snow_coverage_trajectory` |
| `controller/weather_port.rs` (new) | Application/port | `WeatherForecastPort` trait — mirrors `vtn_port.rs`, `solver_port.rs` |
| `services/test_support/mock_weather_port.rs` (new) | Test-only | mock adapter, mirrors `mock_vtn.rs` |
| `VEN/src/weather.rs` (new, top-level) | Infra/adapter | the MQTT client, message parsing/validation, LWT/heartbeat handling — mirrors how `vtn.rs` is the HTTP adapter implementing `VtnPort` |
| `main.rs` | Composition root | constructs `Arc<dyn WeatherForecastPort>`, adds it to `AppState` |
| `assets/pv.rs` / `controller/milp_planner/*` | Infra/planner | wires the weather-sourced forecast into `PvInverter::build_milp_context` as an alternative to the sin model, gated by the staleness policy |
| `services/forecast.rs` (or a sibling file, if it would push past the size cap) | Application | builds the API-visible `AssetForecast` with `ForecastSource::WeatherModel` + the confidence formula |
| `VEN/Cargo.toml` | — | the new `rumqttc` dependency (not logic, but a touched, reviewable file in its own right per the `dependencies` rule) |
| `tests/features/*.feature` + `tests/features/steps/` | BDD | the Phase 8 scenario — this project's existing BDD suite location, confirmed present |
| `docs/history/project_journal.md`, `docs/reference/KEY_LEARNINGS.md`, `docs/reference/TECHNICAL_DEBTS.md` | Docs | Phase 9 bookkeeping — not Rust, but explicitly required by this project's `workflow`/`refactoring` rules |

This table corrects and completes the answer given in conversation before
being appended here: it adds the `Cargo.toml` dependency change, the BDD
feature-file location (verified present at `tests/features/` with a
`tests/features/steps/` step-definitions directory), and the documentation
files from Phase 9, none of which were in the original spoken answer; and
it flags the Phase 3 snow-model file as an open naming decision rather than
stating it as settled, since the plan itself never named a file for it.
