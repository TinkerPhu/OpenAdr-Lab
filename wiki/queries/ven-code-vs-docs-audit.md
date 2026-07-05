---
title: VEN Code vs Documentation Audit
type: query
created: 2026-07-05
updated: 2026-07-05
synced_commit: e138861
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

1. **`SolverPort` does not exist.** `.claude/CLAUDE.md` §ven-architecture and
   `docs/architecture/VEN_ARCHITECTURE.md` list it as a port obligation; there is no such
   trait anywhere. `tasks/planning.rs:266` calls `milp_planner::run_planner()` (a free
   function) directly inside `spawn_blocking`. Solver isolation is real but comes from
   `AssetMilpContext` trait objects, not a solver port. See [[ven-hexagonal-architecture]].
2. **`StaleRatePolicy` is dead vocabulary.** The enum (`entities/asset.rs:109`) is never
   referenced; `PlanTimeSlot.rate_estimated` is hardcoded `false` in `results.rs`. Actual
   stale-rate behaviour: Step/LOCF carries the last known tariff forward indefinitely,
   and hardcoded defaults (0.25 €/kWh import, 0.08 export, 300 g/kWh) fill slots before
   the first sample or when no data exists (`milp_planner/inputs.rs:77-90`). See
   [[tariffs-and-capacity]].
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
   `#[allow(dead_code)]` — deliberately not wired into `build_setpoints`. Ledger
   accounting is `monitor::record_tick`, called from `sim_tick/publish.rs`, not the
   dispatcher. See [[dispatcher]].
6. **Plan-cycle status reports are dead on arrival.** `tasks/planning.rs:338` calls
   `build_status_report(..., program_id: None, ...)`, and the function returns `None`
   unless a program id is supplied (`reporter.rs:512`) — so the TELEMETRY_STATUS report
   the planning loop appears to submit is never built. Either wire a real programID or
   delete the call path.
7. **`DomainError` is three-fifths unused.** `PlanInfeasible`, `VtnUnreachable`,
   `ProfileInvalid` are never constructed in production code; solver failure produces a
   fallback plan with a Critical `PlanWarning` (`results.rs::fallback_plan`), and profile
   validation returns `Vec<String>`. Only `SessionConflict` and `NotFound` are live
   (`services/hems.rs`). See [[reliability-and-config]].
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
10. **`services/test_support` is `#[cfg(test)]`-gated** (`services/mod.rs:2`), while
    `.claude/CLAUDE.md` §testing says the shared mocks are "not cfg(test)".

## Refactoring candidates (ranked)

1. **File-size rule vs reality.** `.claude/CLAUDE.md` demands ≤ 500 lines per
   `VEN/src/` file and < 200 for `tasks/`. Even counting only production lines (before
   `#[cfg(test)]`): `assets/heater.rs` 799, `profile.rs` 777, `assets/mod.rs` 687,
   `routes/hems.rs` 678, `assets/ev.rs` 634, `controller/reporter.rs` 559,
   `assets/battery.rs` 523, `state.rs` 519, `simulator/mod.rs` 503, and
   `tasks/planning.rs` 363 (vs 200). Either split (heater/ev: physics vs MILP-context
   impls are separable; hems.rs: one route module per resource; tasks/planning.rs: hoist
   the context-building block into `services/planning.rs`) or amend the rule.
2. **Dead-code inventory** — delete or wire: `StaleRatePolicy`,
   `apply_battery_correction_overlay`, the plan-cycle status-report call,
   `HvacService`, three `DomainError` variants, and the large unused §-numbered
   vocabulary block in `entities/asset.rs` (`AssetProfile`, `AssetHeuristics`,
   `AssetForecast`, `AssetLedger` (the §3.7 one — the live ledger is
   `state::AssetLedgerEntry`), `PenaltyRule`, `ComfortRate` machinery, `OadrEventCache`,
   `OadrProgramConfig`, `OadrCapacityRequest` in `entities/capacity.rs`). These are
   spec-transcription types under `#![allow(dead_code)]` that mislead readers (and this
   audit's doc-drift items 2 and 3 are their direct consequence).
3. **Slot-start tariff sampling.** `build_milp_inputs` evaluates tariffs at each slot's
   start (`interpolate_at(slot_t)`); `TimeSeries::time_weighted_mean` already exists and
   would price boundary-straddling slots correctly — the exact fix
   `VEN_ARCHITECTURE.md` §5.3 sketches, one call away.
4. **Application ring imports axum.** `services/hems.rs:52` implements
   `From<DomainError> for (StatusCode, Json)` — HTTP mapping belongs in `routes/`.
5. **Envelope magic numbers.** `milp_planner/envelopes.rs` hardcodes
   `max_acceptable_rate: 0.35`, `min_acceptable_rate: 0.05`,
   `budget_remaining_eur: 1.0e9` in every `FlexibilityEnvelope`.
6. **One-shot obligations.** `extract_report_obligations` creates a single obligation
   per (event, payloadType); once fulfilled it never re-arms, so a descriptor with
   `frequency: 900` yields one report, not a report every 15 min
   (`controller/openadr_interface.rs:242`, `services/obligation.rs`). Relevant for
   certification readiness ([[openadr-spec-use-cases]]).
7. **Event `priority` parsed but unused** in the rate merge (last-write-wins,
   acknowledged in a long comment at `openadr_interface.rs:102`) — sort by ascending
   priority before merging to close it.

Distilled drift flags live as callouts on [[ven-hexagonal-architecture]],
[[openadr-interface]], [[asset-layer]], [[dispatcher]], [[reliability-and-config]],
[[tariffs-and-capacity]]; source-doc fixes are queued in `wiki/review.md`.
