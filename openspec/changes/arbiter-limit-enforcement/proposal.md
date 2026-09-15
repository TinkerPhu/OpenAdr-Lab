## Why

The 2026-09-12/14 fleet campaign met every alert and export cap but only 15 of 21 engaged import
caps (GB-47, `docs/BACKLOG.md`). Heater VENs' cap-aware plans end `TIME_LIMIT` and the returned
incumbent schedules a heater stage inside the cap; and unforecast load (ven-1's real base load at
2.3 kW under a 1.0 kW cap) isn't corrected because the deviation arbiter is off by default. The
arbiter only corrects deviation *from the plan*, so a plan that itself violates the cap is executed
as-is. A hard grid limit has to be met at execution, from the next tick, whatever the plan says.

## What Changes

- The deviation arbiter gains a **limit-enforcement pass**: after all other setpoint adjustments,
  it keeps projected import at or below the active hard import limit (capacity limit; 0 kW during
  an alert) using the arbiter's own levers, ranking and apply loop — one arbiter, not a parallel
  component.
- Two independent toggles on the arbiter: **deviation correction** (existing, default off) and
  **limit enforcement** (new, default on).
- Both passes aim at one shared target, `min(plan net, hard limit)`, so they never work against
  each other.
- Heater handling becomes **stage-aware** in both passes (commands only reachable power steps; no
  claimed capacity while the thermostat forces power; release hysteresis).
- Heater emergency heat below the comfort floor (Curtail) may be curtailed **only during alerts**,
  in both passes (today a capacity limit's penalty-inflated marginal cost can open it).
- A hard cap beats `DISPATCH_SETPOINT`; the comms-loss clamp bounds the limit pass.
- Arbiter decisions become traceable: extended `/arbiter-diagnostics`, an edge-triggered
  `ArbiterDecision` controller event on the Event Log page, both toggles on the Devices card.
- Fleet KPIs gain limit **utilisation** and **comfort** next to the unchanged pass bar.

## Capabilities

### New Capabilities
- `arbiter-limit-enforcement`: execution-time enforcement of hard import limits by the arbiter,
  its toggles, lever policy per pass, stage-aware heater handling, ordering decisions and trace
  surface.
- `fleet-limit-utilisation-kpis`: utilisation and comfort KPIs for limit windows.

### Modified Capabilities
<!-- none: no openspec/specs/ exist -->

## Impact

- VEN: `controller/arbiter.rs`, `controller/arbiter/arbiter_levers.rs` (+ a split module if size
  requires), `controller/simulator_port.rs` + `simulator/snapshot.rs` (`power_steps_kw`),
  `assets/heater.rs` (shared quantization), `tasks/sim_tick/helpers.rs` / `arbiter_glue.rs` /
  `dispatch_override.rs` / `context.rs`, `state/arbiter.rs`, `routes/hems/arbiter.rs`,
  `controller/trace.rs`; UI `ArbiterSettingsCard`, Event Log; BDD scenario.
- Experiments: `compliance.py`, `kpi.py`, `run_experiment.py` (trace polling).
- Behaviour: VENs now shed load (battery → EV → heater by marginal cost) to meet active import
  limits even when the plan doesn't; planned EV charging may be reduced during a cap.
