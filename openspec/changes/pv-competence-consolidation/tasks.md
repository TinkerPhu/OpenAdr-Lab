## 1. Verify the mechanism before coding

- [ ] 1.1 Confirm `entities::solar::weather_pv_kw_for_slots`/`weather_pv_forecast_series`'s
      exact return types (`WeatherPvForecastSlot`, etc.) and what a `None`/empty series
      means at each call site — needed to design `PvInverter`'s new field's `Option` shape
      precisely.
- [ ] 1.2 Read `milp_planner/inputs.rs`'s full `pv_ceiling_kw` call site (including the
      `pv_forecast_override`/deterministic-testing-pin precedence) side-by-side with
      `PvInverter::uncurtailed_power_kw`/`max_effort_setpoint`'s existing precedence, to
      confirm D4's equivalence claim or find the real difference before writing code.
- [ ] 1.3 Grep every consumer of `build_forecast_frames`'s PV output and
      `resolve_weather_pv_kw_for_tick`'s `slots_kw` return value, beyond
      `capacity_headroom.rs`'s two call sites already found — confirm nothing else depends
      on `pv_frames` before any deletion task below.

## 2. Reparametrize the decaying offset to a single time constant `τ` (D7)

Done first, mechanically, fully verified on its own — before any new forecast logic (section
3) is layered on top, per design.md's own risk mitigation.

