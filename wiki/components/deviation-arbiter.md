---
title: Deviation Arbiter
type: component
created: 2026-07-28
updated: 2026-07-30
synced_commit: dfdb62b
sources: [VEN/src/controller/arbiter.rs, VEN/src/controller/arbiter/arbiter_levers.rs, VEN/src/entities/arbiter_residual.rs, VEN/src/tasks/sim_tick/arbiter_glue.rs, VEN/src/tasks/sim_tick/tick.rs, VEN/src/state/arbiter.rs, VEN/src/routes/hems/arbiter.rs, VEN/ui/src/components/devices/ArbiterSettingsCard.tsx, docs/architecture/VEN_ARCHITECTURE.md, docs/architecture/ven_milp_planner.md, docs/use-cases/HEMS-USE-CASE-OBSERVATION-MANUAL.md, docs/reference/KEY_LEARNINGS.md, docs/history/project_journal.md]
tags: [arbiter, deviation, real-time, marginal-cost, ven]
---

# Deviation Arbiter

The single real-time layer that reconciles the [[milp-planner]]'s plan against what the
[[simulator]] actually measures each tick, replacing the ad hoc mix of the opportunistic
EV-surplus overlay and no owner for "who corrects a deviation." `controller::arbiter::reconcile`
runs once per 1 s tick (`tasks/sim_tick/arbiter_glue.rs`), ranks the available levers
(battery, EV, heater-pause, heater emergency curtail/absorb, PV curtailment) by their
current marginal €/kWh, and greedily consumes the tick's `deviation_kw` cheapest-lever-first,
respecting each lever's remaining capacity (an EV already at target SoC or a battery at its
SoC limit contributes zero, not merely low priority) — `docs/architecture/VEN_ARCHITECTURE.md`
§2.1.

## Why a single arbiter, not another reactive loop

