# Proposal

## Why

The lab's tariff is changed today by editing JSON in the Events form or re-running
`scripts/seed_vtn.py`. Nobody can change a price and watch the fleet's import and export
planning react without leaving the VTN UI. GHG signals and capacity envelopes will need the
same edit-and-observe loop, so the editor must be built once as a generic signal workbench,
not as a tariff page that is copied twice later.

The most common way a tariff is declared in this lab, and in the OpenADR spec, is a day of
prices that repeats indefinitely: one event with `duration = "P9999Y"` (User Guide 647; seeded
as `tou-pricing-day-ahead` in `scripts/seed_vtn.py`). An editor that can only author a flat list
of dated intervals could not create, or even faithfully round-trip, the tariff the lab actually
runs on.

## What Changes

- New VTN UI page "Tariff" (route `/tariff`): a signal workbench for `PRICE` and
  `EXPORT_PRICE`, with a grid editor (one row per interval, paste from CSV, fill-down) and a
  live preview that reuses the existing fleet tariff chart.
- A **recurrence** control on every edited series: `once`, `repeat for <ISO 8601 duration>`
  (e.g. `P7D`), and `forever` (`P9999Y`). It is written to `event.duration`, the spec's own
  control, and nothing else.
- A signal registry (data, not branches): per signal type its unit, currency, grid and value
  kind. This change registers `PRICE` and `EXPORT_PRICE`; GHG and capacity envelopes are later
  rows.
- Loading an existing event into the draft, including a looping one, and publishing the draft
  as an event with the descriptors the registry declares (`wire-contracts`).
- Publishing creates a new event with a higher priority than the one it supersedes; it never
  edits the live event in place (assumption A1 in design.md).
- No change to the VEN, to `lab_core::event_timing`, or to the planner: the editor emits events
  that the existing shared interval rule already understands.

## Capabilities

### New Capabilities
- `signal-workbench`: registry-driven draft editor for VTN signals — draft model, grid input,
  validation, loading from and publishing to an event, with the live-versus-draft overlay.
- `event-recurrence-editing`: how a recurrence choice maps to `event.duration`, how an existing
  event's duration is read back into that choice, and what the preview shows for it.

### Modified Capabilities
<!-- none: openspec/specs/ is empty; no existing spec changes -->

## Impact

- `VTN/ui/src/`: new `pages/Tariff.tsx`, `utils/signalDraft.ts`, `utils/signalRegistry.ts`,
  `utils/recurrence.ts`, a grid component; `FleetTariffChart.tsx` gains a draft series;
  `App.tsx` route and nav entry; `api/hooks.ts` reuses the existing event create hook.
- `VTN/bff/`: a small preview-expansion endpoint (design D4), reusing
  `lab_core::event_timing::timed_intervals` so the UI has no second copy of the looping rule.
- `lab-core/event_timing.rs`: read only.
- Tests: Vitest for the draft/recurrence units; a BDD scenario in `tests/features/` for the use
  case "change the tariff and see the fleet plan change"; use-case doc update.
- Follow-up change (separate spec): `tariff-editor-compare-and-chart` — before/after fleet
  power and drag-on-chart.
