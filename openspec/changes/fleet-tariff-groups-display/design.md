# Design

## Context

See proposal.md for why. What exists today, found by searching the concept ("which event wins where events overlap" and "what tariff is the fleet under"):

| Concept | Where it lives today | Verdict |
|---|---|---|
| Rank events and resolve overlaps per payload type | `VEN/src/controller/rate_schedule.rs`: `collect_interval_groups` (BL-02 order, `Priority`'s own `Ord`, then `createdDateTime`) and `resolve_segments` (GB-45), used by `parse_rate_snapshots` and `parse_capacity_schedule` | The one implementation. Moves to `lab-core`. |
| Which bands apply to a VEN | `VTN/bff/src/fleet_signals.rs::bands_for_ven` | Resolves targets and timing (`timed_intervals`) but ranks nothing. Its comment says `Priority` has no accessor; the VEN orders by `Priority`'s `Ord`, so none is needed. |
| Interval timing | `lab_core::event_timing::timed_intervals` | Shared already, reused as is. |
| Value and unit of a payload | `lab_core::wire_contract::PayloadReader` | Shared already, reused as is. |
| Fold per-VEN bands to "the tariff" | `VTN/ui/src/utils/fleetTariff.ts::sharedTariff` | Keeps the largest agreeing group only. Replaced. |
| Bands to chart rows | `fleetTariff.ts::tariffRows` | Reused, called once per group. |

`OadrEvent` is the same `openleadr_wire::event::Event` in the VEN and in `lab-core`, so the ranking core can move without a type adaptor.

The signals feed derives from the VTN's events and their `targets`, never from what a VEN has adopted. So the card shows what the VTN is offering, which is what an operator changes; whether a VEN has picked a change up is GB-63 and the plan cards.

## Goals / Non-Goals

**Goals:**
- The tariff drawn beside the fleet power is the tariff each VEN acts on, per VEN group.
- One ranking implementation for the VEN and the feed, pinned by a test that runs both.
- Groups are visible and labelled; nothing about groups needs a new VTN object.

**Non-Goals:**
- Assigning a tariff to VENs from the UI (GB-65).
- The tariff in force at a past time. The feed has no event history; the card says it shows the events as of now.
- Changing what a VEN plans, or the wire to the VENs.
- A per-VEN drill-down of the tariff (the Signals page already lists per-site bands).

## Decisions

**D1. Move the ranking core into `lab-core` (`event_priority`), call it from the VEN and from the feed.**
`collect_interval_groups` and `resolve_segments` become one public function in `lab-core`, taking events, a
`PayloadReader` and the payload types wanted, returning the non-overlapping segments with the winning event
per payload type. `parse_rate_snapshots` and `parse_capacity_schedule` keep their shapes and call it. The
existing VEN tests (`openadr_interface.rs`) stay where they are and keep pinning the behaviour; `lab-core`
gets the core's own tests.
Alternatives: re-implement ranking in the BFF (a second copy, the exact GB-48 failure); have the UI rank
(a third copy, and the UI has no `createdDateTime` tie-break needs); add a `priority` field to the band and
rank client-side (copies the rule into TypeScript). Rejected under one-concept-one-function.

**D2. The core also reports the payloads it skipped as refused.**
A payload whose declared unit the profile does not read contributes nothing in the VEN, so a lower event shows
through. The feed has always shown such a band with `refused` set (GB-50). The core therefore returns the
resolved segments plus the list of skipped payloads (event, type, reason); the VEN ignores that list, the feed
turns it into bands with a null value and `refused`, as today. No behaviour of the VEN changes.

**D3. All payload types in the feed are resolved, not only prices.**
The feed also carries capacity limits and other types that the Fleet page shades. Resolving only PRICE would
make two rules for one feed. After this change a shaded limit window is the limit that wins, as the VEN
applies it. The Signals page reads the same feed for its per-site bands; its tests are rerun and updated where
they assumed overlapping bands.

**D4. Groups are derived in the UI from identical price bands; the VTN has no group object.**
`tariffGroups(signals, window)` fingerprints each VEN's PRICE and EXPORT_PRICE bands (from, to, type, value;
not event id, as `sharedTariff` already does) and returns groups sorted by VEN count, then by the first VEN
name in `compareVenNames` order. Colour is the group's index in a fixed palette from the shared chart theme
(declared once, not per chart), so import and export lines of a group match. VENs with no price band belong to
no group and are counted as not priced.
Alternative: a named group on the VTN (a target-set entity). That is the GB-65 decision and needs the
assignment UI; deriving needs nothing new and works for hand-written events too.

**D5. The short name is the winning PRICE event's name with the leading part shared by all groups cut off.**
Names are cut at a `-` or `_` boundary: `exp-flat-PRICE-g0..g4` become `g0..g4`. Labels read
`<short name> · <n> VEN(s)`; the full event name and the VEN list are on hover. With a single group the full
event name is the label. A group whose winning band has no event name is labelled by its VEN count alone. The
winning event is the one that wins at the window's "now" (or the window start if now is outside it), so a group
whose event changes over the day keeps one label.
Alternative: label by VEN list (unreadable at 6+ VENs); label by the first VEN (misleading).

**D6. Import curves in one chart, export curves in a second chart.**
Five import lines on one axis show the staggering; export lines are secondary. Both read one shared row array per
chart (the tooltip resolves by index, the existing `tariffRows` reason), built once per group and merged by
timestamp. At most eight groups are drawn; the rest are named in a note with their VENs.
The workbench draft (`draftBands`) stays a single dashed pair, drawn beside the live curves on the Tariff page.

**D7. The card states its basis in a caption.**
"VTN events as of now, resolved per VEN" — so the card is not read as the tariff a past plan was made under.

## Risks / Trade-offs

- [The VEN image changes only because a function moved] → The VEN tests are the guard; the move keeps names and
  signatures at the module boundary. The ci-handoff run covers it; no behaviour change is claimed or needed.
- [Resolving every payload type changes shaded windows in the Fleet and Signals pages] → D3; both pages'
  tests rerun; a scenario pins "feed equals VEN".
- [A tariff edited in place is shown at once but VENs may not have replanned (GB-63)] → The caption says
  "VTN events as of now"; the plan cards show what VENs adopted.
- [More than eight tariffs] → Eight drawn, the rest named; the fleet sizes the lab uses stay within it.
- [Past hours are drawn with today's events] → D7 and the Non-Goal; recording tariff history is a separate item
  if wanted.
- [Group identity by value hides two events that happen to price alike] → Intended: they are one tariff for the
  fleet. The hover shows the event name of the group's winning band.
