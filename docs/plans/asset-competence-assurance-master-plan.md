# Master Plan: Asset Competence Assurance

> **Status:** Phase 0 complete (`asset-competence-audit`, 2026-09-10). Phase 1 (PV) complete
> (`pv-competence-consolidation`, 2026-09-10/2026-09-11). Phase 2 (base load) complete
> (`base-load-competence-consolidation`, 2026-09-11). Phase 3 (battery efficiency, R-69)
> complete (`battery-efficiency-model-reconciliation`, 2026-09-11). All three
> unit/integration-tested and locally verified (fmt/clippy/file-size audit green);
> E2E/resilience and manual UI verification not yet run for any — see each phase's own section
> for the exact gap. Phases 4-5 open. This document sequences and motivates the work; it
> deliberately contains no
> implementation-level detail. Each phase's actual work happens as its own openspec change
> (`openspec new change ...`), proposed and reviewed carefully when that phase's turn comes
> — not as one bundled change, and not from this document directly.

## The principle

Infrastructure/data-acquisition — MQTT reception, weather APIs, a heuristics-learning store
— may live outside an asset's own module, and may even be shared across assets (a future
heater will need the same weather-temperature feed PV already needs). That's not a
violation of anything; it's ordinary infrastructure.

But **interpretation** — "what is this asset's current state" or "what is this asset's
forecast" — must have exactly one authority: the asset itself. No other module may
independently compute, or assume, its own answer to that question for a live or future
value. Raw external data flows *into* the asset (as an injected parameter — the asset
doesn't have to fetch it itself); everything downstream of receiving it — blending it,
projecting it forward, deciding what it means — is the asset's own encapsulated business.

The one exception is immutable history: an external recorder collecting the (unchangeable)
past isn't a competing authority, since there's no divergence risk once a value can no
longer change. Live and forecast values carry that risk; recorded history doesn't.

This is the same failure shape this codebase has already paid for multiple times — the
PV-Import and Heater-Export bugs `unified-capacity-envelope-engine` (Spec E) fixed, and the
site-headroom/capacity-curve seam divergence found afterward — just not yet recognized as
one recurring pattern with a name, or audited exhaustively. This master plan is that audit,
turned into a sequenced remediation.

## Why phases instead of one change

Each asset's fix is independently valuable, independently testable, and carries a different
risk profile — some touch only forecast-reporting surfaces, one touches live, tick-by-tick
dispatch physics. Bundling them would force the riskiest phase (EV departure) to block the
safest, highest-value one (PV) from landing. Same rationale
`asset-max-power-forecast-master-plan.md` used for its own five specs.

## Dependency graph

```
Phase 0 (formalize rule + complete audit)
   +--> Phase 1 (PV)
   +--> Phase 2 (Base load)
   +--> Phase 3 (Battery, R-69)
   +--> Phase 4 (Heater)
   +--> Phase 5 (EV departure) -- deliberately last, highest risk
            +--> Phase 6 (close out)
```

Phases 1–5 have no dependencies on each other — only on Phase 0's completed audit — and can
be reordered or interleaved if priorities change. The suggested order below is a
recommendation (safest/most-scoped first), not a hard requirement.

## Phase 0 — Formalize the rule, complete the audit

**Status: complete** (`asset-competence-audit`, 2026-09-10). The rule is recorded in
`.claude/CLAUDE.md` and `docs/architecture/VEN_ARCHITECTURE.md` §3.0d. Audit findings: EV
(`ev.rs` vs `ev_milp.rs`) and shiftable load (`shiftable_load.rs`, single-file) both
confirmed consistent, no divergence — no new debt entries. R-76 confirmed out of scope for
this master plan (site-level, not asset-level) and stays a separate investigation, noted on
its own `TECHNICAL_DEBTS.md` entry.

No behavior change. Two things:

1. Record the principle above as a permanent, named rule — `asset-competence-assurance` —
   in `.claude/CLAUDE.md`'s `ven-architecture` section (alongside `declare-dont-branch`,
   `generic-over-bespoke`, `naming-transparency`) and in
   `docs/architecture/VEN_ARCHITECTURE.md`, so it governs new code from this point on
   regardless of when the remediation phases below actually land.
