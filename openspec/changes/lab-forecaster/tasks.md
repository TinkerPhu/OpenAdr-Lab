## 1. Separate the code from VEN types into crate `lab-forecast` (every existing test unchanged and green after each step)
- [ ] 1.1 Create crate `lab-forecast` (pinned semver deps, licence check); add to the workspace and the VEN
- [ ] 1.2 Test first: `TickSource` trait (only `query_ticks`); the VEN's history store implements it; the learner's existing tests run against a mock `TickSource`
- [ ] 1.3 Move the weather parse, `PvArrayGeometry`/`PvForecastParams`/`PvSnowParams`, `solar.rs`, `pv_snow.rs` (drop `ts_rs` from the moved code; keep the generated UI types in the VEN as thin wrappers)
- [ ] 1.4 Move the learner and a plain `LearnedProfile` parameter struct; the VEN keeps the `AssetHeuristics` adapter
- [ ] 1.5 Run `audit_ven_architecture.py`, `audit_duplication.py`, VEN unit tests and clippy; register any copy left over in `TECHNICAL_DEBTS.md`

## 2. Contract
- [ ] 2.1 Test first: forecast payload constructor and validator in `lab-forecast` (cannot build a payload without quantity/unit/sign/reading; sign per kind; `valid_until`; `coverage_h`)
- [ ] 2.2 `docs/architecture/forecast_publication.md` (topics, payload, freshness, status) and an entry in `docs/reference/WIRE_PROFILE.md`

## 3. Forecaster service (tests first)
- [ ] 3.1 Crate `forecaster/`: config from the VEN profile (arrays, measurement topics, site), injectable clock
- [ ] 3.2 MQTT inputs (weather, PV and base-load measurement, via the existing translation) and the SQLite tick cache
- [ ] 3.3 Snow-state observation per array and the PV publishers (per array and total)
- [ ] 3.4 Base-load learner run and publisher; cold start profile with `model_version`
- [ ] 3.5 Status topic with last will, `producer_id` and the single-producer guard
- [ ] 3.6 Dockerfile and compose service; runs with the broker alone, no InfluxDB

## 4. VEN consumption
- [ ] 4.1 Subscriber adapter (via `PayloadReader` and the payload validator), source preference with fallback, boundary conversion in `entities/units.rs`
- [ ] 4.2 Inject the received series into the PV asset and the base-load asset; no other module interprets it
- [ ] 4.3 VEN UI: source, age, rejection reason
- [ ] 4.4 Accuracy samples against the received series with `model_version`

## 5. Tests and docs
- [ ] 5.1 BDD scenarios in `tests/features/` plus `audit_step_definitions.py`
- [ ] 5.2 Use-case doc; update `forecasting_model.md` and `weather_forecast.md`
- [ ] 5.3 Journal and KEY_LEARNINGS; `openspec validate lab-forecaster`
- [ ] 5.4 Report to the coordination session: new card for the digest to read these topics and retire its VEN-REST read; the sign and unit mapping to the catalogue
- [ ] 5.5 Wave into current-state docs and delete this change after the merge
