# Proposal

## Why

The VTN can see what the fleet *did* (telemetry) and when each VEN saw an event and replanned
(`/api/fleet/reactions`), but not what the fleet *plans*. A tariff change shows up in actual power
only when the affected hours arrive, possibly many hours after it is published, and weather and
time of day confound it. The VENs replan within minutes; their plans are where the effect of a price
signal is visible first, and it is what the tariff editor's before/after comparison needs
(`tariff-editor-compare-and-chart`). The phase-0 fleet design already reserves a `plan` topic
(`docs/plans/fleet-monitor/phase-0-foundation.md` §6.2, "cheap to add now"); nothing publishes it.

## What Changes

- The VEN publishes a compact projection of each **adopted** plan on `openadr-lab/fleet/<ven>/plan`
  (retained, QoS 1): plan id, creation time, trigger, and per slot start/end, planned net import and
  export power, and the import/export tariff the slot was planned against. Field names are those of
  `GET /plan` (no new vocabulary).
- The BFF consumes the `plan` leaf, stores every adopted plan (`lab_recorder.fleet_plan`) with
  retention, and serves `GET /api/fleet/plan?at=<instant>`: for each VEN the latest plan adopted at or
  before `at`, resampled to a common grid and summed into planned fleet import and export per slot.
- The VTN UI shows the planned fleet import/export on the Fleet page ("Fleet plan" card), so the
  capability is inspectable on its own (`ui-transparency`), and offers it to the tariff editor.
- No change to planning, to the plan's own contents, or to the VEN's HTTP API.

## Capabilities

### New Capabilities
- `fleet-plan-publishing`: what the VEN publishes about an adopted plan, when, and with what
  guarantees about the payload.
- `fleet-plan-view`: how the BFF stores plans and answers "what did the fleet plan as of instant T",
  and how the VTN UI shows it.

### Modified Capabilities
<!-- none: openspec/specs/ is empty -->

## Impact

- `VEN/src/fleet_telemetry.rs` (new `publish_plan` on `TelemetryPort`), `VEN/src/tasks/planning.rs`
  (calls it after adoption), `VEN/src/services/test_support/` (mock port).
- `VTN/bff/src/fleet.rs` (route the `plan` leaf), `fleet_store.rs` (table, writer, retention, query),
  new `fleet_plan.rs` (the resample-and-sum), `routes/fleet.rs` (one route).
- `VTN/ui/src`: `api/` client + hook + types, a `FleetPlanChart` component, a card on `pages/Fleet.tsx`.
- Tests: Rust unit tests on both sides, a BDD feature `tests/features/fleet_plan_feed.feature`,
  UI component tests; docs: `docs/architecture/` (VEN and VTN), phase-0 §6.2 table.
- Broker load: one retained ~30 kB message per VEN per adopted plan.
