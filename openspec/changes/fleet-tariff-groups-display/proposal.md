# Proposal

## Why

The Fleet page draws the tariff directly above the metered fleet power, which says "this price produced
that curve". With VENs on different tariffs (the 2026-10-29 experiment sent five targeted price events to five
VEN groups), the page draws only the largest group's bands and calls the other VENs "dissenting". It also
draws the standard event next to the priority-1 event that overrides it, because `/api/fleet/signals`
returns every event that targets a VEN without ranking them. The flatter curve it shows is therefore
unexplained, and the tariff beside it is not the one it was built under (BACKLOG GB-64).

## What Changes

- `GET /api/fleet/signals` returns, per VEN, only the bands that win: where events overlap, the one the VEN
  itself would act on (priority, then newer `createdDateTime`, absent priority last), per payload type.
  The ranking is the rule the VEN already uses; it moves into `lab-core` so both call one implementation.
- The Fleet page's tariff card shows one tariff per distinct set of price bands: the import curves overlaid in
  one chart and the export curves in a second chart below it, same colour per group. Each curve is labelled
  with a short name taken from the event name and the VEN count (for example `g3 · 6 VENs`); the VEN list is
  on hover. A fleet with one tariff shows one curve labelled with all its VENs.
- The "VEN(s) were sent a different tariff" note and the "largest group wins" fold are removed: a deliberate
  split is shown, not reported as a disagreement.
- The Tariff page's workbench keeps drawing its unpublished draft beside the live curves.

## Capabilities

### New Capabilities
- `fleet-tariff-groups`: what the Fleet page and the signals feed say about the tariff each VEN is under when
  VENs are priced differently.

### Modified Capabilities
<!-- None: `openspec/specs/` holds no capability for the signals feed or the tariff card. -->

## Impact

- `lab-core/src/`: the priority-resolution core moves here from `VEN/src/controller/rate_schedule.rs`
  (`collect_interval_groups`, `resolve_segments`); the VEN's `parse_rate_snapshots` and
  `parse_capacity_schedule` call it.
- `VTN/bff/src/fleet_signals.rs` and `routes/fleet.rs`: bands resolved through the shared core; the stale
  "priority is deliberately absent" comment goes.
- `VTN/ui/src/`: `utils/fleetTariff.ts` (`tariffGroups` replaces `sharedTariff`), `components/FleetTariffChart.tsx`
  (import chart, export chart, labels), `pages/Fleet.tsx`, `pages/Tariff.tsx` (same component), `api/types.ts`
  unchanged (the band shape is the same).
- Deploys: BFF, VTN UI and, because the VEN's source changes, the VEN image via the normal ci-handoff process.
  The wire to the VENs does not change.
- Docs: `docs/use-cases/` (tariff per VEN group), journal, KEY_LEARNINGS, BACKLOG GB-64 row removed on merge.
- Out of scope, filed: assigning a tariff to VENs in the UI (GB-65).