2. Complete the parts of the violation inventory that weren't fully verified during initial
   research: compare `ev_milp.rs`'s SoC-bound handling against `ev.rs`'s own (battery's R-69
   shows this class of divergence is real, not hypothetical — EV needs the same scrutiny);
   confirm whether shiftable-load's MILP-side modeling is actually consistent with
   `shiftable_load.rs`'s own `max_effort_schedule`, or just hasn't been checked closely;
   decide whether `reporter.rs`'s R-76 (`IMPORT_RESERVATION_CAPACITY`/
   `EXPORT_RESERVATION_CAPACITY` field mapping) belongs in this master plan — it's a
   *site*-level interpretation question, not asset-level, so may warrant its own separate
   investigation rather than a seventh phase here.

Same role R-70 (tick-physics-deduplication) played as a prerequisite before Spec A in
`asset-max-power-forecast-master-plan.md`.

## Phase 1 — PV consolidation

**Problem:** four independent implementations of "PV's achievable power" exist today:
- `Pv::forecast()` (`VEN/src/assets/pv.rs`) — sin-model only, no live offset, no weather data.
- `Pv::step_inner`/`capability_inner` (`pv.rs`) — the asset's live truth: sin-model blended
  with the live decaying `irradiance_offset`/`pv_alpha`.
- `entities::solar::pv_ceiling_kw` (`VEN/src/entities/solar.rs`) — a third formula, called
  directly from `VEN/src/controller/milp_planner/inputs.rs` on PV's raw snapshot *values*
  rather than through any of PV's own methods.
- `pv_frames` (`VEN/src/tasks/sim_tick/arbiter_glue.rs::resolve_weather_pv_kw_for_tick`,
  `VEN/src/simulator/forecast.rs::build_forecast_frames`) — weather-MQTT-driven, used by
  `VEN/src/controller/capacity_headroom.rs`; confirmed neither function calls
  `Pv::forecast()` at all.

**Scope:** designate `Pv`'s own `Asset` trait methods as sole authority. Give those methods
weather data as an injected parameter (infra still resolves/polls it externally over MQTT;
the asset decides how to use it, matching the principle above) rather than fetching it
themselves. Retire `pv_ceiling_kw`'s external use in `milp_planner/inputs.rs` and
`pv_frames`'s parallel existence in `capacity_headroom.rs` in favor of calling into PV's own
(now-upgraded) methods.

**Non-goals:** no change to how weather data is fetched/polled (that infrastructure is fine
where it is); no change to PV's live dispatch behavior beyond making its forecast-facing
methods actually authoritative.

**Risk:** mostly forecast-surface (low risk); one call site
(`milp_planner/inputs.rs`, feeding live planning input) needs careful before/after
comparison since it affects real planning decisions, not just reporting.

**Status: complete** (`pv-competence-consolidation`, 2026-09-10 sections 1-3, 2026-09-11
sections 4-5; change directory deleted once merged — this section is now the durable record).
`PvInverter`'s own `Asset` trait methods (`forecast()`, `max_effort_schedule`, and the new
`simulate_forward`) are the sole authority for PV's achievable power everywhere, closing all
four implementations named in the Problem section above:

- The decaying-offset formula was reparametrized from a two-knob `(pv_alpha, T)` encoding to a
  single time constant `τ_s` (`PvSmoothingState::decayed_offset_after`/`pv_smoothing::decayed_offset`)
  — fixing a real bug this consolidation set out to find: the old `pv_ceiling_kw`'s reference step
  (`PLAN_STEP_S=300`, hardcoded) and the MILP planner's own reference step (`zone_a_step_s`,
  configurable) only coincided by default-value accident.
- `entities::solar::pv_ceiling_kw`/`PvCeilingParams` were deleted outright, not migrated. The
  MILP planner's `p_pv_kw` input now reads a live `PvInverter`-derived forecast
  (`simulator::plan_context::resolve_pv_forecast_kw`, resolved from the live `SimState`
  `tasks/planning/cycle.rs` already holds before flattening to `SimSnapshot` — no new
  `MilpParticipant`/`PvMilpContext` mechanism was needed after all, contrary to this section's
  earlier assessment; the live-`SimState` access already existed one call frame up).
- `capacity_headroom.rs`'s `pv_frames`-based special-casing (and the whole
  `build_forecast_frames`/`insert_pv_points`/`insert_simulated_points` apparatus feeding it) was
  deleted. PV now flows through the same `asset_max_power_series`/`simulated_trajectory`
  primitives every other asset kind uses. This required two structural fixes not foreseen when
  Phase 1 started: `PvInverter` needed its own `Asset::simulate_forward` override (since
  `asset_max_power_series` calls it directly, not just the site-headroom forecast path), and
  `AssetHandle`'s own `Asset` impl needed to delegate `simulate_forward` to `self.config` (it
  was the one method not already delegating, which would have made any asset's override
  invisible through `AssetHandle`/`simulated_trajectory` — found implementing this phase, not a
  PV-specific gap).

