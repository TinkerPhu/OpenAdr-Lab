## Context

See proposal.md - Why. Current state, read from the code:

- The VEN publishes `status`, `telemetry` and `trace` through the `TelemetryPort` trait
  (`VEN/src/fleet_telemetry.rs`: `publish_telemetry`, `publish_trace`, `to_wire`); `plan` and
  `signals` are reserved in phase-0 §6.2 and published by nobody.
- The BFF routes leaves in `fleet::apply_message` (`plan` is explicitly ignored), persists telemetry
  and trace through a queue (`fleet_store::TelemetryWriter::offer`, `offer_trace`) into
  `lab_recorder.*` tables with retention (`run_retention`), and answers history with
  `power_series` -> `group_and_resample` -> `sum_over_grid` over `lab_core::time_series::TimeSeries`
  (Step interpolation, `resample_uniform(step, Mean)`).
- A plan slot already carries `net_import_kw`, `net_export_kw`, `import_tariff_eur_kwh`,
  `export_tariff_eur_kwh` (`VEN/src/entities/plan.rs::PlanTimeSlot`); adoption happens in
  `services::planning::PlanningService::adopt_if_warranted`, called from `tasks/planning.rs`.

## Goals / Non-Goals

**Goals**: the VTN can state, for any instant, what the fleet planned to import and export; one
function sums plans, whether for now or an earlier instant.
**Non-Goals**: asset-level plan detail at the VTN; planned *cost*; replacing `/api/fleet/reactions`;
publishing the `signals` leaf; any change to planning or its acceptance gate.

## Inventory of existing logic reused or consolidated

| Concept | Where it lives today | This change |
|---|---|---|
| Resample a step series to a grid, sum across VENs | `lab_core::time_series`, `fleet_store::group_and_resample`, `sum_over_grid` | Reused unchanged; plans enter as step series (a planned kW holds for its slot) |
| Serialise a payload for the fleet topics | `fleet_telemetry::to_wire` | Reused |
| Port + no-op when unconfigured | `TelemetryPort`, `FleetMqttPublisher` | One more method, same pattern |
| Queue + table + retention for a fleet leaf | `fleet_store` trace path (`TraceRow`, `offer_trace`, `fleet_trace`, `run_retention`) | Same shape for plans; shared helpers extracted if the third copy of the writer boilerplate appears (task 2.3) |
| Topic routing | `fleet::apply_message`, `parse_topic` | The `plan` arm added |
| Which plan is current at an instant | VEN-side `AppState::active_plan`; `plan_history` | Not reused (different process); the BFF's rule is "latest adopted at or before", stated once in `plan_at` |

## Decisions

**D1 Payload is a projection with the plan's names.** Not the whole `GET /plan` (a 288-slot plan with
per-asset allocations is hundreds of kB, retained, for 20 VENs): `id`, `created_at`, `trigger` and per
slot only the six fields in the spec, under the names `GET /plan` uses. Alternative: full plan —
rejected for broker and store size. Alternative: pre-summed at the VEN — impossible, the sum is across
VENs.

**D2 A bridge task publishes the active plan when it changes (decided while applying 1.1).**
`tasks/fleet_plan.rs`, shaped like `tasks/fleet_trace.rs`: subscribe to the controller-trace broadcast,
and on each `PlanCycle` (pushed by `adopt_if_warranted` right after `set_active_plan`, adopted or not)
read `AppState::active_plan()` and publish it when its id differs from the last one published. A
rejected cycle leaves the active plan unchanged, so nothing is published; an adopted one does. The
planning loop, `run_plan_cycle` (already 17 parameters) and `services/` stay untouched and free of the
broker. Alternative: call `publish_plan` from `run_plan_cycle` — rejected: another parameter on a
function R-71 already lists, and the planner would know a broker exists. Alternative: a new
`PlannerEvent` — rejected: it is serialised to the UI over SSE and would carry the plan.

**D3 Store every adopted plan.** Table `lab_recorder.fleet_plan(ven_name, plan_id, created_at,
received_at, slots jsonb)`, unique on (ven_name, plan_id), retention 30 days like decisions.
Alternative: keep only the latest per VEN in memory — rejected: "as of an earlier instant" is the whole
point of the comparison.

**D4 `plan_at(at)` is the one rule.** `GET /api/fleet/plan?at=` selects per VEN the plan with the
greatest `created_at <= at`, builds a step series from its slots, resamples to a grid of the slots'
length, and sums with `sum_over_grid`. "Now" is `plan_at(now)`. The response lists the contributing
plan ids so a caller can tell a replan from a stale answer.

**D5 Gaps stay gaps.** A VEN's plan covers only its horizon; beyond it the VEN contributes nothing
(not zero), and `contributingVens` per slot says so, exactly as `/api/fleet/power` does.

**D6 `received_at` is stored but `created_at` decides.** The VEN's clock names when it adopted a plan;
the BFF's names when it learned. `at` is compared with `created_at`; a VEN with a skewed clock is
visible through the pair (as in reactions, §6.3).

## Risks / Trade-offs

- [Retained 30 kB x 20 VENs on a small broker] -> measured in task 4.2; if too heavy, drop retained
  and rely on the BFF store (the BFF is the only consumer).
- [Slot grids differ per VEN] -> resample to a common grid; the grid step is the shortest slot step
  seen, so no VEN is smeared.
- [Replans every few minutes grow the table] -> unique key + 30-day retention + only adopted plans.
- [Clock skew makes "before the publish" off by seconds] -> the comparison uses plans adopted well
  before the publish instant, not on it.

## Migration Plan

Additive: new topic, new table (`CREATE TABLE IF NOT EXISTS` in `init_schema`), new route. Roll back by
reverting; the table can be dropped by hand. VENs and BFF can deploy in either order (the BFF ignores
nothing it does not know; the VEN's extra topic is unread by an old BFF).

## Open Questions

1. Should the retained flag stay? Does not change specs; settled by the broker measurement in 4.2.
