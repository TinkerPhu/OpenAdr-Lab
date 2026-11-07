## Order (D14)
Slice 1 (R-81): 1.3, 2.1, 3.1, 3.2 (PV and weather input only), 3.3, 3.5, 3.6, then a BDD scenario for the retained PV topics and the
PV rows of 5.2. Slice 2: 1.2, 1.4, 3.4, base load. Slice 3: section 4 (VEN consumption). Each slice is handed to CI on its own.

## 1. Separate the code from VEN types into crate `lab-forecast` (every existing test unchanged and green after each step)
- [x] 1.1 Create crate `lab-forecast` beside `lab-core` (pinned semver deps, licence check); the VEN depends on it by path; add it to `VEN/Dockerfile` (COPY beside `lab-core`), the BFF/test compose contexts that copy `lab-core`, and `ci/runner.sh` / `ci/suites.map` so a change there runs the Rust suites
- [x] 1.2 (done as a plain `LoadSample` slice instead of a trait: `learn_asset_heuristics` maps the store's ticks to samples; its existing tests run unchanged against the mock history port)
- [x] 1.3 Move the weather parse and status wire (`weather_wire`), `PvArrayGeometry`/`PvForecastParams`/`PvSnowParams`, `solar.rs`, `pv_snow.rs`. The moved types keep their `ts_rs` derive (a proc-macro, no runtime cost) and stay listed in `VEN/src/ui_types.rs`, so no wrapper types are needed
- [x] 1.4 Move the learner and a plain `LearnedProfile` parameter struct; the VEN keeps the `AssetHeuristics` adapter
- [ ] 1.5 Run `audit_ven_architecture.py`, `audit_duplication.py`, VEN unit tests and clippy; register any copy left over in `TECHNICAL_DEBTS.md`

## 2. Contract
- [x] 2.1 Test first: forecast payload constructor and validator in `lab-forecast` (cannot build a payload without quantity/unit/sign/reading; sign per kind; `valid_until`; `coverage_h`)
- [x] 2.2 `docs/architecture/forecast_publication.md` (topics, payload, freshness, status) and an entry in `docs/reference/WIRE_PROFILE.md`

## 3. Forecaster service (tests first)
- [x] 3.1 Crate `forecaster/`: config from the VEN profile (arrays, measurement topics, site), injectable clock
- [x] 3.2 (slice 1: weather and PV measurement over MQTT through the shared parsers; the SQLite tick cache and the base-load measurement come with slice 2, where the learner needs history) MQTT inputs
- [x] 3.3 Snow-state observation per array and the PV publishers (per array and total)
- [x] 3.4 Base-load learner run and publisher; cold start profile with `model_version`
- [x] 3.5 Status topic with last will, `producer_id` and the single-producer guard
- [x] 3.6 Dockerfile and compose service; runs with the broker alone, no InfluxDB

## 4. VEN consumption
- [x] 4.1 Subscriber adapter (via the payload validator), source preference with fallback, boundary conversion in `entities/units.rs`
- [x] 4.2 Inject the received series into the PV asset and the base-load asset; no other module interprets it
- [x] 4.3 VEN UI: source, age, rejection reason (Weather page, `forecast_sources` in `GET /weather`)
- [x] 4.4 Accuracy samples against the received series with `model_version`

## 5. Tests and docs
- [ ] 5.1 BDD scenarios in `tests/features/` plus `audit_step_definitions.py` (forecast_publication.feature covers the container; the VEN side is covered by unit tests only: enabling the consumer on `test-ven-1` would break the weather scenarios that expect the sin-model fallback, because the forecaster's retained series stays valid for 90 min; a dedicated test VEN is the follow-up)
- [x] 5.2 Use-case doc; update `forecasting_model.md` and `weather_forecast.md`
- [ ] 5.3 Journal and KEY_LEARNINGS; `openspec validate lab-forecaster`
- [x] 5.4 Report to the coordination session: new card for the digest to read these topics and retire its VEN-REST read; the sign and unit mapping to the catalogue
- [x] 5.5 (done on Node2 and Node3; Node1 has a full tree) Before the first host run: `git sparse-checkout add lab-forecast` (and `forecaster`) on the Node hosts, with the user's go (the pre-flight in `run_all_tests.sh` aborts otherwise)
- [ ] 5.6 Wave into current-state docs and delete this change after the merge

## 6. Read-only HTTP view (D15)
- [ ] 6.1 Decide the port and the UI surface (a line on the Weather forecast-source panel with the URL, or a justified exemption); record it in D15
- [ ] 6.2 Test first (`forecaster`): the view returns exactly the bytes last published for each topic (byte equality against the published message), 404 for an unknown path, 503 before the first publish, nothing served when `FORECASTER_HTTP_PORT` is unset
- [ ] 6.3 The publisher keeps the last payload per topic in one place: the function that publishes is the one that stores; the HTTP handler only reads that store (one serialiser, no second contract copy)
- [ ] 6.4 Server in `forecaster/src` (axum, caret-pinned to the version the VEN uses; review the new import and licences); compose: port on the Lab network, the healthcheck may use it
- [ ] 6.5 BDD scenario in `forecast_publication.feature`: GET returns a payload that `ForecastPayload::parse` accepts and equals the retained MQTT message; run `audit_step_definitions.py`
- [ ] 6.6 Docs: `forecast_publication.md` (endpoint, and the rule that MQTT stays the channel), journal and KEY_LEARNINGS; `openspec validate lab-forecaster`
