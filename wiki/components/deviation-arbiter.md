---
title: Deviation Arbiter
type: component
created: 2026-07-28
updated: 2026-07-28
synced_commit: c27b296
sources: [VEN/src/controller/arbiter.rs, VEN/src/controller/arbiter/arbiter_levers.rs, VEN/src/entities/arbiter_residual.rs, VEN/src/tasks/sim_tick/arbiter_glue.rs, VEN/src/routes/hems/arbiter.rs, VEN/ui/src/components/devices/ArbiterSettingsCard.tsx, docs/plans/deviation-scenarios-analysis.md, openspec/changes/deviation-arbiter/]
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
SoC limit contributes zero, not merely low priority) — `docs/plans/deviation-scenarios-analysis.md`
§5.3.

## Why a single arbiter, not another reactive loop

A real-time correction layer (feature 017, `absorber.rs`, `PlanTrigger::DeviceDeviation`) was
built and removed twice for oscillating against the pre-existing opportunistic EV-surplus
overlay — two independent writers reacting to a one-tick-stale snapshot, with no arbitration
order between them (`docs/plans/deviation-scenarios-analysis.md` §1). The arbiter is not a
revival of that mechanism: it **subsumes** the overlay (folded into `arbiter_levers.rs` rather
than left as a second loop) and reads the current tick's actual PV/base-load, not a lagged one —
the two specific failure modes that killed feature 017. `LEVER_PREEMPTION_MARGIN_EUR_PER_KWH`
adds hysteresis so a lever near a cost tie doesn't chatter between ticks.

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
replan — the mechanism feature 017 was evolving toward before its removal (deviation-scenarios-analysis.md
§5.5): a replan recomputes future obligations against real SoC instead of trusting stale
marginal-cost numbers.

## Levers

| Lever | Mechanism | Capacity check |
|---|---|---|
| Battery | Charge/discharge adjustment within power limits | SoC bounds |
| EV | Adjust opportunistic/session charging | Session target SoC |
| Heater pause | Suppress in-progress heating | Zero-cost only when mid-cycle toward `temp_max_c` |
| Heater emergency curtail/absorb | Drift below `temp_min_c` / heat above `temp_max_c` toward `temp_safety_max_c` | [[asset-layer]]'s `HeaterEmergencyMode` |
| PV curtailment | Reduce `p_pv_used` below forecast | Only relieves an active export cap |

## Runtime toggle

`deviation_arbiter_enabled` (default `false` in every profile) gates the whole mechanism —
exposed via `GET/PUT /arbiter-settings` (`routes/hems/arbiter.rs`), mirroring the pre-existing
`/ev-settings` pattern, and surfaced in the VEN UI's `ArbiterSettingsCard` on the Devices page.
Disabled by default because the marginal-cost duals (§5.2) are new and unvalidated against real
deviation reduction — `docs/plans/deviation-scenarios-analysis.md` §5.6 flags that no experiment
has yet quantified the effect.

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
overlay function still exists in code but the arbiter is designed to be its replacement, per
`docs/plans/deviation-scenarios-analysis.md` §5.3's "opportunistic overlay is folded into this
arbiter."
