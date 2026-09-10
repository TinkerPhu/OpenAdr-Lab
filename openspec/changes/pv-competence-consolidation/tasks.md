## Status (as of this session)

Sections 1-3 done, including D7's τ reparametrization and the new weather-aware
`max_effort_schedule`/`forecast()` methods, plus the UI slider re-model these required
(originally scoped under 2.4/2.5, folded in here since leaving the old `pv_irradiance_alpha`
key live in the UI while the backend expects `pv_tau_s` would have been a silent regression,
not deferred polish).

Section 4 is **partially** done: `pv_ceiling_kw`'s internal decay math now calls the same
`pv_smoothing::decayed_offset` function PV's own live/forecast methods use (closing the real
`PLAN_STEP_S`/`zone_a_step_s` divergence bug this phase set out to fix), but its two call
sites (`milp_planner/inputs.rs`, `simulator/forecast.rs::insert_pv_points`) still call it —
they were **not** replaced with calls into `PvInverter`'s own methods. Reason: `MilpInputs`
is built from `&SimSnapshot` (the flattened port-boundary data), not a live `PvInverter` — no
other asset kind resolves this from inside `milp_planner/inputs.rs` either; battery/EV/heater
instead pre-resolve their MILP-specific values earlier, in `plan_context.rs::build_asset_contexts`
(which does have live `&SimState` access), via `MilpParticipant::build_milp_context`, producing
an `AssetMilpContext` passed downstream. PV has no such participant today. Retiring these two
call sites for real needs that same threading (a `PvMilpContext`-shaped mechanism, or a live
reference threaded through `plan_context.rs`) — a second phase of work, not a quick follow-up
inside this session's remaining budget. Tracked as remaining work below (was tasks 4.2-4.4).

Section 5 (`capacity_headroom.rs`'s PV special-casing) is **not started** — blocked on the
same live-access gap, since the generic per-asset loop it would join also only sees
`&SimSnapshot`.

Net effect: the actual bug from the master plan's "why" section (two independently-computed
decay reference steps that only coincided by default-value accident) is fixed — one shared
formula now backs all three of PV's forecast surfaces. What remains is structural: giving
`pv_ceiling_kw`'s two callers and `capacity_headroom.rs` a live `PvInverter` to call into
instead of raw snapshot values, so `PvInverter`'s own methods become the *only* formula, not
just a *consistent* one. `pv_ceiling_kw`/`PvCeilingParams` therefore still exist in
`entities/solar.rs` — not deleted.

## 1. Verify the mechanism before coding

- [x] 1.1 Confirm `entities::solar::weather_pv_kw_for_slots`/`weather_pv_forecast_series`'s
      exact return types (`WeatherPvForecastSlot`, etc.) and what a `None`/empty series
      means at each call site — needed to design `PvInverter`'s new field's `Option` shape
      precisely.
- [x] 1.2 Read `milp_planner/inputs.rs`'s full `pv_ceiling_kw` call site (including the
      `pv_forecast_override`/deterministic-testing-pin precedence) side-by-side with
      `PvInverter::uncurtailed_power_kw`/`max_effort_setpoint`'s existing precedence, to
      confirm D4's equivalence claim or find the real difference before writing code.
      Finding: the real difference was the `PLAN_STEP_S`/`zone_a_step_s` divergence (now
      fixed via D7); no other precedence mismatch found.
- [x] 1.3 Grep every consumer of `build_forecast_frames`'s PV output and
      `resolve_weather_pv_kw_for_tick`'s `slots_kw` return value, beyond
      `capacity_headroom.rs`'s two call sites already found — confirmed nothing else depends
      on `pv_frames`.

## 2. Reparametrize the decaying offset to a single time constant `τ` (D7)

Done first, mechanically, fully verified on its own — before any new forecast logic (section
3) is layered on top, per design.md's own risk mitigation.

