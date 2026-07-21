## 1. Dependencies and scaffolding

- [x] 1.1 Add `rumqttc` to `VEN/Cargo.toml`, pinned to a semver range per the `dependencies` rule
- [x] 1.2 Run `cargo audit` and verify the license is on the project's acceptable-license list — found 4 advisories transitively via rumqttc's default `use-rustls` feature (old rustls-webpki/unmaintained rustls-pemfile); fixed by building with `default-features = false` (plaintext MQTT to the local broker needs no TLS feature). `cargo audit` now exits clean. License: Apache-2.0 (on the acceptable list).
- [x] 1.3 Confirm the crate compiles unused (`wsl cargo check -p ven-app -j 2`, under the wsl-lock) before any logic is added

## 2. Domain entities

- [x] 2.1 Add `SkyCondition`, `WeatherForecastSample`, `WeatherForecast`, `GeoPosition` — landed in a new `entities/weather.rs` (file-size cap consideration decided in favor of a dedicated file up front)
- [x] 2.2 Write serde round-trip tests for each type against the wire contract's documented example JSON messages
- [x] 2.3 Write construction/equality tests for `SkyCondition`
- [x] 2.4 Verify no `use crate::profile` appears in these types

## 3. PV geometry and physics (pure domain functions)

- [x] 3.1 Add `PvArrayGeometry`, `PvForecastParams` to `entities/asset_params.rs`, additive alongside the existing `PvParams`
- [x] 3.2 Add `entities/solar.rs` with `solar_position(pos, t) -> SolarPosition`
- [x] 3.3 Write golden-value tests for `solar_position` (Zunzgen, summer/winter solstice noon, midnight below horizon)
- [x] 3.4 Implement the internal clear-sky GHI model (horizontal and panel-plane variants)
- [x] 3.5 Implement `poa_irradiance_w_m2(ghi_w_m2, sun, panel) -> f64` using the clear-sky-index method
- [x] 3.6 Write tests: south-facing tilted panel exceeds horizontal near solar noon at solstice; panel facing >90° incidence angle gets zero direct component; nighttime irradiance is zero
- [x] 3.7 Implement `cell_temperature_c(air_temp_c, poa_w_m2, noct_c) -> f64` (NOCT model)
- [x] 3.8 Write tests: cell temp equals air temp at zero irradiance; cell temp equals `noct_c` at NOCT reference conditions
- [x] 3.9 Implement `forecast_ac_kw(params, sample, t, snow_state) -> f64` composing transposition → DC power → cell-temp derate → performance ratio → AC clamp → snow override
- [x] 3.10 Write tests: zero at night; monotonically non-decreasing in GHI; clamps at `ac_limit_kw`; `performance_ratio` is a pure linear multiplier

## 4. Snow-cover state model

