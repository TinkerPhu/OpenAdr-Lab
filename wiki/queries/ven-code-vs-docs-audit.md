---
title: VEN Code vs Documentation Audit
type: query
created: 2026-07-05
updated: 2026-07-06
synced_commit: ae4a1ed
sources: [VEN/src/, docs/architecture/VEN_ARCHITECTURE.md, .claude/CLAUDE.md]
tags: [audit, drift, refactoring, ven]
---

# VEN Code vs Documentation Audit

Full read of `VEN/src/` (90 files, ~25 k lines) at e138861, checked against
`docs/architecture/VEN_ARCHITECTURE.md`, `.claude/CLAUDE.md` §ven-architecture, and the
wiki's own component pages. Question: does the code match the documentation, and where
should refactoring effort go?

## What matches well

- **Ring discipline holds.** All four grep invariants from `.claude/CLAUDE.md` pass: no
  `use crate::profile` in `entities/`/`controller/`/`routes/`, no `use crate::assets::`
  in `milp_planner/` production code or `entities/`, `serde_json::Value` internal to
  `vtn.rs` (the one exception — raw report pass-through for `GET /reports` — is a
  documented design choice, `VEN/src/state.rs` `PollingState.reports`).
- **Two-phase MILP** works as [[milp-planner]] describes: lexicographic Phase 2 with
  warm start, epsilon cap, Phase 1 fallback (`solver_phase2.rs`), acceptance gate with
  decay, expiry force-adopt and heater-switch surcharge (`services/planning.rs`).
- **Task supervision, profile startup validation, `align_to_step`, the three "nows"**
  ([[three-tier-plan-grid]]) all match their pages exactly (`tasks/mod.rs`,
  `tasks/planning.rs`).
- **`TimeSeries` exists and is used.** `common/mod.rs` implements Step/Linear
  interpolation, `time_weighted_mean`, `resample_uniform`, `resample_to_grid` — the
  abstraction `VEN_ARCHITECTURE.md` §5.3 still calls "target architecture". Tariff
  lookups (`TariffTimeSeries`, Step/LOCF), obligation reports (multi-interval,
  time-weighted mean per bucket, `reporter.rs`), and timeline resampling all sit on it.

## Confirmed drift (docs say X, code does Y)

1. ~~**`SolverPort` does not exist.**~~ **RESOLVED** — `controller/solver_port.rs` now
   defines the trait; `MilpSolver` (`milp_planner/mod.rs`) implements it, and
   `services::PlanningService::solve_plan` is the only caller. See
   [[ven-hexagonal-architecture]], [[milp-planner]].
2. **`StaleRatePolicy` is dead vocabulary.** Quarantined (not deleted) into
   `entities/design_vocabulary.rs`; `PlanTimeSlot.rate_estimated` is still hardcoded
   `false` in `results.rs`. Actual stale-rate behaviour: Step/LOCF carries the last known
   tariff forward indefinitely, and hardcoded defaults (0.25 €/kWh import, 0.08 export,
   300 g/kWh) fill slots before the first sample or when no data exists
   (`milp_planner/inputs.rs:77-90`). `docs/BACKLOG.md` BL-07 tracks wiring it as a real
   feature. See [[tariffs-and-capacity]].
3. **The event-translation table is one-third aspirational.** Parsed inbound:
   `PRICE`, `EXPORT_PRICE`, `GHG` (with looping-event support for `P9999Y` daily prices),
   `IMPORT_/EXPORT_CAPACITY_LIMIT`, `IMPORT_CAPACITY_SUBSCRIPTION`,
   `IMPORT_CAPACITY_RESERVATION` (`controller/openadr_interface.rs`). Not handled
   anywhere: `ALERT_*`, `DISPATCH_SETPOINT`, `CHARGE_STATE_SETPOINT`,
   `EXPORT_CAPACITY_SUBSCRIPTION`, `EXPORT_CAPACITY_RESERVATION` (inbound) —
   `OadrCapacityState` has no export-subscription field at all. `PlanTrigger::Alert` and
   `::CapacityChange` are never sent; every poll-detected change fires
   `PlanTrigger::RateChange` (`tasks/poll_events.rs:173`). See [[openadr-interface]].
4. **No `AssetInterface`, no `SimulatedAsset`/`MeasuredAsset`.**
   `VEN_ARCHITECTURE.md` §3.0's trait (`current()/forecast()/past()`) was never built.
   The real abstraction is the `Asset` trait (`assets/mod.rs:545` — `step`, `capability`,
   `simulate_forward`, `capability_trajectory`) plus enum-dispatch `AssetConfig` and
   `AssetHandle`; history lives in per-asset ring buffers (3600 points ≈ 1 h at 1 s).
   See [[asset-layer]].
5. **Dispatcher ≠ doc description.** `controller/dispatcher.rs` is a pure-function
   module (`build_setpoints` + `apply_surplus_ev_overlay`) driven by the 1 s
   `tasks/sim_tick` loop. There is no "auto-follow NetDeviation distribution"; the
   battery deviation correction `apply_battery_correction_overlay` exists but is
   `#[allow(dead_code)]` — kept intentionally, not wired into `build_setpoints`
   (`docs/BACKLOG.md` BL-22 tracks the wiring decision). Ledger accounting is
   `monitor::record_tick`, called from `sim_tick/publish.rs`, not the dispatcher. See
   [[dispatcher]].
6. ~~**Plan-cycle status reports are dead on arrival.**~~ **RESOLVED** — the dead
   `build_status_report`/`TELEMETRY_STATUS`-on-`PlanCycle` call path was deleted rather
   than given a real program ID (it never had one to report against). `/trace/events`
   and `/plan/events` (SSE) remain the observability paths for plan cycles. See
   [[openadr-interface]].