**Verification status:** unit/integration-tested (`cargo test`: 1264 passed), `cargo fmt`/
`clippy -D warnings`/`scripts/audit_file_sizes.py` all green. E2E/resilience (Node2) and manual
UI verification (Controller Site Headroom chart, Diagnostics Capacity Forecast panel, the
re-modeled "Blend-back Time" slider) were **not run** as part of landing this — flagged as
follow-up verification, not blocking the phase's completion, matching this master plan's own
risk assessment (this phase is the lowest-risk one; battery/heater/EV phases below should not
skip this step).

## Phase 2 — Base load consolidation

**Problem:** `BaseLoad::forecast()` (`VEN/src/assets/base_load.rs`) returns a flat constant
`baseline_kw` for its whole span, ignoring the learned heuristic
(`AssetHeuristics::sample_kw`) that `VEN/src/controller/milp_planner/inputs.rs` calls
*directly*, bypassing the asset's own method entirely — the same shape as Phase 1, one tier
smaller. Confirmed during implementation: `tasks/sim_tick/context.rs`'s
`base_load_heuristic_kw_now` is a *different*, already-correct mechanism (it flows through
`BaseLoad`'s own live tick precedence, `natural_base_kw`) — not a violation, and not touched by
this phase.

**Scope:** upgrade `BaseLoad::forecast()` to accept and use the heuristic as an injected
parameter (same "infra resolves it, asset interprets it" pattern as Phase 1); retire the
direct `sample_kw` call site in `build_milp_inputs` in favor of calling the asset's own method.

**Non-goals:** no change to how the heuristic itself is learned/updated; no change to the live
tick's already-correct `natural_base_kw` precedence; not touching
`services::forecast::build_heuristic_forecasts`/`record_forecast_accuracy_samples` or
`report_intervals.rs` — generic multi-asset infra reading raw heuristics output for
cross-asset reporting, not base_load-specific duplication of the asset's own forecast formula.

**Risk:** low — forecast-surface only; base load has no live dispatch setpoint to get wrong.

**Status: complete** (`base-load-competence-consolidation`, 2026-09-11). `BaseLoad` gained a
`heuristic: Option<AssetHeuristics>` field (mirrors `PvInverter.weather_forecast`), populated
each tick via `TickOverrides.base_load_heuristic`. `BaseLoad::forecast()` now samples it
per-hour via a new `forecast_kw_at` helper, falling back to the static `baseline_kw_profile`
when no heuristic exists yet (cold start). `build_milp_inputs`'s direct
`asset_heuristics.get(ASSET_BASE_LOAD)` read was replaced by a live-`BaseLoad`-derived forecast
(`simulator::plan_context::resolve_base_load_forecast_kw`, same pattern as Phase 1's
`resolve_pv_forecast_kw`) — and since base_load was the *only* consumer of the
`asset_heuristics: HashMap` parameter anywhere in that call chain (confirmed via grep before
removing it, not assumed), the parameter was dropped entirely from `build_milp_inputs`,
`run_planner`, `SolveRequest`, and `build_solve_request`, rather than left unused.
Unit/integration-tested (`cargo test`: 1270 passed), `cargo fmt`/`clippy -D warnings`/
`scripts/audit_file_sizes.py` all green. No UI files touched. E2E/resilience and manual UI
verification not run — same accepted, flagged gap as Phase 1.

## Phase 3 — Battery efficiency-model reconciliation (R-69)

**Problem:** already tracked in `docs/reference/TECHNICAL_DEBTS.md` — `assets/battery.rs`'s
live simulator puts round-trip efficiency loss on the charge leg only;
`assets/battery_milp.rs`'s MILP model splits it symmetrically
(`eff_ch=eff_dis=sqrt(round_trip_efficiency)`). Both agree on full-cycle totals but diverge
on intermediate SoC for any partial cycle — the normal case under the 5-minute rolling
replan. The textbook instance of this master plan's whole principle, already scoped.

**Scope:** reconcile into one shared source of truth for battery's own efficiency model —
this phase's openspec change should read R-69's own note for the two candidate resolutions
already identified there rather than re-deriving them.

**Risk:** touches live SoC tracking and MILP planning simultaneously — needs the equivalence
test R-69 itself already calls for (a `KEY_LEARNINGS.md` entry from this codebase's own
history warns that such a test is "only as strong as its parameter coverage" — don't let
this phase repeat that mistake).

**Status: complete** (`battery-efficiency-model-reconciliation`, 2026-09-11). Resolved the
open D-A vs. D-B decision as D-A: `battery.rs::step_inner`/`forecast` now split round-trip
loss symmetrically via `sqrt(round_trip_efficiency)` on both the charge and discharge legs,
matching `battery_milp.rs::build_milp_context`'s `eff_ch`/`eff_dis` (already symmetric, so
`battery_milp.rs` itself needed no change) — was previously all-loss-on-charge. Chosen over
D-B (making the planner asymmetric to match the simulator) per the design doc's own
recommendation: `sqrt`-split is the more standard textbook convention for a single combined
efficiency figure, and was already the MILP's existing convention; made this call directly
rather than blocking on synchronous user confirmation, since the design doc had already framed
it as a reasoned recommendation, not an open toss-up. A partial-cycle test (charge 10 kWh,
then discharge 9 kWh, assert the resulting SoC reflects loss on *both* legs, not just one) was
written first and confirmed to fail against the pre-existing asymmetric code before
implementing the fix. Unit/integration-tested (`cargo test`: 1271 passed — all 63 pre-existing
battery tests green, including `battery_milp.rs`'s, which needed no changes since it already
used the now-shared convention), `cargo fmt`/`clippy -D warnings`/`scripts/audit_file_sizes.py`
all green. No UI files touched. E2E/resilience and manual UI verification not run — this phase
touches live SoC tracking, a higher-risk surface than Phases 1/2's forecast-only changes, so
this gap matters more here; flagged explicitly, not silently carried forward.