A real-time correction layer (feature 017, `absorber.rs`, `PlanTrigger::DeviceDeviation`) was
built and removed twice for oscillating against the pre-existing opportunistic EV-surplus
overlay — two independent writers reacting to a one-tick-stale snapshot, with no arbitration
order between them (`docs/reference/KEY_LEARNINGS.md`'s Deviation Absorber section). The arbiter
is not a revival of that mechanism: it **subsumes** the overlay (folded into `arbiter_levers.rs`
rather than left as a second loop) and reads the current tick's actual PV/base-load, not a lagged
one — the two specific failure modes that killed feature 017. `LEVER_PREEMPTION_MARGIN_EUR_PER_KWH`
adds hysteresis so a lever near a cost tie doesn't chatter between ticks.

## Battery/EV deviation-correction runaway (found and fixed 2026-07-30)

A third, distinct real-time-oscillation bug surfaced live on ven-1: `projected_net_kw`'s
battery/EV term fell back to `base_setpoints` (the plan's static per-slot allocation) instead of
`AssetSnapshot.setpoint_kw` (the arbiter's own last-applied command) — the same field
`apply_battery_lever`/`apply_ev_lever` already use as their integrator state. A correction applied
on tick N was invisible to tick N+1's deviation calc, so it either stacked a fresh correction on
top of the last one (runaway) or got silently reverted (`reconcile`'s `setpoints` baseline also
defaulted to `base_setpoints`), re-creating the deviation next tick. Both call sites now read
`setpoint_kw` for battery/EV specifically. Verified live: 12+ minutes flat at a single kW value
post-fix, vs. the pre-fix rapid ±0.1–0.2 kW alternation every ~90 s. `docs/history/project_journal.md`
("Deviation arbiter battery/EV runaway fix"); regression test
`reconcile_battery_converges_under_stationary_disturbance_not_runaway_to_clamp`
(`controller/tests/arbiter_tests.rs`) drives the real `reconcile` entry point, not just the
lever in isolation — the pre-existing multi-tick convergence test bypassed `reconcile` and
therefore missed this.

## Marginal-cost signal

The ranking isn't a hand-written priority table (cost vs. self-consumption vs. comfort vs. DR
obligation) — it's the plan's own shadow price. See [[milp-planner]]'s marginal-cost extension
(§5.2) for how `SolverPort` produces `marginal_cost_import/export_eur_per_kwh` per slot; a DR
obligation's breach penalty and a routine tariff both collapse into the same number, so an active
event doesn't need a special-cased rule to win — it just has a higher shadow price. §5.5 notes
this is a **within-tick tie-breaker, not a substitute for replanning**: greedy absorption doesn't
know a resource's later obligations, which is what the residual-based replan trigger below is for.

## Residual escalation → replan

Absorbed kWh on SoC-coupled assets (battery, EV) accumulates in `entities::arbiter_residual::AssetResidual`.
Once `breach_fraction` crosses `RESIDUAL_THRESHOLD_FRACTION` (0.2) past a 900 s cooldown,
`arbiter_glue::apply_residual_escalation` fires `PlanTrigger::ResidualThreshold` for a fresh MILP
replan — the mechanism feature 017 was evolving toward before its removal
(`docs/reference/KEY_LEARNINGS.md`'s Deviation Absorber section, "Residual vs. raw deviation for
Tier 2 triggers"): a replan recomputes future obligations against real SoC instead of trusting
stale marginal-cost numbers.

## Levers

| Lever | Mechanism | Capacity check |
|---|---|---|
| Battery | Charge/discharge adjustment within power limits | SoC bounds |
| EV | Adjust opportunistic/session charging | Session target SoC |
| Heater pause | Suppress in-progress heating | Zero-cost only when mid-cycle toward `temp_max_c` |
| Heater emergency curtail/absorb | Drift below `temp_min_c` / heat above `temp_max_c` toward `temp_safety_max_c` | [[asset-layer]]'s `HeaterEmergencyMode` |
| PV curtailment | Reduce `p_pv_used` below forecast | Only relieves an active export cap |

## Runtime toggle and diagnostics surface

`deviation_arbiter_enabled` (default `false` in every profile) gates the whole mechanism —
exposed via `GET/PUT /arbiter-settings` (`routes/hems/arbiter.rs`), mirroring the pre-existing
`/ev-settings` pattern, and surfaced in the VEN UI's `ArbiterSettingsCard` on the Devices page.
`GET /arbiter-diagnostics` (backed by `AppState.arbiter_diagnostics`, updated every tick alongside
the preemption-margin hysteresis state) reports the last tick's projected net site power, residual
deviation, and active lever — the `ArbiterSettingsCard` renders this readout while the arbiter is
enabled (ui-transparency: no reactive-lever decision without an inspectable surface). See
`docs/use-cases/HEMS-USE-CASE-OBSERVATION-MANUAL.md` UC-15 for the observation walkthrough.

> **DRIFT** The Planner tab's `CorrectionBanner` (`VEN/ui/src/pages/Planner.tsx`, labeled
> "Plan F: Layer 1 reactive correction") listens for SSE events `correction_active`/
> `correction_cleared` on the planner event stream, but no backend code constructs a
> `PlannerEvent::CorrectionActive`/`CorrectionCleared` variant (`VEN/src/planner_events.rs`'s
> `PlannerEvent` enum has no such variants) — the banner is permanently dead UI, a leftover
> from feature 017's design vocabulary that predates this arbiter and was never wired to it.
> See [[ven-ui]].

## Relationship to the dispatcher

The arbiter and [[dispatcher]]'s `apply_surplus_ev_overlay` overlap in intent (both react to live
PV surplus for EV charging) but are not both active: when `deviation_arbiter_enabled` is true,
the arbiter's EV lever is the one live reactive layer for a tick; the dispatcher's separate
overlay function still exists in code, gated to the `deviation_arbiter_enabled=false` rollout
path only, but the arbiter is designed to be its eventual replacement.
