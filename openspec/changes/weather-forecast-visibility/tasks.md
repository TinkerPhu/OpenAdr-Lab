## 1. Profile config surface (closes R-51 for this purpose)

- [x] 1.1 Add an optional `weather_pv` section to the profile YAML schema — landed as `VEN/src/profile/weather_pv.rs` (split out of `schema.rs` to stay under the file-size cap), parsing into `entities::asset_params::{PvArrayGeometry, PvForecastParams, PvSnowParams}`
- [x] 1.2 Wire `profile.weather_pv_params()` into `AppCtx` as `Option<PvForecastParams>` (`main.rs`)
- [x] 1.3 Profile-parsing tests: no-`weather_pv` regression guard, required-fields-parse, tuning-field-defaults, tuning-field-overrides — 3 tests in `profile/weather_pv.rs`

## 2. Derived-series computation (pure, shared)

- [x] 2.1 Add `weather_pv_forecast_series(params, forecast) -> Vec<WeatherPvForecastSlot>` to `entities/solar.rs`
- [x] 2.2 Define `WeatherPvForecastSlot { valid_at, forecast_ac_kw, snow_covered }`
- [x] 2.3 Tests: series length matches sample count; `snow_covered` matches a direct `snow_coverage_trajectory` call; `forecast_ac_kw` matches a direct `forecast_ac_kw` call (no double-computation drift)

## 3. GET /weather route

- [x] 3.1 Add `VEN/src/routes/weather.rs`, registered as `.route("/weather", get(weather::get_weather))`
- [x] 3.2 Response shape implemented: `status` (`ok`/`stale`/`no_forecast`), `is_fresh`, `raw` (nullable), `derived` (nullable)
- [x] 3.3 Route tests (pure `build_weather_response`, no `AppCtx` needed): fresh+config → full response; stale → raw still populated; no forecast → both null; forecast present, no config → derived null — 4 tests

## 4. VEN UI — Plan tab rename

- [x] 4.1 Nav label "Planner" → "Plan" in `App.tsx` (route `/planner`, `data-testid="nav-planner"` unchanged); page heading in `Planner.tsx` renamed to match
- [x] 4.2 Updated `App.test.tsx`'s nav-visibility assertion for the new "nav-weather" entry (no assertion existed on the literal label text, so no update needed there)
- [x] 4.3 Confirmed no other Planner content changed (only the two label strings)

## 5. VEN UI — Weather tab

- [x] 5.1 Added `useWeather` hook (`api/hooks.ts`), `weather()` client method (`api/client.ts`), and wire-matching types (`api/types.ts`)
- [x] 5.2 Added `pages/Weather.tsx` + `components/weather/{WeatherRawPanel,WeatherDerivedPanel}.tsx` (MUI tables, mirroring `components/planner/TraceTable.tsx`'s style)
- [x] 5.3 Added "Weather" nav entry + `/weather` route in `App.tsx`
- [x] 5.4 Implemented all states: raw+derived; no-forecast empty state; stale warning (raw still shown); derived-unavailable (raw present, no PV config)
- [x] 5.5 Component tests for all four states — `__tests__/Weather.test.tsx`, 4 tests

## 6. Verification and bookkeeping

- [x] 6.1 Full VEN Rust test pyramid — 754/754 green (`wsl cargo test -j 2`, under the wsl-lock)
- [x] 6.2 `cargo fmt --check` / `cargo clippy --all-targets --all-features -- -D warnings` — clean (found and fixed one real error along the way: `WeatherPvForecastSlot` needed `#[derive(Serialize)]` for the route to compile)
- [x] 6.3 `scripts/audit_file_sizes.py` — initially failed (`profile/schema.rs` 503/500 lines); fixed by splitting `WeatherPvConfig` + its tests into a new `profile/weather_pv.rs`; passes clean after the split
- [x] 6.4 VEN UI test suite (406/406, +4 new), `tsc --noEmit` (clean), ESLint (clean, one pre-existing unrelated warning)
- [ ] 6.5 **Deferred.** Manual browser verification against a running VEN — requires either a local VEN run (env vars: `VTN_BASE_URL`/`CLIENT_ID`/`CLIENT_SECRET`) or a Pi4 deployment, neither performed this session since deployment wasn't requested; the full test pyramid (unit + route-response tests covering all four UI states) is the verification actually run
- [x] 6.6 Closed R-57 in `docs/reference/TECHNICAL_DEBTS.md`; narrowed R-51 to "planner-side config consumption only" (this change supplies the config surface + a read-only consumer; the planner itself still doesn't read it — that remains R-50)
- [x] 6.7 Recorded the build in `docs/history/project_journal.md`; added learnings to `docs/reference/KEY_LEARNINGS.md`