- [x] 4.1 Add `PvSnowParams` (`entities/asset_params.rs`), `PvSnowState`, `PvSnowState::step` (`entities/pv_snow.rs`)
- [x] 4.2 Write tests: fresh snowfall above trigger sets `covered=true` regardless of prior state; temperature at/above `clear_threshold_c` clears a covered state; sustained cold with no new snowfall holds the covered state (plus a below-trigger negative case)
- [x] 4.3 Implement `snow_coverage_trajectory(initial, params, samples) -> Vec<PvSnowState>` as a pure fold
- [x] 4.4 Write a trajectory test with a known sequence (snow → cold → cold → warm → clear) asserting the expected per-hour `covered` sequence
- [x] 4.5 Wire the trajectory's per-slot `covered` flag into `forecast_ac_kw` as the final override multiplier
- [x] 4.6 Write a regression test: a covered slot's forecast is `× covered_output_fraction` regardless of how high the transposition math alone would compute
- [x] 4.7 Implement the forecast-only fallback for `initial` state (`PvSnowState::default()`, uncovered, then folded forward from the forecast's own `age_h=0` sample) — demonstrated by `trajectory_bootstraps_from_forecasts_own_fact_sample` (added during self-review; this is a calling convention on the existing generic `snow_coverage_trajectory`, not separate code). Telemetry-based cross-check deferred (R-55)

## 5. Port trait and mock adapter

- [x] 5.1 Add `controller/weather_port.rs` with the `WeatherForecastPort` trait + `NoopWeatherPort`
- [x] 5.2 Add `services/test_support/mock_weather_port.rs`, mirroring `mock_vtn.rs`/`mock_solver_port.rs`
- [x] 5.3 Write a shared adapter-contract test helper (`assert_returns_none_before_any_data`) — run against the mock now; not yet run against the real MQTT adapter (would need a live/mocked broker, see 6.9)

## 6. MQTT adapter

- [x] 6.1 Add `VEN/src/weather.rs` implementing `WeatherForecastPort`, mirroring `vtn.rs`'s adapter shape
- [x] 6.2 Implement MQTT subscription to `<root>/weather/<site_id>/forecast` and `.../status` (QoS 1)
- [x] 6.3 Factor "parse incoming MQTT payload bytes → `WeatherForecast`" into a standalone function (`parse_forecast_message`), independent of the `rumqttc` event loop
- [x] 6.4 Implement schema validation on the parse path (`validate_forecast`): reject (log + drop, never panic) any message missing a required field or failing a documented bound
- [x] 6.5 Write adapter-contract tests: valid message, missing required field, out-of-range temperature/GHI, unknown-fields-ignored, valid/invalid status message
- [x] 6.6 Implement `status` message tracking (`last_status`) and liveness detection (`is_alive`, dead if no status message within 2× the heartbeat interval)
- [x] 6.7 Write the `latest()` implementation backed by a `tokio::sync::watch` channel updated on every valid inbound `forecast` message
- [x] 6.8 Add config surface (`WeatherMqttConfig::from_env`: `WEATHER_MQTT_HOST`/`_PORT`/`_ROOT`/`_SITE_ID`), following the existing env-var-driven pattern used for the VTN adapter
- [ ] 6.9 **Deferred.** Integration test against a real (or `testcontainers`-launched) Mosquitto instance — requires Pi4/Docker; not run this session (see R-56)

## 7. Composition-root wiring

- [x] 7.1 In `main.rs`, construct `Arc<dyn WeatherForecastPort>` (real adapter if `WEATHER_MQTT_HOST` is set, `NoopWeatherPort` otherwise) and add it to `AppCtx`, mirroring the existing `VtnPort`/`SolverPort` wiring
- [x] 7.2 Verify no behavior changes yet for anything not explicitly reading the new port — full suite run (743/743 passed, no regressions)

## 8. Planner integration

- [x] 8.1 Decide the staleness threshold mechanism: `WeatherForecast::is_fresh(now, max_age)`, a simple duration check (2h starting default) rather than reusing `StaleRatePolicy`'s enum shape — documented as an open question in design.md, resolved pragmatically here
- [x] 8.2 **Landed** (follow-up session, after R-51's `weather_pv` profile config existed to unblock it). Weather-sourced PV input wired end to end: `SolveRequest.weather_pv_kw` → `MilpSolver::solve` → `run_planner` → `inputs::build_milp_inputs` (new `weather_pv_kw: Option<&[f64]>` param, precedence: `pv_forecast_override` > `weather_pv_kw` > sin-model/live-snapshot fallback, unchanged when `None`). The staleness/config resolution itself is `entities::solar::resolve_weather_pv_kw` (pure) plus `services::planning::resolve_weather_pv_kw_for_cycle`/`build_solve_request` (the async port-fetch wrapper). Only 9 real call sites needed updating (test files call local wrapper functions, not the production functions directly, once traced) — far fewer than the "6+ call sites, risky" estimate from the original pass.
- [x] 8.3 Planner-input tests for the three cases — `resolve_weather_pv_kw_fresh_forecast_and_config_is_used`, `_stale_forecast_falls_back`, `_no_config_falls_back`, `_no_forecast_received_falls_back` (`entities/solar.rs`), plus precedence tests at the `build_milp_inputs` level (`weather_pv_kw_overrides_sin_model_fallback`, `weather_pv_kw_none_falls_back_to_sin_model`, `pv_forecast_override_wins_over_weather_pv_kw` in `controller/milp_planner/tests/pv.rs`) — 7 new tests total
- [x] 8.4 API-visible forecast path: `services::forecast::build_weather_pv_forecast` tags PV `ForecastSource::WeatherModel`, built from the same `weather_pv_forecast_series` the planner-input path uses (no divergence risk). Wired into `publish_post_cycle_state`/`finish_plan_cycle` (both now take `weather`/`weather_pv_params`) — added only when PV has no Optimization-sourced entry already (always true, PV has no LP decision variable) and the cached forecast is fresh, mirroring the existing heuristics-fallback precedence pattern exactly.
- [x] 8.5 Confidence-formula tests: `slot_confidence_uniform_sky_is_not_reduced`, `_broken_sky_is_maximally_reduced`, `_missing_variability_treated_as_maximal_uncertainty`, `_decays_with_age_h` (`services/forecast.rs`) — matches the design doc's worked example exactly (uniform ⇒ no reduction, broken ⇒ maximal reduction, missing ⇒ maximal uncertainty), plus 2 more tests on `build_weather_pv_forecast` itself (source tag + sign convention, empty-samples edge case)
- [x] 8.6 Full VEN Rust test pyramid — 769/769 passed (final run, after 8.2's wiring, the `tasks/planning/` file-size split, and 8.4/8.5's API-visible forecast tagging), `cargo fmt --check` clean, `cargo clippy --all-targets --all-features -D warnings` clean, file-size audit clean, architecture-invariant greps empty

## 9. BDD / E2E coverage

- [x] 9.1 Add `tests/features/weather_forecast.feature`, tagged `@wip` (excluded from the default suite via `behave.ini`'s `tags = ~@wip`, matching the existing `ven_reports.feature` precedent)
- [x] 9.2 Add corresponding step definitions under `tests/features/steps/weather_forecast_steps.py`
- [ ] 9.3 **Still deferred**, but no longer blocked on 8.2 (now landed) — only on Pi4/Docker access, not attempted this session. The scenarios can be un-`@wip`'d and run via `bash run_all_tests.sh --e2e` once there's a Pi4 session available; the underlying planner behavior they check for is now real.

## 10. Documentation and debt bookkeeping

- [x] 10.1 Record the build in `docs/history/project_journal.md`
- [x] 10.2 Add new learnings to `docs/reference/KEY_LEARNINGS.md`
- [x] 10.3 Record deferred items as R-50..R-56 in `docs/reference/TECHNICAL_DEBTS.md` (planner wiring, profile config surface, liveness surfacing, horizon/shading/Perez model/degradation, broker anonymous auth, snow-model telemetry cross-check, missing E2E coverage)
- [x] 10.4 Run `scripts/audit_file_sizes.py` across every new/touched file — clean
- [x] 10.5 Run `cargo fmt --check` and `cargo clippy --all-targets --all-features -- -D warnings` — both clean
- [x] 10.6 Verify the architecture invariants still hold — both greps empty