7. **`DomainError` is three-fifths unused.** `PlanInfeasible`, `VtnUnreachable`,
   `ProfileInvalid` are never constructed in production code; solver failure produces a
   fallback plan with a Critical `PlanWarning` (`results.rs::fallback_plan`), and profile
   validation returns `Vec<String>`. Only `SessionConflict` and `NotFound` are live
   (`services/hems.rs`). Kept intentionally (not trimmed) — `docs/BACKLOG.md` BL-25
   tracks wiring each at a real boundary. See [[reliability-and-config]].
8. **`VEN_ARCHITECTURE.md` §4 (API) and §5.2 (alignment audit) are stale.** Routes live
   in `routes/mod.rs`, not `main.rs`. `/sim/override` (decision D-06) no longer exists —
   replaced by `POST /sim/inject` with four behaviour classes (one-shot, frozen+EMA,
   frozen+snap, planning-only). `/trace` split into `/trace/events` (ControllerEvent
   ring, capacity 500 — not 1000, and not setpoints) and `/trace/history`. Endpoints the
   doc doesn't know: `/timeline/all`, `/timeline/:asset_id`, `/forecast/:id`,
   `/history/:id`, `/capability/:id`, `/plan/objective`, `/plan/events` (SSE),
   `/plan/trigger`, `/ev-session`, `/ev-settings`, `/heater-target`, `/shiftable-loads`,
   `/baseline-override`, `/sim/config/battery`. §5.2's "planner samples tariff with
   exact-interval containment at planner.rs:540" and "reporter emits latest snapshot
   only" both describe code that no longer exists.
9. **Two-speed loop numbers.** Docs and [[hems-planning]] said "20 s periodic" replan;
   `PlannerParams::default().replan_interval_s` is 300 s and profile-configurable. Poll
   intervals are env-configurable (`POLL_EVENTS_SECS`, default 30 s), not fixed (D-07).
10. ~~**`services/test_support` is `#[cfg(test)]`-gated** (`services/mod.rs:2`), while
    `.claude/CLAUDE.md` §testing says the shared mocks are "not cfg(test)".~~
    **RESOLVED** — `.claude/CLAUDE.md` corrected to state they are `#[cfg(test)]`-gated.
11. ~~**Report obligations are one-shot.**~~ **RESOLVED** — `extract_report_obligations`'s
    dedup was correct all along; the actual fix was replacing permanent
    `mark_obligation_fulfilled` with `state.rs::rearm_obligation` (advances `due_at` by
    `interval_duration_s` instead of disabling the obligation forever) plus
    `retire_obligations_not_in` for when the source event expires. See
    [[openadr-interface]].

## Refactoring candidates (ranked)

1. **File-size rule vs reality — open, and growing.** `.claude/CLAUDE.md` demands
   ≤ 500 lines per `VEN/src/` file and < 200 for `tasks/`. The violation count keeps
   growing as normal feature work lands — `routes/timeline.rs` (now ~772 lines) and
   `controller/timeline.rs` (~1179) both grew further past the cap during the 2026-07
   timeline-forecast fix, and `tasks/planning.rs` is now 398 (vs 200), up from 363.
   Current per-file register: `docs/reference/TECHNICAL_DEBTS.md`. Still open — R4 in
   `docs/plans/review_items_resolution_strategy.md` is the tracked decision item.
2. ~~**Dead-code inventory** — delete or wire: `StaleRatePolicy`,
   `apply_battery_correction_overlay`, the plan-cycle status-report call,
   `HvacService`, three `DomainError` variants, and the large unused §-numbered
   vocabulary block in `entities/asset.rs`~~ **RESOLVED** — quarantined, not deleted.
   The vocabulary block (`AssetProfile`, `AssetHeuristics`, `AssetForecast`,
   `AssetLedger` — the live ledger is `state::AssetLedgerEntry` — `PenaltyRule`,
   and more) moved verbatim into `entities/design_vocabulary.rs` with a
   not-current-behaviour banner; `ComfortRate` stayed in `entities/asset.rs` (it's
   actually live, not dead — a correction found during this resolution).
   `apply_battery_correction_overlay`/`HvacService`/`OadrEventCache` family/
   `DomainError` variants stayed in place with corrected comments. Every item now has a
   `docs/BACKLOG.md` entry (BL-14 through BL-29).
3. **Slot-start tariff sampling.** `build_milp_inputs` evaluates tariffs at each slot's
   start (`interpolate_at(slot_t)`); `TimeSeries::time_weighted_mean` already exists and
   would price boundary-straddling slots correctly — the exact fix
   `VEN_ARCHITECTURE.md` §5.3 sketches, one call away.
4. **Application ring imports axum.** `services/hems.rs:52` implements
   `From<DomainError> for (StatusCode, Json)` — HTTP mapping belongs in `routes/`.
5. **Envelope magic numbers.** `milp_planner/envelopes.rs` hardcodes
   `max_acceptable_rate: 0.35`, `min_acceptable_rate: 0.05`,
   `budget_remaining_eur: 1.0e9` in every `FlexibilityEnvelope`.
6. ~~**One-shot obligations.**~~ **RESOLVED** — see doc-drift item 11 above. Relevant for
   certification readiness ([[openadr-spec-use-cases]]).
7. **Event `priority` parsed but unused** in the rate merge (last-write-wins,
   acknowledged in a long comment at `openadr_interface.rs:102`) — sort by ascending
   priority before merging to close it.

Distilled drift flags live as callouts on [[ven-hexagonal-architecture]],
[[openadr-interface]], [[asset-layer]], [[dispatcher]], [[reliability-and-config]],
[[tariffs-and-capacity]]; source-doc fixes are queued in `wiki/review.md`.
