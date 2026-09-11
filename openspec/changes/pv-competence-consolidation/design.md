## Context

Confirmed by reading `VEN/src/assets/pv.rs` in full: `PvInverter::max_effort_setpoint`
(`Physical`, `Export`) is already correct and weather-aware for `t1` — it calls
`uncurtailed_power_kw` on `self.weather_power_kw`/`self.measured_power_kw`, the live
per-tick values. The gap is entirely about points *beyond* `t1`: PV has no
`max_effort_schedule` override, so it inherits the generic trait default, which has no way
to know future weather changes — this is exactly why `capacity_headroom.rs` special-cased
PV out of its generic per-asset loop and used the external `pv_frames` mechanism instead.

Also confirmed: `weather_power_kw` (today's single "now" value) already flows into
`PvInverter` every tick via the established `TickOverrides`/`TickOverridable` channel
(`simulator/mod.rs:350` → `TickOverrides.pv_weather_power_kw` →
`PvInverter::apply_tick_overrides`), populated from
`tasks/sim_tick/arbiter_glue.rs::resolve_weather_pv_kw_for_tick`. That function *already*
also resolves a multi-point forecast (`weather_pv_kw_slots: Option<Vec<f64>>`, aligned to
plan-slot boundaries) — currently used only to build the external `pv_frames`, not injected
into `PvInverter` at all.

## Goals / Non-Goals

**Goals:**
- `PvInverter::max_effort_schedule` becomes a real override, correct at `t1` (matching
  `max_effort_setpoint` exactly) and weather-aware for every later point.
- Retire `pv_ceiling_kw`'s external use in `milp_planner/inputs.rs` and `pv_frames`'s PV
  special-casing in `capacity_headroom.rs`.
- No change to PV's live dispatch (`step`/`capability`/`max_effort_setpoint` at `t1`).

**Non-Goals:**
- No change to how weather is fetched/polled (`weather.rs`'s MQTT client, `WeatherForecastPort`).
- No change to `Pv::forecast()`'s callers unless design work below finds a reason to.
- No change to any other asset kind's `TickOverrides` fields or trait methods.

## Decisions

**D1 — Thread the raw weather series, not a pre-sampled slot array, into `PvInverter`.**
`resolve_weather_pv_kw_for_tick`'s `slots_kw` is tied to the *caller's* slot boundaries
(the active plan's), which don't match what `max_effort_schedule`'s own 60s-step loop needs
and won't exist at all when there's no active plan (the exact case `compute_tick_forecasts`
already falls back to a bare 48h sweep for). Instead, thread the *reusable domain function*
`entities::solar::weather_pv_forecast_series(params, &forecast)`'s output — call it
`WeatherPvSeries` — into `PvInverter` once per tick, and let `max_effort_schedule` sample it
at whatever timestamps its own loop needs via the *same*, already-shared
`entities::solar::weather_pv_kw_for_slots` function `resolve_weather_pv_kw_for_tick` already
calls. This is the same "don't duplicate, call the one shared function" pattern
`natural_irradiance_at`'s existing doc comment already established for the sin-model case —
extended to the weather case.

**D2 — New `TickOverrides` field, not a new trait-method parameter.** Adding a weather-series
parameter to `Asset::max_effort_schedule`'s signature would force every other asset kind's
override (battery, EV, heater, shiftable load) to accept-and-ignore an irrelevant parameter —
the exact `declare-dont-branch`/generic-pollution shape this repo's own rules warn against.
Reusing `TickOverrides` (already PV-specific fields alongside `pv_irradiance`/
`pv_weather_power_kw`) keeps the trait signature untouched: add
`pv_weather_forecast: Option<WeatherPvSeries>` to `TickOverrides`, write it onto a new
`PvInverter.weather_forecast: Option<WeatherPvSeries>` field in `apply_tick_overrides`
(mirroring `pv_weather_power_kw`'s existing handling exactly), and have the new
`max_effort_schedule` override read `self.weather_forecast` directly — no external caller
needs to pass anything PV-specific through `capacity_headroom.rs` or
`milp_planner/inputs.rs` ever again.

**D3 — `max_effort_schedule`'s shape.** For `t1`: reuse `uncurtailed_power_kw` exactly as
`max_effort_setpoint` does today (guarantees the seam matches by construction, same
guarantee `asset_max_power`'s own doc comment already relies on for every other asset).
For each subsequent 60s-step point up to `t_end`: if `self.weather_forecast` is present,
sample it via `weather_pv_kw_for_slots` at that point's timestamp and apply the same
`inverter_max_kw`/active `generation_limit_kw` clamping `uncurtailed_power_kw` already does
(factor that clamping into a small shared helper both call, so there's still only one place
that logic lives); if absent (no weather feed configured/stale — the same condition
`resolve_weather_pv_kw_for_tick` already returns `None` for), fall back to the sin-model
(`irradiance_at`), matching today's `pv_ceiling_kw`/`Pv::forecast()` fallback behavior — a
graceful degradation, not a silent wrong answer.

**D4 — `pv_ceiling_kw`'s fate, and its actual call sites.** `pv_ceiling_kw` has **three**
call sites, not one — confirmed by grep, correcting the proposal's original count:
`milp_planner/inputs.rs:178-193` (MILP's `p_pv_kw` input), `simulator/forecast.rs:220-240`
(`insert_pv_points`, part of `build_forecast_frames` — i.e. `pv_frames`'s own construction
already goes through `pv_ceiling_kw` internally), and `simulator/forecast.rs`'s own test
module.

> **Status update (follow-up design pass, sections 1-3 already merged):** the paragraph
> below was written before sections 1-3 landed and assumed "call into `PvInverter`'s own
> method" was a simple swap once `max_effort_schedule` existed. Implementation found the real
> blocker: `milp_planner/inputs.rs::build_milp_inputs` receives `assets: &SimSnapshot` — the
> flattened port-boundary data — with no live `PvInverter` in scope to call
> `uncurtailed_power_kw_at` on. The mechanism below replaces the vague "directly, or via
> `asset_max_power_series`" with the actual fix.
>
> The caller one level up, `VEN/src/tasks/planning/cycle.rs::run_plan_cycle`, does have live
> access: `sim_snap: SimState` (the live clone) exists there, and is only flattened to
> `SimSnapshot` (`sim_snap.to_sim_snapshot()`) to build the value eventually passed into
> `build_milp_inputs`. `n_slots`/`cum_s`/`now` are already resolved before that flattening
> point too — the same place `build_asset_contexts` already runs against this same live
> `sim_snap`.
>
> **Mechanism:**
> 1. New helper in `simulator/plan_context.rs` (which already does the equivalent live
>    downcast for `apply_pending_pv_inject`): `resolve_pv_forecast_kw(sim_snap: &SimState,
>    n_slots: usize, cum_s: &[i64], now: DateTime<Utc>) -> Option<Vec<f64>>` — `None` when no
>    live `"pv"` asset exists (preserves today's fallback-to-`pv_cfg`-static-curve branch
>    untouched), `Some(vec)` of one `uncurtailed_power_kw_at(now + cum_s[i], cum_s[i] as f64)`
>    per slot otherwise.
> 2. Call it from `run_plan_cycle`, right alongside `build_asset_contexts` (same live
>    `sim_snap`, same `n_slots`/`cum_s`/`now` already in scope there), and thread the result
>    down through `services::planning::build_solve_request` → `build_milp_inputs` as a new
>    parameter (`pv_live_forecast_kw: Option<&[f64]>`), replacing the per-slot `pv_ceiling_kw`
>    reconstruction in `inputs.rs`'s loop. Precedence collapses to: `pv_forecast_override`
>    (unchanged, always wins) → `pv_live_forecast_kw[i]` (replaces the live-snapshot
>    `pv_ceiling_kw` branch — already weather-aware internally via
>    `PvInverter.weather_forecast`, so no separate `weather_pv_kw` fold-in is needed for this
>    branch) → the existing static-curve fallback when `None`.
> 3. **Fix a found correctness discrepancy before/while swapping the call site**:
>    `pv_ceiling_kw` clamps the irradiance *fraction* to `[0, 1]` before scaling by
>    `rated_kw` (`(natural + decayed_offset).clamp(0.0, 1.0) * rated_kw`), matching
>    `PvPowerInputs.irradiance`'s doc'd contract that the live `step_inner` path already
>    honors. `uncurtailed_power_kw_at`'s no-weather fallback branch (`pv_schedule.rs`, added
>    in sections 1-3) does not — it scales `natural` and `decayed_offset` separately by
>    `rated_kw` and only floors at `0.0`, with no upper clamp before the `inverter_max_kw`
>    clip. These diverge whenever `decayed_offset` pushes the fraction above `1.0` and
>    `inverter_max_kw > rated_kw` (an uncommon config, not an impossible one). Task 4.1's
>    numeric-equivalence test would very likely surface this regardless — fix it as part of
>    this same task rather than let the test find it cold.
> 4. **Open question, not resolved here**: once `pv_live_forecast_kw` covers the "has live
>    PV" case, is `weather_pv_kw`'s separate threading into `build_milp_inputs` still needed
>    at all, or only for the "no live PV asset" fallback branch? Decide during implementation,
>    not here — collapsing it is a nice simplification but not required for this task's own
>    correctness.
>
> `insert_pv_points`'s PV-specific code is deleted outright once section 5's `pv_frames`
> retirement lands (D6, below) — unchanged by this update. After both real call sites are
> gone, compare `pv_ceiling_kw`'s remaining logic against `PvInverter`'s own precedence;
> once equivalent (with the `[0,1]`-clamp fix above, it should be), delete
> `pv_ceiling_kw`/`PvCeilingParams` from `entities/solar.rs` entirely — that file keeps only
> `natural_irradiance_at`/`weather_pv_kw_for_slots`/`weather_pv_forecast_series`,
> genuinely-shared low-level math, not an asset-shaped ceiling function.

**D7 — Replace the `(pv_alpha, T)` two-knob decay encoding with a single time constant
`τ` (seconds), fixing the root cause, not just today's symptom.** `(1 − alpha)^(t/T)` and
`A·e^(−t/τ)` are the same curve family — they're related by `τ = −T / ln(1 − alpha)`
(today's default `alpha=0.1, T=300` → `τ ≈ 2848.5s`). The two-knob form is why the bug
found during design was possible at all: `T` is a *second*, easy-to-duplicate parameter that
`alpha` alone gives no signal is wrong. Storing `τ` directly removes the redundant degree of
freedom structurally, not just at today's two call sites.

- Replace `pv_alpha: f64` with `tau_s: f64` everywhere it appears: `PvInverter.pv_alpha`,
  `PvSmoothingState::update`/`next_offset` (formula becomes
  `self.irradiance_offset * (-elapsed_s / tau_s).exp()`, both for the live per-tick case
  --`elapsed_s = dt_s`-- and the new forward-projection case), `TickOverrides.pv_alpha` →
  `pv_tau_s`, the `pv_irradiance_alpha` inject/override key, `PvCeilingParams.pv_alpha`
  (moot once D4 deletes the struct), and `state_values`' `"pv_alpha"` map key (→ `"tau_s"` —
  check `VEN/ui` for any reader of this exact key, since it's exposed through asset
  state/history, not just internal).
- Add `PvSmoothingState::decayed_offset_after(&self, elapsed_s: f64, tau_s: f64) -> f64` —
  the one function both the live per-tick update and `PvInverter::max_effort_schedule`'s
  forward projection call. `zone_a_step_s` stops being read for this purpose at all (its
  *other*, unrelated use in `services/planning/mod.rs` for switch-cost weighting is
  untouched — confirmed a distinct, legitimate use, not part of this consolidation).
- **Slider re-modeling** (per explicit instruction): the "Blend-back Speed" control
  (`control_schema`, key `pv_irradiance_alpha` → rename to match, e.g. `pv_tau_s`) changes
  from an opaque `alpha` fraction (0.01–1.0, higher = faster) to `τ` in seconds (lower =
  faster — note the inversion, a real UX-facing change worth calling out, not just a
  relabeling). Default value preserved at `≈2848.5s` so existing behavior is unchanged
  out of the box; min/max chosen during implementation to cover a sensible real-world
  range (e.g. a few seconds for "snap back almost immediately" to on the order of an hour
  for "barely fades" — exact bounds decided in task 2.x against what actually reads
  sensibly on the slider, not fixed here).
- **Persisted-state backward compatibility**: `sim_state.json` may hold an old `pv_alpha`
  value under the current key. Follow this file's own established pattern
  (`inverter_max_kw`'s `#[serde(default)]` precedent, same file) — deserializing a payload
  with the old key/shape must not fail the whole state load. Decide during implementation
  whether to accept-and-convert an old `pv_alpha` value (compute `τ` from it, so a resumed
  session keeps its exact prior decay behavior) or simply default to the new field's default
  — converting is more faithful and not materially harder, so prefer it unless it proves
  awkward.

**D5 — `Pv::forecast()` — has a real live caller, must be reconciled, not left stale.**
Confirmed: `VEN/src/routes/assets.rs:44` calls `Asset::forecast()` generically per asset
(a real route, presumably the per-asset forecast diagnostic surface, not test-only). Leaving
`Pv::forecast()` on the sin-model while `max_effort_schedule` becomes weather-aware would
create a *new* internal inconsistency between two of PV's own trait methods — exactly what
this phase exists to prevent. Upgrade `forecast()` to sample the same
`self.weather_forecast`-via-`weather_pv_kw_for_slots` path `max_effort_schedule` uses
(falling back to `irradiance_at` under the same absent/stale conditions), so all of PV's own
methods agree with each other, not just with external callers.

**D6 — Removing `pv_frames`'s PV special-casing.**

> **Status update (follow-up design pass): this decision's original premise was wrong.** It
> assumed `PvInverter::max_effort_schedule` alone would unblock *both*
> `capacity_headroom.rs` call sites once it existed. Re-reading both functions in detail
> found they call two genuinely different mechanisms, so this splits into two independent
> sub-tasks, 5a and 5b, with different blockers.

**5a — `compute_site_capacity_curve`'s `"pv" => continue"`.** This function already takes
`sim: &SimState` (live), and for every non-PV asset calls `assets::asset_max_power_series(cfg,
&entry.state, now, t2_max, direction, LimitTier::Physical)`, which calls
`cfg.max_effort_schedule(...)` directly — exactly the method `PvInverter` now implements
(sections 1-3, weather-aware, does not go through `step()`). **No remaining blocker.** Remove
the exclusion and the `pv_capacity_events` call (and its `pv_frames` parameter, if nothing
else in the function still needs it); verify PV now produces a correct, weather-aware curve.

**5b — `compute_site_headroom_forecast`'s `"pv" => continue"`.** This function also takes
`sim: &SimState` (live), but calls a *different* path: `simulator::forecast::
simulated_trajectory(entry, cfg, future_slots)`, which drives `Asset::simulate_forward`
using the plan's own committed `planned_kw_by_asset` schedule. `simulate_forward`'s default
body calls `self.step(&state, setpoint_kw, dt)` in a loop, and **`Asset::step`'s signature
carries no timestamp at all** (`fn step(&self, state: &AssetState, setpoint_kw: f64, dt:
Duration) -> (AssetState, f64)`, `asset_trait.rs:53`). This is 5b's actual blocker, and it is
not a live-access problem: even with a live `PvInverter` in hand, `step()` has no way to know
which future calendar instant a given step represents, so PV's sun-position-dependent
physics cannot vary across a simulated trajectory through this path — exactly the
"ceiling flattens to a constant" regression this decision already worried about, just for a
different structural reason than originally stated (`max_effort_schedule` doesn't help here;
this path never calls it).

Fix: give `PvInverter` its own `simulate_forward` override — the same "override where PV's
physics genuinely differs" pattern already used for `max_effort_schedule` — building each
`TrajectoryPoint` directly from the real timestamps `future_slots` already carries
(`PlanTimeSlot.start`), via `uncurtailed_power_kw_at(slot.start, elapsed_s)`, bypassing
`step()`'s dt-only interface for PV specifically. This does **not** require changing
`Asset::step`'s signature for any other asset kind (battery/EV/heater's `step()` genuinely is
setpoint+duration-driven, no timestamp needed) — a scoped, PV-only override.

5a and 5b are separable tasks, not one atomic step — 5a has no remaining blocker, 5b does; do
not gate 5a on 5b's extra work. Once both land, confirm no other consumer of
`pv_frames`/`build_forecast_frames`'s PV-specific output exists before deleting
`resolve_weather_pv_kw_for_tick`'s `slots_kw` return value or `build_forecast_frames`'s PV
handling — if `pv_frames` is used for anything beyond feeding `pv_capacity_events` and
`compute_site_headroom_forecast`'s PV branch, that other use needs its own migration path,
not silent deletion.

## Risks / Trade-offs

- [Risk] `milp_planner/inputs.rs`'s `p_pv_kw` feeds live planning decisions — a subtle
  behavior change here isn't just a forecast-reporting cosmetic fix. → Mitigation: write the
  numeric-equivalence test called for in the proposal *first* (same weather/state inputs,
  compare old `pv_ceiling_kw`-derived `p_pv_kw` against the new `PvInverter`-derived value,
  across a representative sweep of irradiance/limit/override combinations), confirm it fails
  before the change and passes after, not just "tests still pass."
- [Risk] `weather_pv_kw_for_slots`'s existing slot-alignment assumptions (built for
  plan-slot boundaries) might not generalize cleanly to `max_effort_schedule`'s arbitrary
  60s-step timestamps. → **Resolved during design**: read the actual implementation
  (`entities/solar.rs:265-278`) — `slot_starts` is just a parameter name; the function is a
  generic linear interpolator over `&[DateTime<Utc>]` (`interpolate_ac_kw`, sorts
  defensively, clamps outside the series' range) with no structural dependency on plan
  slots. D1 is directly implementable as designed, no revisiting needed.
- [Risk] Deleting `pv_frames` plumbing could silently break a consumer this design didn't
  find. → Mitigation: D6's explicit grep-before-delete step; keep the deletion as its own
  task, ordered after the new PV path is verified working, not combined into one commit.
- [Risk] D7's `pv_alpha` → `tau_s` rename touches 16 Rust files and 4 UI files (found via
  grep: `VEN/ui/src/api/types.ts`, `AssetRightSection.test.tsx`,
  `pv_irradiance_one_shot.test.ts`, `useSetSimInject.test.tsx`) — materially larger surface
  than the rest of this phase. → Mitigation: do the rename as its own ordered task group
  (section 2a below), full-suite-verified before touching `max_effort_schedule` itself, so a
  mechanical rename mistake doesn't get tangled up with the actual new-behavior logic.
- [Risk] The slider inversion (higher `alpha` = faster decay, but lower `τ` = faster decay)
  is a real behavior-facing change for anyone with muscle memory on the old control, not
  just a relabel. → Mitigation: note it explicitly in the control's own label/tooltip if the
  UI supports one, and in the journal entry — not hidden as an implementation detail.

## Open Questions

None blocking — D4/D5's "check before assuming" items are implementation-phase
verification steps. D7's exact slider min/max bounds are an implementation-time judgment
call (task 2a.x), not something requiring the user's sign-off before tasks.md, per their own
instruction to re-model the range at implementation.