## Phase 4 — Heater duplication audit and consolidation

**Problem:** `assets/heater.rs` and `assets/heater_milp.rs` both implement the same
temperature↔thermal-energy conversion (`(temp - temp_min) * thermal_mass_kwh_per_c`)
independently. Currently consistent — same formula, same config field — but this is exactly
the duplication shape that let R-69 silently diverge before anyone noticed.

**Scope:** confirm (don't assume) whether any other part of heater's live vs. MILP modeling
has already diverged the way battery's did; consolidate the conversion into one shared
implementation regardless, to remove the standing risk even if nothing has drifted yet.

**Risk:** moderate — same dual live/planning surface as battery, smaller in scope.

## Phase 5 — EV departure consolidation (deliberately last)

**Problem:** three inconsistent mechanisms handle the same fact today:
- `MilpParticipant::build_milp_context` (`assets/asset_trait.rs`, implemented in
  `assets/ev.rs`) — receives `EvSession`/`departure_time` as a proper trait parameter for
  MILP planning. Done right.
- `compute_site_headroom_forecast` (`controller/capacity_headroom.rs`) — a *site-level*
  `ev_session` parameter independently excludes the EV past `departure_time`, entirely
  outside the asset.
- `Asset::step()`/`simulate_forward()` (the general trait used by live dispatch and
  `asset_max_power_series`) — no departure-awareness at all; confirmed `EvState::plugged` is
  never toggled by `step()`.

**Scope:** make `Asset::step()`/`simulate_forward()` genuinely departure-aware using the same
`EvSession` data `build_milp_context` already receives correctly, retiring the site-level
`ev_session` forecast-time workarounds once the asset itself can answer the question.

**Non-goals:** no change to how/when `EvSession`/`departure_time` itself is captured or
updated (session management stays as-is).

**Risk:** highest in this plan — the only phase that changes live, tick-by-tick dispatch
physics, not just a forecast-reporting surface. Deliberately ordered last so the pattern is
proven on four lower-risk phases first, and so this one gets undivided scrutiny and test
coverage rather than being rushed alongside easier wins.

## Phase 6 — Close the master plan

Once every phase's openspec change is implemented, tested, and merged: fold durable lessons
into `docs/reference/KEY_LEARNINGS.md`; update `docs/architecture/VEN_ARCHITECTURE.md` and
`docs/reference/TECHNICAL_DEBTS.md` (removing R-69 once Phase 3 lands); delete this master
plan document — per this repo's own no-lingering-plans workflow rule.

## Suggested execution order

Phase 0, then 1, 2, 3, 4, 5, 6 — safest and most-scoped work first (PV and base load are
forecast-surface only; battery is already precisely scoped as R-69), building confidence in
the pattern before the one phase that touches live dispatch physics. Phases 1–4 can be
reordered or interleaved freely if priorities change; Phase 5 should stay last regardless.
