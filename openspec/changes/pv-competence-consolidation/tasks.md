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
they were **not** replaced with calls into `PvInverter`'s own methods. Reason: `build_milp_inputs`
is built from `&SimSnapshot` (the flattened port-boundary data), not a live `PvInverter`.
Section 4's checklist below now has a concrete design for closing this (see design.md D4's
"Status update"), found in a dedicated follow-up design pass — not yet implemented.

Section 5 (`capacity_headroom.rs`'s PV special-casing) is **not started**. A follow-up design
pass (design.md D6's "Status update") found the original "blocked on the same live-access
gap" framing above was **only half right** — `capacity_headroom.rs`'s two functions already
take live `sim: &SimState`, not a flattened snapshot. Re-reading both in detail found they
split into two independently-blocked halves: 5a (`compute_site_capacity_curve`) has **no
remaining blocker** at all — it already calls `max_effort_schedule` via
`asset_max_power_series`, which PV now implements; 5b (`compute_site_headroom_forecast`)
calls `simulate_forward`/`step()` instead, whose signature carries no timestamp, so PV's
time-of-day physics can't flow through it regardless of live access — needs a new
`PvInverter::simulate_forward` override, not more threading. See the checklist below.

Net effect: the actual bug from the master plan's "why" section (two independently-computed
decay reference steps that only coincided by default-value accident) is fixed — one shared
formula now backs all three of PV's forecast surfaces. What remains is structural: giving
`pv_ceiling_kw`'s two callers a live `PvInverter`-derived forecast to read instead of raw
snapshot values (section 4), and giving `PvInverter` a `simulate_forward` override so its
time-of-day physics survives the plan-driven trajectory walk `compute_site_headroom_forecast`
uses (section 5b) — so `PvInverter`'s own methods become the *only* formula everywhere, not
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
fix (4.0 below, not in the original task list) landed; 4.0a-4.7 remain, with a concrete
design now in place (see below and design.md D4's "Status update").

- [x] 4.0 (not originally scoped, done instead) `pv_ceiling_kw`'s own decay computation now
      calls `pv_smoothing::decayed_offset` directly (`entities/solar.rs`), the same function
      `PvInverter`'s live/forecast paths use — closes the actual `PLAN_STEP_S`/
      `zone_a_step_s` divergence bug, even though the call sites themselves still call
      `pv_ceiling_kw` rather than `PvInverter`.

Design for the mechanism below: design.md D4's "Status update" (found in a dedicated
follow-up design pass, not yet implemented).

- [ ] 4.0a Fix the found `[0,1]`-irradiance-fraction-clamp discrepancy: `pv_ceiling_kw`
      clamps `(natural + decayed_offset)` to `[0,1]` before scaling by `rated_kw`;
      `uncurtailed_power_kw_at`'s no-weather branch (`pv_schedule.rs`) doesn't — it scales
      `natural`/`decayed_offset` separately and only floors at `0.0`. Fix
      `uncurtailed_power_kw_at` to match `pv_ceiling_kw`'s (and `step_inner`'s) clamp-before-
      scale contract, with its own test pinning the divergent case (`inverter_max_kw >
      rated_kw`, offset pushing the fraction above 1.0).
