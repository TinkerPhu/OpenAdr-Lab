## Why

A capacity limit is a schedule, but the VEN treats it as one number. `OadrCapacityState.import_limit_kw`/`export_limit_kw` (`parse_capacity_state`) is the strictest limit over every interval of every event the VTN returns for `?active=true`, which also keeps events that have not started yet. The planner applies that number to every slot of its horizon (GB-48), the PV generation-limit resolver applies a future export limit now, and the Dashboard, the simulated grid asset and the `CapacityChange` trace show a future limit as the current one. The demo seed (`scripts/seed_vtn.py`) already triggers it: its 0 kW import limit two hours ahead would make ven-2 plan the whole day at 0 kW import.

The per-interval schedule (`parse_capacity_schedule`) exists but drops the spec's own Dynamic Operating Envelope form: one event-level `intervalPeriod` with contiguous intervals that carry none (OpenADR 3.1 User Guide §7.3, Example 8.10.1-1). A 48 × 30 min DOE day parses to an empty schedule. Switching consumers to the schedule without fixing that would drop spec-form DOEs entirely.

Behind both is the pattern this project has to stop: "which interval of an event covers time t" is resolved in seven places with four different rules (`collect_interval_groups`, `parse_alert_windows`, `parse_simple_windows`, `parse_dispatch_windows`, `parse_charge_state_setpoint`, `reporter::event_is_active`, and not at all in `parse_capacity_state`), with the default for a missing duration being PT1H in five of them and one year in the reporter. "The limit in force at t" is looked up in three (history sampler, GB-47 limit pass, and now the planner).

## What Changes

- **One OpenADR interval-timing resolver** (new, shared): resolves every interval of an event to an absolute `[start, end)` per the spec — an interval's own `intervalPeriod` wins; otherwise it starts where the previous interval ended (the first at `event.intervalPeriod.start`) and lasts the event-level default duration. All seven call sites use it; their private timing code is deleted.
- **Spec-form DOE schedules parse**: multi-interval events with only an event-level `intervalPeriod` produce contiguous intervals instead of nothing.
- **One capacity-schedule lookup** (new, shared): the limit in force at an instant and the tightest limit overlapping a span, per direction. Used by the planner, the "in force now" state, the history sampler and the GB-47 limit pass (replacing their own lookups).
- **Planner caps per slot**: each slot's import/export cap is the tightest scheduled limit overlapping the slot, else the physical limit. A limit announced for later caps only the slots it covers, so the planner can prepare for it (e.g. charge the battery before the window).
- **"In force now" state**: `OadrCapacityState.import_limit_kw`/`export_limit_kw` become the limit in force at poll time, derived from the schedule. Every consumer of the folded value (PV generation-limit resolver, grid asset, Dashboard, `/capacity`, `CapacityChange` trace) becomes correct without its own change; the trace now marks a change of the limit in force.
- **BREAKING (behaviour)**: overlapping limits resolve by event priority (GB-45 rule, spec §7.1), not "strictest wins". Alerts, SIMPLE levels, dispatch and charge-state events that are multi-interval without per-interval periods now get contiguous windows instead of every interval inheriting the whole event window.

## Capabilities

### New Capabilities
- `openadr-interval-timing`: resolving the absolute start/end of every interval of an OpenADR event, per OpenADR 3.1 §7.3, once for all event parsers.
- `time-aware-capacity-limits`: the capacity-limit schedule as the single source for "limit in force at t" and "tightest limit over a span", feeding planner slot caps, the in-force-now state and every execution-side consumer.

### Modified Capabilities
<!-- none: openspec/specs/ holds no capability specs -->

## Impact

- Code: `controller/rate_schedule.rs` (timing via the resolver), new timing module in `controller/`, `controller/openadr_interface.rs` (alert/SIMPLE/dispatch/charge-state parsers, `parse_capacity_state`), `controller/reporter.rs` (`event_is_active`), `entities/capacity.rs` (schedule lookup), `controller/solver_port.rs` + `services/planning` + `tasks/planning/cycle.rs` (schedule into the solve request), `controller/milp_planner/inputs.rs` (per-slot caps), `controller/arbiter/limit.rs` and `tasks/history_sampler/accumulator.rs` (use the shared lookup).
- API: `/capacity` keeps its shape; its limit fields now mean "in force now". `/capacity/schedule` gains spec-form DOE intervals.
- UI: no code change expected; the Dashboard's limit and compliance chip become correct.
- Tests: `test_parse_capacity_state_strictest_wins` and `test_parse_capacity_schedule_does_not_guess_for_multi_interval_events` change expectation (to be explained with the change); new tests from User Guide Example 8.10.1-1 and the seed's ven-2 event; a BDD use case for a limit announced in advance.
- Docs: `VEN_ARCHITECTURE.md` (event timing, capacity), use-case doc, BACKLOG GB-48.
