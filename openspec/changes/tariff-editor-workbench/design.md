## Context

See proposal.md - Why. Current state, as read from the code:

- The tariff on the wire is one event per signal pair: `tou-pricing-day-ahead` has 24 hourly
  `PRICE` intervals plus `event.duration = "P9999Y"` (`scripts/seed_vtn.py`).
- `lab_core::event_timing::timed_intervals` is the single place that decides when each interval
  runs. It trims to `event.duration`, then repeats the sequence by a fixed elapsed period
  (last end minus first start), capped at `MAX_LOOPED_INTERVALS = 2000`.
- `VTN/bff/src/fleet_signals.rs::bands_for_ven` turns events into per-VEN bands with that rule;
  `VTN/ui/src/utils/fleetTariff.ts` folds them into the tariff series; `FleetTariffChart.tsx`
  draws it.
- The BFF proxies `POST/PUT/DELETE /events` to the VTN untouched. `Events.tsx` +
  `EventFormDialog.tsx` is the existing raw-JSON-ish create path.
- Priority: lower number wins, absent is lowest, ties go to the newer `createdDateTime`
  (`rate_schedule.rs`, BL-02).
- `lab_core::wire_contract::event_quantity_of` already knows `PRICE`/`EXPORT_PRICE` →
  `PricePerKwhEur` and `GHG` → `EmissionGPerKwh`.

## Goals / Non-Goals

**Goals**
- One draft model and one registry that GHG and envelope editing reuse as new registry rows.
- Recurrence authored and read back through `event.duration` only.
- No second copy of "when does interval i run" in TypeScript.

**Non-Goals**
- Drag-on-chart, templates/brush and before/after fleet comparison: change
  `tariff-editor-compare-and-chart`.
- Editing GHG or capacity envelopes (registry rows added later; the `band(min,max)` value kind is
  designed for in the draft type but not implemented here).
- Local-time-aware repetition. The wire has only fixed-period repetition (D5).

## Inventory of existing logic reused or consolidated

| Concept | Where it lives today | This change |
|---|---|---|
| When interval i of an event runs, incl. repetition | `lab_core::event_timing::timed_intervals` | Reused via BFF (D4); no TS copy |
| Events → price bands | `fleet_signals::bands_for_ven` | Reused for draft preview |
| Fold bands into one tariff, dissent detection | `fleetTariff.ts` (`sharedTariff`, `tariffRows`) | Reused unchanged; chart gains a draft series |
| Payload type → quantity/unit | `wire_contract::event_quantity_of` (Rust) | Registry holds UI-facing unit text only; see Open Question 1 |
| Event create call | `api/hooks.ts` create-event mutation | Reused |
| Priority ordering | `rate_schedule.rs` | Read only; the publisher must compute "higher" consistently (D6) |
| ISO 8601 duration parse in UI | none: searched `VTN/ui/src` for `P9999`, `PT`, `iso8601`, `duration`. `EventFormDialog.tsx` passes the string through unparsed and its "Duration" field writes `intervalPeriod.duration` (the per-interval default), not `event.duration` | `recurrence.ts` is the first and only UI parser; the Events form cannot author recurrence, which is why the workbench exists |
| Priority arithmetic in UI | none: `Events.tsx` only displays it; `EventFormDialog.tsx` validates an integer | D6 supersede function is new, sits in `signalDraft.ts` |
| Payload-unit text in UI | none in VTN UI (`Signals.tsx` only has colours per type) | Registry is the first; see Open Question 1 |

## Decisions

**D1 Registry as data.** `signalRegistry.ts` exports a record keyed by payload type:
`{ label, unit, currency, step, min, max, valueKind: "scalar" }`. Editor, validator and
publisher take an entry as a parameter. Alternative: a component per signal — rejected
(`declare-dont-branch`; it is what makes GHG/envelope a rewrite).

**D2 Draft model is the only state.** `signalDraft.ts`: `{ type, intervals: {start, end,
value}[], recurrence }` plus pure functions (`setValue`, `fillRange`, `pasteRows`, `validate`,
`toEventInput`, `fromEvent`). The grid renders it, later the chart edits it. `toEventInput`
cannot produce an event without the registry's descriptors (wire-contracts "make it structural").

