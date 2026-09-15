## Context

`tasks/sim_tick/helpers.rs::build_tick_setpoints` builds each tick's setpoints: plan setpoints →
(`deviation_arbiter_enabled` ? `arbiter::reconcile` : `dispatcher::apply_surplus_ev_overlay`) →
`apply_dispatch_override` → `apply_comms_loss_clamp`. `reconcile` corrects the deviation of
projected net power from the plan slot's net with ranked levers (battery, EV, heater pause, heater
emergency, PV curtail). Nothing compares projected import with the active hard limit
(`ctx.capacity_snap.import_limit_kw`, alert windows). A design review found that bolting a second
pass after the first would make them fight every tick (the battery lever's integrator is last
tick's applied command), and several lever functions assume things a limit pass breaks.

## Goals / Non-Goals

**Goals:** active import limit met from the next tick; one arbiter with shared machinery; stable
across ticks; traceable decisions; switchable for planner-only measurements.

**Non-Goals:** export side (PV curtailment via the generation limit already meets export caps);
planner-side repair of timed-out incumbents (GB-40 follow-up); tuning solver timeouts.

## Decisions

**D1 — Shared target.** `effective_target_kw = min(plan_signed_net_kw, hard_limit_kw − margin)`,
`hard_limit_kw` = active capacity import limit, or 0 during an active alert window (strictest
wins). The deviation pass uses it in `deviation_kw`, so with both passes on the limit pass is a
no-op backstop. *Alternative:* two independent targets — rejected, they oscillate.

**D2 — Limit pass memoryless.** It reads only this tick's setpoint map and live inputs; no
integrator state. With deviation correction off it recomputes from the plan each tick.

**D3 — One rank-and-apply loop.** Extracted from `reconcile`, called by both passes, each with its
own incumbent (preemption hysteresis must not leak between passes).

**D4 — Levers read the setpoint map; `apply_*` return the achieved delta.** `reconcile` seeds the
map from `snap.setpoint_kw` for battery/EV, so its behaviour is unchanged.

**D5 — `LeverPolicy` per pass, as data.** Limit pass: EV may reduce a planned allocation; battery
ignores the MaxRevenue discharge refusal. Both passes: heater emergency (Curtail) only while an
alert is active; costs come in as data with a fallback (no plan yet).

**D6 — Stage-aware heater.** `AssetSnapshot.power_steps_kw` from the asset's capability; one shared
quantization function used by the heater's `step_inner` and by the arbiter's projection; the
arbiter commands exact steps only (highest ≤ current − needed); no pause capacity while
`forced_power_kw` is reported; a paused stage returns only when `projected + step ≤ target −
margin`. During an alert, Curtail also sets the heater setpoint to 0.

**D7 — Ordering.** Limit pass last. A hard cap beats `DISPATCH_SETPOINT`; the comms-loss clamp's
per-asset bounds cap lever capacity; `apply_dispatch_override` uses the shared projection.
Limit-pass Curtail overrides a deviation-pass Absorb.

**D8 — Residual feeds energy.** Limit-pass battery/EV adjustments add `(applied − planned) × dt_h`;
heater pause doesn't feed it. Persistent limit use triggers a replan; no per-tick storm.

**D9 — Unresolvable excess reported** with the remaining kW (forced power / comfort floor).

**D10 — Trace.** `ArbiterDiagnostics` per pass (target, excess before/after, adjustments,
unresolved, measured-vs-projected); `ControllerEvent::ArbiterDecision` on state change only.

## Risks / Trade-offs

- [Heater chatter near the threshold] → release hysteresis (D6).
- [Planned EV charging cut during a cap may miss a departure] → feeds residual → replan; visible in
  the trace.
- [Limit pass hides planner mistakes] → the toggle allows planner-only measurements.
- [Size caps in `tasks/`] → logic in `controller/arbiter*`, call sites only in `tasks/`.

## Migration Plan

No data migration; `limit_enforcement_enabled` defaults to true at startup. Rollback = revert.