- [x] 2.1 Add `PvSmoothingState::decayed_offset_after(&self, elapsed_s: f64, tau_s: f64) ->
      f64` (`self.irradiance_offset * (-elapsed_s / tau_s).exp()`) with its own test-first
      coverage (matches today's `next_offset` output at `elapsed_s = dt_s` for the
      not-forced/not-already-decayed-to-zero case — confirms the reparametrization is
      numerically equivalent to today's `(1-alpha)^(dt_s/300)`, not just structurally similar).
- [x] 2.2 Rewrite `PvSmoothingState::next_offset`'s decay branch to call
      `decayed_offset_after` (passing `tau_s`) instead of computing
      `(1 - pv_alpha).powf(dt_s / PLAN_STEP_S)` directly — `PLAN_STEP_S`/`pv_alpha` params
      removed from this function.
- [x] 2.3 Renamed `pv_alpha` → `tau_s` (seconds) throughout, converting each default/test
      value via `τ = −300 / ln(1 − alpha)` so behavior is unchanged: `PvInverter.pv_alpha`,
      `TickOverrides.pv_alpha` → `pv_tau_s`, `PvCeilingParams.pv_alpha` → `tau_s`, the
      `pv_irradiance_alpha` inject/override key → `pv_tau_s`, `state_values`'s `"pv_alpha"`
      map key → `"tau_s"`. Every file found during design (`dispatcher.rs`, `plan_context.rs`,
      `pv_preview.rs`, `simulator_port.rs`, `mock_simulator_port.rs`, `milp_planner/tests/`)
      updated.
- [x] 2.4 Re-modeled the "Blend-back Speed" control (`PvInverter::control_schema`) to expose
      `τ` in seconds directly: key `pv_tau_s`, label "Blend-back Time", range 10s-3600s
      (a few seconds up to an hour, per design.md's own guidance), unit "s", default
      `≈2847.37s` preserving today's behavior exactly. Inversion (lower `τ` = faster decay)
      is documented on the struct literal; `ControlDescriptor` has no tooltip field to carry
      it into the UI itself.
- [x] 2.5 `VEN/ui/src/api/types.ts`: renamed `pv_irradiance_alpha` → `pv_tau_s`; updated
      `AssetRightSection.test.tsx`, `pv_irradiance_one_shot.test.ts`, `useSetSimInject.test.tsx`
      for the new name/range. Also updated `VEN/tests/fixtures/schema_snapshot.json` (golden
      file for `GET /sim/schema`), `docs/architecture/asset_simulation.md`,
      `docs/architecture/VEN_ARCHITECTURE.md`, `wiki/components/real-measurement-mqtt.md`, and
      `tests/features/steps/real_measurement_mqtt_steps.py` (a live BDD step that posts the
      inject field by name) — all found via a repo-wide grep for the old key after the
      backend rename, to make sure no consumer was left silently sending the retired name.
- [x] 2.6 Persisted-state backward compatibility: deserializing an old `sim_state.json` with
      the legacy `pv_alpha` key does not fail the whole load (`#[serde(default =
      "default_tau_s")]` on `PvInverter.tau_s`, matching `inverter_max_kw`'s existing
      precedent) — falls back to the τ-equivalent of the old default (`alpha=0.1`) rather
      than silently discarding it, covered by
      `pv_inverter_deserializes_from_json_missing_new_fields`.
- [x] 2.7 Full suite green (`wsl cargo test -j 2`; `cd VEN/ui && npm test`) before section 3.

## 3. `PvInverter` — new weather-forecast-aware methods (test-first)

- [x] 3.1 Added `weather_forecast: Option<Vec<WeatherPvForecastSlot>>` field to `PvInverter`,
      `#[serde(skip)]` — matching `weather_power_kw`'s existing doc pattern.
- [x] 3.2 Added `pv_weather_forecast: Option<Vec<WeatherPvForecastSlot>>` to `TickOverrides`
      (`assets/asset_trait.rs`); written onto the new `PvInverter` field in
      `apply_tick_overrides`.
- [x] 3.3 Populated the new `TickOverrides` field each tick in `simulator/mod.rs`/
      `tasks/sim_tick/arbiter_glue.rs`+`context.rs`, from the same already-resolved weather
      data `pv_weather_power_kw` is populated from.
- [x] 3.4 `max_effort_schedule`'s `t1` point equals `max_effort_setpoint(Export,
      Physical)`'s answer, by construction (`elapsed_s=0` case in
      `uncurtailed_power_kw_at`/`max_effort_schedule_inner`, now in `pv_schedule.rs`).
- [x] 3.5 A future point samples the injected weather series via `weather_pv_kw_for_slots`
      correctly, with `inverter_max_kw`/active `generation_limit_kw` clamps still applying.
- [x] 3.6 With no weather forecast injected, `max_effort_schedule` falls back to the
      sin-model plus `decayed_offset`-projected offset for `t > t1` — graceful degradation
      confirmed by test.
- [x] 3.7 Upgraded `Pv::forecast()` to sample `uncurtailed_power_kw_at` (weather/decay-aware)
      instead of the old sin-model-only `irradiance_at`, with a `curtailed` closure applying
      `generation_limit_kw` on top; tests updated for weather-present/absent cases.

## 4. Retire `pv_ceiling_kw`'s two real call sites — PARTIAL

`milp_planner/inputs.rs` and `simulator/forecast.rs::insert_pv_points`. Only the decay-math
fix (4.0 below, not in the original task list) landed; 4.1-4.4 remain, blocked as described
in Status above.

- [x] 4.0 (not originally scoped, done instead) `pv_ceiling_kw`'s own decay computation now
      calls `pv_smoothing::decayed_offset` directly (`entities/solar.rs`), the same function
      `PvInverter`'s live/forecast paths use — closes the actual `PLAN_STEP_S`/
      `zone_a_step_s` divergence bug, even though the call sites themselves still call
      `pv_ceiling_kw` rather than `PvInverter`.
- [ ] 4.1 Write the numeric-equivalence test first: same state/weather inputs, old
      `pv_ceiling_kw`-derived `p_pv_kw` vs. new `PvInverter`-derived value — **not done**,
      moot until 4.2 gives `PvInverter`-derived values a call site to compare against.
- [ ] 4.2 Replace `milp_planner/inputs.rs`'s `pv_ceiling_kw` call with a call into
      `PvInverter`'s own method — **blocked**: `build_milp_inputs` only receives
      `&SimSnapshot` (flattened), not a live `PvInverter`. Needs a `PvMilpContext`/
      `MilpParticipant` mechanism analogous to battery/EV/heater's, resolved earlier in
      `plan_context.rs::build_asset_contexts` (which has live `&SimState` access) and passed
      downstream — a right-sized follow-up change, not a one-line swap.
- [ ] 4.3 `simulator/forecast.rs::insert_pv_points`'s PV-specific code — **not done**, same
      blocker; still calls `pv_ceiling_kw` directly.
- [ ] 4.4 Delete `pv_ceiling_kw`/`PvCeilingParams` from `entities/solar.rs` — **not done**,
      both real call sites (4.2, 4.3) still exist.

## 5. Retire `pv_frames`'s PV special-casing (`capacity_headroom.rs`) — NOT STARTED

Blocked on the same live-`PvInverter`-access gap as section 4 — the generic per-asset
`asset_max_power_series`/`simulated_trajectory` loops this would join also only see
`&SimSnapshot`, not live assets.

- [ ] 5.1 Remove the `"pv" => continue` exclusion in the shared capacity-curve aggregator
      (`capacity_headroom.rs:98`).
- [ ] 5.2 Remove the equivalent exclusion in `compute_site_headroom_forecast`
      (`capacity_headroom.rs:352`).
- [ ] 5.3 Delete `pv_capacity_events` and its call site (`capacity_headroom.rs:115`).
- [ ] 5.4 Delete `pv_frames`'s now-unused plumbing if confirmed to have no other consumer.
- [ ] 5.5 Update `capacity_headroom.rs`'s module doc (the D1/PV-is-special paragraph).

## 6. Full verification

- [x] 6.1 `wsl cargo test -j 2` (full suite): 1270 passed, 0 failed.
- [x] 6.2 `cargo fmt --check` / `cargo clippy --all-targets --all-features -- -D warnings`:
      clean.
- [x] 6.3 `scripts/audit_file_sizes.py`: passes (`pv.rs` split into `pv.rs` + `pv_schedule.rs`
      to stay at/under the 500-line cap after the new logic landed).
- [x] 6.3b `cd VEN/ui && npm test`: 629 passed (53 files).
- [ ] 6.4 E2E/resilience on Node2 — **not run this session**; scoped for when section 4/5's
      MILP-input and capacity-headroom call sites actually change (the parts of this change
      that touch live planning input and the capacity-curve UI), not for the τ-rename/new
      trait-method work alone, which the Rust+UI unit suites already cover directly.
- [ ] 6.5 Manual check: weather-driven variation in the capacity-curve overlay — deferred
      with 6.4/section 5 (nothing changed there yet to re-verify).
- [ ] 6.6 Manual check: the re-modeled "Blend-back Time" slider (now in seconds, 10-3600s)
      — not manually verified in a running UI this session; unit-test coverage (6.3b)
      confirms the wiring, not the felt UX.

## 7. Docs and close-out

- [ ] 7.1 Update `docs/architecture/VEN_ARCHITECTURE.md` §3.0c (Unified Capacity/Headroom
      Engine) to remove the "PV is asset-kind-and-direction-special, by design" framing —
      **not done**; still accurate today, since section 5 (the part that would make it
      inaccurate) hasn't landed.
- [x] 7.2 Added a `docs/history/project_journal.md` entry: what changed, the τ
      reparametrization (including the `zone_a_step_s`/`PLAN_STEP_S` divergence bug this
      closes), and the explicitly-deferred section 4/5 scope.
- [x] 7.3 Updated `docs/plans/asset-competence-assurance-master-plan.md`'s Phase 1 status to
      **partial**, not complete — see that file for specifics.
- [ ] 7.4 Delete `openspec/changes/pv-competence-consolidation/` — **not done**, deliberately:
      the change is only partially implemented (sections 4-5, 6.4-6.6, 7.1 remain), so per
      this repo's own workflow rule ("if only part of a plan/change is done and tested,
      remove just that part and leave the rest in place") the directory stays until the
      remaining sections land, at which point this task list should be trimmed to just what's
      left rather than deleted wholesale.
