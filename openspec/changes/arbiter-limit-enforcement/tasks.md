## 1. Behaviour-neutral refactor (`controller/arbiter.rs`, `arbiter/arbiter_levers.rs`)

- [x] 1.1 Levers read the current setpoint from the setpoint map (`projected_net_kw`, `battery_lever`, `apply_battery_lever`, `ev_lever`, `apply_ev_lever`)
- [x] 1.2 Every `apply_*` returns the achieved delta; the loop subtracts achieved, not assigned
- [x] 1.3 Extract the rank-and-apply loop into one shared function; introduce `LeverPolicy` (data) with the deviation pass's current behaviour
- [x] 1.4 All existing arbiter/dispatcher/tick tests green unchanged (docker `ven-unit-test` on Node2)

## 2. Stage-aware heater

- [x] 2.1 Tests first: 0.72 kW shed from 1.75 kW (steps 0/1.75/3.5) commands 0; no pause capacity while forced; restore only below target − margin
- [x] 2.2 `AssetSnapshot.power_steps_kw` from capability (`controller/simulator_port.rs`, `simulator/snapshot.rs`, test-support builders)
- [x] 2.3 Shared step quantization used by `assets/heater.rs::step_inner` and the arbiter projection
- [x] 2.4 Heater pause lever: exact steps, forced guard, release hysteresis

## 3. Limit pass and shared target

- [x] 3.1 Tests first (campaign cases): ven-10 heater → 0; ven-1 battery covers 2.3 kW; planned EV reduced; alert Curtails emergency heat + setpoint 0, capacity limit doesn't; forced power respected; no-plan window; MaxRevenue; unresolvable reported; 10-tick stability with both passes on; dispatch setpoint above cap; comms-clamp bounds respected
- [x] 3.2 `hard_limit_kw` (capacity limit, 0 in alert) and `effective_target_kw` used by both passes
- [x] 3.3 Limit pass in `controller/arbiter` (split module if needed) using the shared loop and a limit `LeverPolicy`; alert-only Curtail in both passes
- [x] 3.4 Tick wiring in `tasks/sim_tick/helpers.rs` (last step), comms-clamp bounds, `apply_dispatch_override` on the shared projection
- [x] 3.5 Energy-based residual feed for limit-pass battery/EV adjustments
- [ ] 3.6 fmt, clippy, file-size audit, architecture greps, full `cargo test`

## 4. Toggle, diagnostics, trace, UI

- [x] 4.1 `limit_enforcement_enabled` (default true): `HemsState`, `state/arbiter.rs`, tick context, `GET/PUT /arbiter-settings`
- [x] 4.2 `ArbiterDiagnostics` per pass; `ControllerEvent::ArbiterDecision` on state change only
- [x] 4.3 UI: second switch + limit state on `ArbiterSettingsCard`; Event Log renders `ArbiterDecision`; UI unit tests, eslint

## 5. Use-case BDD

- [x] 5.1 Cap event + injected base-load step → import ≤ cap within 30 s; decision in `/arbiter-diagnostics` and `/trace/events`; with limit enforcement off the step exceeds the cap
- [ ] 5.2 E2E + resilience green on Node2 (detached)

## 6. KPIs and harness

- [x] 6.1 Self-checks first: utilisation, unused headroom, comfort block (`experiments/compliance.py`)
- [x] 6.2 `kpi.py` reports them per window; harness poller saves `/trace/events` to `<ven>-arbiter-events.jsonl`

## 7. Merge, deploy, verify, docs

- [ ] 7.1 Rebase + ff-merge, deploy Node1 + Node2
- [ ] 7.2 `run_batch.py` S-3 + S-7 (fresh results root) plus one S-7 with limit enforcement off; ven-10/12/20 pass; read utilisation/comfort and arbiter events
- [ ] 7.3 Docs: `VEN_ARCHITECTURE.md` arbiter section, use-case doc, `FLEET_EXPERIMENT_DESIGN.md`, BACKLOG (GB-47 done; GB-40 keeps feasibility repair), project + fleet journal; delete this change