- [ ] 2.1 Add `PvSmoothingState::decayed_offset_after(&self, elapsed_s: f64, tau_s: f64) ->
      f64` (`self.irradiance_offset * (-elapsed_s / tau_s).exp()`) with its own test-first
      coverage (matches today's `next_offset` output at `elapsed_s = dt_s` for the
      not-forced/not-already-decayed-to-zero case — confirms the reparametrization is
      numerically equivalent to today's `(1-alpha)^(dt_s/300)`, not just structurally similar).
- [ ] 2.2 Rewrite `PvSmoothingState::next_offset`'s decay branch to call
      `decayed_offset_after` (passing `tau_s`) instead of computing
      `(1 - pv_alpha).powf(dt_s / PLAN_STEP_S)` directly — `PLAN_STEP_S`/`pv_alpha` params
      removed from this function once callers are updated (task 2.3).
- [ ] 2.3 Rename `pv_alpha` → `tau_s` (seconds) throughout, converting each default/test
      value via `τ = −300 / ln(1 − alpha)` so behavior is unchanged: `PvInverter.pv_alpha`,
      `TickOverrides.pv_alpha` → `pv_tau_s`, `PvCeilingParams.pv_alpha` (until deleted in
      task 4.4), the `pv_irradiance_alpha` inject/override key → e.g. `pv_tau_s`,
      `state_values`'s `"pv_alpha"` map key → `"tau_s"`. Grep-confirm every file found
      during design (`dispatcher.rs`, `plan_context.rs`, `pv_preview.rs`, `simulator_port.rs`,
      `mock_simulator_port.rs`, `milp_planner/tests/`) is updated, not just the primary ones.
- [ ] 2.4 Re-model the "Blend-back Speed" control (`PvInverter::control_schema`) to expose
      `τ` in seconds directly — choose min/max that read sensibly (e.g. a few seconds up to
      roughly an hour), default `≈2848.5s` to preserve today's behavior exactly, and note the
      inversion (lower `τ` = faster decay) in the descriptor's label if the schema supports a
      tooltip/description field.
- [ ] 2.5 `VEN/ui/src/api/types.ts`: rename the corresponding inject/override field to match
      2.3's new key; update `AssetRightSection.test.tsx`, `pv_irradiance_one_shot.test.ts`,
      `useSetSimInject.test.tsx` for the new name/range.
- [ ] 2.6 Persisted-state backward compatibility: deserializing an old `sim_state.json` with
      the legacy `pv_alpha` key must not fail the whole load (`#[serde(default)]`, matching
      `inverter_max_kw`'s existing precedent in the same file) — prefer converting the old
      value to its equivalent `τ` over silently discarding it, per design.md D7.
- [ ] 2.7 Full suite green (`wsl cargo test -j 2` under `wsl_lock.sh`; `cd VEN/ui && npm
      test`) before starting section 3 — this rename must land clean on its own, not be
      diagnosed later mixed in with new forecast-logic changes.

## 3. `PvInverter` — new weather-forecast-aware methods (test-first)

- [ ] 3.1 Add `WeatherPvSeries`-typed field to `PvInverter` (name TBD from 1.1's findings),
      `#[serde(default)]`/`NOT from YAML`/`Set each tick by the sim loop` — matching
      `weather_power_kw`'s existing doc pattern exactly.
- [ ] 3.2 Add `pv_weather_forecast: Option<WeatherPvSeries>` to `TickOverrides`
      (`assets/asset_trait.rs`); write it onto the new `PvInverter` field in
      `apply_tick_overrides`.
- [ ] 3.3 Populate the new `TickOverrides` field each tick in `simulator/mod.rs`, from the
      same already-resolved weather data `pv_weather_power_kw` is populated from (the raw
      `weather_pv_forecast_series` output, per design.md D1).
- [ ] 3.4 Write the test-first case: `max_effort_schedule`'s `t1` point equals
      `max_effort_setpoint(Export, Physical)`'s answer, for a representative set of
      states (curtailed, forced-override, weather-present, weather-absent) — confirm it
      fails against the current (nonexistent) override, then implement
      `PvInverter::max_effort_schedule` to make it pass.
- [ ] 3.5 Write the test-first case: a future point samples the injected weather series via
      `weather_pv_kw_for_slots` correctly (not a flat repeat of `t1`'s value), including the
      `inverter_max_kw`/active `generation_limit_kw` clamp still applying — then implement.
- [ ] 3.6 Write the test-first case: with no weather forecast injected (`None`/stale),
      `max_effort_schedule` falls back to the sin-model (`irradiance_at`) for `t > t1`,
      decaying any residual offset via `decayed_offset_after` — confirm graceful
      degradation, not a panic or silent zero.
- [ ] 3.7 Upgrade `Pv::forecast()` per design.md D5 to use the same weather-sampling and
      offset-decay path; add/update its own tests for the weather-present and
      weather-absent cases.

## 4. Retire `pv_ceiling_kw`'s two real call sites

`milp_planner/inputs.rs` and `simulator/forecast.rs::insert_pv_points` — corrected count,
see design.md D4 (the original proposal missed the second one).

- [ ] 4.1 Write the numeric-equivalence test first (design.md's called-for risk mitigation):
      same state/weather inputs, old `pv_ceiling_kw`-derived `p_pv_kw` vs. new
      `PvInverter`-derived value, across curtailed/uncurtailed/override/no-weather cases —
      confirm it currently passes trivially (both paths coexist), then swap the production
      call site and confirm it still passes.
- [ ] 4.2 Replace `milp_planner/inputs.rs`'s `pv_ceiling_kw` call with a call into
      `PvInverter`'s own method (directly, or via `asset_max_power_series` if that fits the
      per-slot loop shape better — decide against the actual loop structure, not
      abstractly).
- [ ] 4.3 `simulator/forecast.rs::insert_pv_points`'s PV-specific code (including its
      `pv_ceiling_kw` call) is deleted outright as part of section 5 (it exists only to help
      build `pv_frames`, which section 5 retires) — do not migrate it, confirm it becomes
      dead code once section 5 lands.
- [ ] 4.4 Per design.md D4: delete `pv_ceiling_kw`/`PvCeilingParams` from `entities/solar.rs`
      once both real call sites (4.2, 4.3) are gone, if 1.2 confirmed equivalence; otherwise
      fold the confirmed real difference into `PvInverter` itself first — either way, no two
      formulas left standing.

## 5. Retire `pv_frames`'s PV special-casing (`capacity_headroom.rs`)

- [ ] 5.1 Remove the `"pv" => continue` exclusion in the shared capacity-curve aggregator
      (`capacity_headroom.rs:98`) — let PV flow through the generic per-asset
      `asset_max_power_series` loop like every other asset.
- [ ] 5.2 Remove the equivalent exclusion in `compute_site_headroom_forecast`
      (`capacity_headroom.rs:352`) — let PV flow through `simulated_trajectory`'s generic
      per-asset loop there too.
- [ ] 5.3 Delete `pv_capacity_events` and its call site (`capacity_headroom.rs:115`) —
      confirmed dead once 5.1/5.2 land.
- [ ] 5.4 Per 1.3's findings: delete `pv_frames`'s now-unused plumbing
      (`resolve_weather_pv_kw_for_tick`'s `slots_kw` output, `build_forecast_frames`'s PV
      handling) only if confirmed to have no other consumer — otherwise leave it, documented
      why, rather than break something 1.3 didn't find.
- [ ] 5.5 Update `capacity_headroom.rs`'s module doc (the D1/PV-is-special paragraph) to
      reflect that PV is no longer special-cased — it was a deliberate, documented exception
      before this change; state plainly that it no longer is, per `asset-competence-assurance`.

## 6. Full verification

- [ ] 6.1 `wsl cargo test -j 2` (full suite) under `wsl_lock.sh`.
- [ ] 6.2 `cargo fmt --check` / `cargo clippy --all-targets --all-features -- -D warnings`.
- [ ] 6.3 `scripts/audit_file_sizes.py` (`pv.rs`/`pv_smoothing.rs`/`capacity_headroom.rs`/
      `inputs.rs` line counts — `pv.rs` is gaining real logic, check it doesn't cross the
      500-line cap).
- [ ] 6.4 E2E/resilience on Node2 (`DOCKER_HOST=Node2 bash run_all_tests.sh --e2e
      --resilience`) — the `capacity_envelope_absolute_quantities.feature` PV-Import BDD
      scenario specifically re-verifies the confirmed-fixed-by-construction PV-Import bug
      still holds after this refactor, not just that it passes.
- [ ] 6.5 Manual check: inject a weather forecast (or observe the live weather feed) and
      confirm the Controller's capacity-curve overlay and Diagnostics capacity-forecast
      chart show genuine weather-driven variation across the sweep for PV's Export curve,
      not a flat line.
- [ ] 6.6 Manual check: the re-modeled "Blend-back Speed" slider (now in seconds) produces
      the same default decay behavior as before, and its min/max feel sensible in the UI.

## 7. Docs and close-out

- [ ] 7.1 Update `docs/architecture/VEN_ARCHITECTURE.md` §3.0c (Unified Capacity/Headroom
      Engine) to remove the "PV is asset-kind-and-direction-special, by design" framing —
      it no longer is; note the resolution in §3.0d instead.
- [ ] 7.2 Add a `docs/history/project_journal.md` entry: what changed, the numeric
      equivalence verification result, any real difference found in task 1.2/4.4, and the
      `τ` reparametrization (including the found `zone_a_step_s`/`PLAN_STEP_S` divergence
      risk this closes).
- [ ] 7.3 Update `docs/plans/asset-competence-assurance-master-plan.md`'s Phase 1 status to
      complete.
- [ ] 7.4 Delete `openspec/changes/pv-competence-consolidation/` once merged, per this
      repo's no-lingering-plans workflow rule.