- [ ] 4.1 Write the numeric-equivalence test: same state/weather inputs, today's
      `pv_ceiling_kw`-derived `p_pv_kw` vs. `PvInverter::uncurtailed_power_kw_at`-derived
      value, across curtailed/uncurtailed/override/no-weather cases — should pass once 4.0a
      lands (that's the discrepancy this test exists to catch).
- [ ] 4.2 Add `resolve_pv_forecast_kw(sim_snap: &SimState, n_slots: usize, cum_s: &[i64], now:
      DateTime<Utc>) -> Option<Vec<f64>>` to `simulator/plan_context.rs` (same live-downcast
      pattern as `apply_pending_pv_inject` in the same file) — `None` when no live `"pv"`
      asset exists, `Some(vec)` of one `uncurtailed_power_kw_at(now + cum_s[i], cum_s[i] as
      f64)` per slot otherwise. Own unit tests: live-PV-present, no-PV-present.
- [ ] 4.3 Call `resolve_pv_forecast_kw` from `tasks/planning/cycle.rs::run_plan_cycle`
      alongside `build_asset_contexts` (same live `sim_snap`/`n_slots`/`cum_s`/`now` already
      in scope there); thread the result through `services::planning::build_solve_request` →
      `build_milp_inputs` as a new `pv_live_forecast_kw: Option<&[f64]>` parameter.
- [ ] 4.4 In `build_milp_inputs`, replace the per-slot `pv_ceiling_kw` reconstruction with the
      collapsed precedence: `pv_forecast_override` → `pv_live_forecast_kw[i]` →
      existing static-curve (`pv_cfg`) fallback when `None`.
- [ ] 4.5 `simulator/forecast.rs::insert_pv_points`'s PV-specific code — delete once section 5
      lands (depends on 5's `pv_frames` retirement, per D4/D6 — do not migrate it).
- [ ] 4.6 Delete `pv_ceiling_kw`/`PvCeilingParams` from `entities/solar.rs` once both real
      call sites (4.4, 4.5) are gone.
- [ ] 4.7 Open question to resolve during this work, not before: does `weather_pv_kw`'s
      separate threading into `build_milp_inputs` become redundant once `pv_live_forecast_kw`
      covers the "has live PV" case, or is it still needed for the "no live PV asset"
      fallback? Decide against the actual code, not abstractly.

## 5. Retire `pv_frames`'s PV special-casing (`capacity_headroom.rs`) — NOT STARTED

Splits into two independently-blocked halves — see design.md D6's "Status update" (found in
a dedicated follow-up design pass; the original "same blocker as section 4" framing was only
half right, corrected here).

**5a — `compute_site_capacity_curve` (no remaining blocker):**

- [ ] 5a.1 Remove the `"pv" => continue` exclusion in the shared capacity-curve aggregator
      (`capacity_headroom.rs:98`) — `asset_max_power_series` already calls
      `max_effort_schedule`, which PV now implements (sections 1-3).
- [ ] 5a.2 Delete `pv_capacity_events` and its call site (`capacity_headroom.rs:115`); remove
      its `pv_frames` parameter from `compute_site_capacity_curve`'s signature if nothing else
      in the function still needs it.
- [ ] 5a.3 Verify: PV's capacity curve now shows genuine weather/time-of-day variation, not a
      flat repeat — test with a solar-noon vs. a night `now`.

**5b — `compute_site_headroom_forecast` (blocked on `Asset::step`'s missing timestamp, not
live access):**

- [ ] 5b.1 Add a `PvInverter::simulate_forward` override (`Asset` trait) — bypasses the
      generic `step()`-based default entirely; builds each `TrajectoryPoint` directly from the
      setpoints' own timestamps (which for this call path are `future_slots`' real
      `PlanTimeSlot.start` values) via `uncurtailed_power_kw_at(ts, elapsed_s)`. Test-first: a
      trajectory point at a future slot must reflect that slot's own time-of-day irradiance
      (e.g. solar-noon slot vs. night slot produce different ceilings), not a flat repeat of
      `now`'s.
- [ ] 5b.2 Remove the `"pv" => continue` exclusion in `compute_site_headroom_forecast`
      (`capacity_headroom.rs:352`) — only after 5b.1 lands; removing it before would silently
      regress to the flat-ceiling bug this section exists to fix.
- [ ] 5b.3 Verify: the Site Headroom forecast's PV contribution varies across future slots.

**Once both 5a and 5b land:**

- [ ] 5.4 Delete `pv_frames`'s now-unused plumbing (`resolve_weather_pv_kw_for_tick`'s
      `slots_kw` output, `build_forecast_frames`'s PV handling) — only if confirmed to have no
      other consumer beyond `pv_capacity_events`/`compute_site_headroom_forecast`'s PV branch.
- [ ] 5.5 Update `capacity_headroom.rs`'s module doc (the "PV is asset-kind-and-direction-
      special, by design" paragraph) to reflect that PV is no longer special-cased.

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