**D3 Recurrence as a discriminated union.** `once | span(ISO duration) | forever`, mapped
to/from `event.duration` by `recurrence.ts`. `forever` ⇔ `P9999Y`; any other duration at least as
long as the pattern ⇒ `span`; shorter ⇒ `truncated`. The pattern is stored once; the draft never
holds an expansion.

**D4 Preview expansion comes from the BFF.** New `POST /api/signals/preview` takes a draft
event body and returns bands via the same `timed_intervals` + band code the live fleet view uses.
The chart therefore shows what receivers compute, including the 2000-interval cap. Alternatives:
(a) port the looping to TypeScript — rejected, a second copy of the rule that caused GB-48;
(b) compile `event_timing` to WASM — rejected, build-chain cost for ~40 lines of behaviour;
(c) draw only the pattern once and a "repeats" arrow — rejected, hides DST and truncation
effects. The endpoint's caller is the Tariff page (no half-built feature).

**D4a Loading uses the BFF too (decided during apply).** An event's intervals may omit their own
start or duration (User Guide §7.3: follow the previous interval; default to the event-level
duration). Resolving that in TypeScript would be another copy of "when does interval i run". So
the same endpoint answers `pattern: true` with the event's *unexpanded* timed intervals (made
public from `lab_core::event_timing::base_intervals`), and `draftFromPattern` builds the draft from
those. The workbench always writes explicit starts and durations, so what it publishes never needs
that rule on the way back.

**D5 Fixed-period repetition, stated.** Receivers repeat by elapsed time, so a daily pattern
authored at Europe/Zurich midnight is one local hour off after a DST change. The editor does not
paper over it: it shows the shifted preview and the note required by the spec. Re-publishing the
pattern re-anchors it. Alternative: publish several bounded events per DST period — rejected as
hidden machinery.

**D6 Publish supersedes.** Priority of the new event = (lowest numeric priority among the events
being superseded) − 1, floored at 0; at 0 the newer `createdDateTime` wins by BL-02, which the
publish dialog states. Superseded events are left in place. The Events page already lists them,
and the VTN refuses to delete events that reports reference.

**D7 Where it lives.** New page `/tariff` and nav entry; `FleetTariffChart` gets an optional
`draftBands` prop rendered as a dashed series. The fleet power chart is mounted below on the same
window via the existing `fleetChartWindow.ts`.

## Risks / Trade-offs

- [Draft preview diverges from VEN planning because VENs poll on a cycle] → The page shows
  "published at HH:MM, VENs poll every N s"; real before/after is the follow-up change.
- [Two events targeting different VENs make "the tariff" ambiguous] → Reuse `sharedTariff`
  dissent warning; load-from-live is disabled until the user picks an event.
- [Superseding raises priority forever / stale events accumulate] → Documented in the use-case
  doc; a clean-up affordance is out of scope.
- [A pattern with a gap loops wrongly because shift = last end − first start] → Validated before
  publish (spec: pattern period is visible and valid).
- [Forever + 15-minute grid hits the 2000-interval cap within ~20 days] → Preview shows a
  "receivers expand at most N intervals" note when the cap stops the expansion *inside the asked-for
  window*. Reaching the cap alone is not the signal: any forever series reaches it (2000 hourly
  intervals is 83 days); found by the first preview test.

## Migration Plan

Additive. New route, new BFF endpoint, no wire or schema change; rollback is reverting the
branch. `scripts/seed_vtn.py` is unchanged and remains the way to reset the lab tariff.

## Open Questions

1. (Resolved during apply, 2026-10-18) Unit and currency come from the BFF on `GET
   /api/signals/registry`, derived from `event_quantity_of`. Found while doing task 1.1: the
   PRICE→KWH/EUR table already exists in `wire_contract.rs` and `scripts/seed_vtn.py`, and a
   third copy in TypeScript would be exactly what `one-concept-one-function` forbids. The seed
   script's copy is recorded as a debt row (task 6.2).
2. Assumption A1 (supersede, don't edit in place) was chosen from how a real utility behaves;
   confirm before apply. Changing it to in-place `PUT` alters only task 5.2.
