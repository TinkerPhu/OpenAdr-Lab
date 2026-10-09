# OpenADR Lab wire profile

What a number sent by this lab means. Programs point here through a namespaced
`attributes` entry, so a reader can resolve the contract from the object itself rather than
from a convention it has to already know:

```json
{ "type": "openadr-lab.profile",
  "values": ["https://github.com/TinkerPhu/OpenAdr-Lab/blob/main/docs/reference/WIRE_PROFILE.md#v2"] }
```

This profile is **stricter than OpenADR 3.1, never a variant of it**. Everything below either
mandates something the spec leaves optional, or states a convention the spec has no opinion on.
A conformant peer that ignores this document entirely still reads every object correctly.

## v1

### Every payload is declared

OpenADR marks `payloadDescriptors` optional. Here it is mandatory: no event leaves this lab
carrying a payload type it has not declared. The descriptors are *derived* from the payloads
present (`payload_descriptors_for` in `scripts/seed_vtn.py`), so an undeclared payload is not a
thing that can be forgotten — it raises before anything is sent.

| payload type | quantity | `units` | notes |
|---|---|---|---|
| `SIMPLE` | dimensionless level | — | 0–3; the spec defines no unit for it |
| `IMPORT_CAPACITY_LIMIT` | power | `KW` | positive at the grid coupling point |
| `EXPORT_CAPACITY_LIMIT` | power | `KW` | positive at the grid coupling point |
| `CHARGE_STATE_SETPOINT` | state of charge | `PERCENT` | 0–100 |
| `PRICE` | price per energy | `KWH` + `currency: EUR` | `units` is the denominator |
| `EXPORT_PRICE` | price per energy | `KWH` + `currency: EUR` | `units` is the denominator |
| `GHG` | emission intensity | `GHG` | grams CO₂e per kWh |

### What a report value means

The table above is what this lab *sends in events*. Reports are the other
direction, and carry their own `payloadDescriptors`, derived from the payloads
present by `controller::report_payload::descriptors_for` in the VEN:

| report payload type | quantity | `units` |
|---|---|---|
| `USAGE`, `USAGE_FORECAST`, `BASELINE`, `DELTA_USAGE` | energy over the interval | `KWH` |
| `DEMAND` | real power, signed (positive = import) | `KW` |
| `IMPORT_RESERVATION_CAPACITY`, `EXPORT_RESERVATION_CAPACITY` | power | `KW` |
| `STORAGE_MAX_CHARGE_POWER`, `STORAGE_MAX_DISCHARGE_POWER` | power | `KW` |
| `STORAGE_CHARGE_LEVEL` | state of charge | `PERCENT` |
| `OPERATING_STATE`, `DATA_QUALITY` | a label, not a measurement | — |
| `SIMPLE` | 0–3 shed level | — |

`USAGE` is **energy over an interval**, not instantaneous power — the spec says
so in its payload-type table, and it is why a report interval always states the
window its value covers. Until 2026-09-20 this lab sent watts under it, and
`experiments/kpi.py` multiplied by the interval duration to undo that. Both
halves are fixed; `kpi.py` now reads `units` from the report and applies the
watt reading only to rows that declare nothing, saying so when it does.

A payload type absent from the table above is not "dimensionless" — it is one
this lab has not decided the meaning of. The VEN logs it and sends it
undeclared rather than inventing a unit.

### An open report grid is 60 seconds

`reportDescriptor.reportIntervals` says whose grid a report uses. `INTERVALS`
and `SUB_INTERVALS` take it from the event. `OPEN_INTERVALS` means *"the VEN is
expected to generate intervals independent of the event's intervals"* (User
Guide 745) — the spec deliberately leaves the choice to the VEN, so this lab
writes its choice down instead of leaving it implicit:

**An `OPEN_INTERVALS` descriptor is reported on a 60-second grid.**

This exists for the standing `fleet-telemetry` event, which runs for a year and
therefore cannot supply a report grid from its own single interval.
`OPEN_INTERVAL_WIDTH_S` in `controller/openadr_interface.rs` is the one copy of
this number.

### A report interval's width is not its cadence

`frequency` is *"number of intervals that elapse between reports"*. So a 60-second
grid with `frequency: 4` is four 60-second intervals carried by one submission
every 240 seconds — not one 240-second interval every 240 seconds. The two
numbers are `interval_width_s` and `submit_every_s` on an obligation, and
conflating them produced data coarser than the VTN asked for, arriving exactly
when it asked for it.

### Power is kW and energy is kWh, always

OpenADR's `Unit` enum has no watt. That is not a gap to work around: a value in watts is a value
that **cannot be declared**, and under the `wire-contracts` rule anything we cannot declare we do
not send. Power is kilowatts, energy is kilowatt-hours, and a number whose unit cannot be named
on the wire is a bug rather than a formatting choice.

This is the resolution of GB-50, where four parts of this project disagreed about the same
numbers: reports emitted watts, capacity limits were read as kilowatts, a fixture claimed `KW`
for `USAGE` (which the spec defines as energy), and an analysis script carried a `× duration`
compensation for the mismatch.

### State-of-charge setpoints are received, not applied

`CHARGE_STATE_SETPOINT` is "the state of charge of an energy storage resource", which fits a
grid-scale or aggregator-controlled battery. This VEN decides when and how much a household EV
charges, so a grid operator's command about it is neither the driver's plan (it must not become an
`EvSession` competing for the driver's calendar) nor a planner input. It is **received and not
applied**, and is not silent: each event carrying it produces one Info notification naming the
event and the reason. It is deliberately not a wire rejection, because the VTN sent nothing
malformed and `/health` must not read as degraded for a stated policy. The lab still *emits* the
payload in seeded events (above) so a VEN's refusal can be observed.

### `randomizeStart` is honoured, per VEN

`intervalPeriod.randomizeStart` is "the absolute range of client applied offset to start": a VTN
that wants a fleet not to respond on one instant sets it. Each VEN delays every period that
declares it by its own offset within that window.

- **Stable and reproducible, not random.** The offset is a pure function of the VEN's name and
  the event id (`lab_core::event_timing::randomized_start_offset`), so every VEN differs, one VEN
  keeps the same offset across polls and restarts, and tests need no clock or generator.
- **The window moves as a whole.** Start and end shift together, so the response also *ends*
  staggered. All periods of one event take the same offset, which keeps a sequence contiguous.
- **Applies to what the VEN acts on:** alert, SIMPLE and dispatch windows, rates and capacity
  limits are parsed from the shifted copy (`events_this_ven_acts_on`).
- **Does not apply to reporting.** Obligations are read from the declared events, so reports keep
  the cadence the VTN asked for (a request is a guarantee). The declared event, as stored and
  shown by the UI and the BFF, is never changed.
- **Announced.** One Info notification per event states the delay, so a window that opens after
  the VTN's start does not read as a fault.

### Sign convention

Import is positive, export is positive in its own payload type. There is no signed quantity that
means "import when negative" — the direction is carried by the payload type, not by the sign.

### Targets address VENs by name

A program or event targeting `"ven-7"` reaches the VEN whose own `targets` contain `"ven-7"`,
which this lab provisions to equal its `venName`. This is a lab convention, not a protocol rule:
3.1 matches an object's targets against the union of a VEN's targets and its resources', and has
no opinion about what the strings mean. An empty target list means every VEN, per the spec.

### Reports name their event

3.1 makes `eventID` a report's only object link. Reports from this lab always carry one; there
is no program-level reporting.

## v2

v2 adds to v1 and changes nothing in it: an object created under v1 reads the same under v2.

### More payload types are defined

The VEN acted on these before the profile said what unit they are in. They are power, like the
capacity limits:

| payload type | quantity | `units` | notes |
|---|---|---|---|
| `DISPATCH_SETPOINT` | power | `KW` | net site power; positive = import |
| `IMPORT_CAPACITY_SUBSCRIPTION` | power | `KW` | positive at the grid coupling point |
| `EXPORT_CAPACITY_SUBSCRIPTION` | power | `KW` | positive at the grid coupling point |
| `IMPORT_CAPACITY_RESERVATION` | power | `KW` | positive at the grid coupling point |
| `EXPORT_CAPACITY_RESERVATION` | power | `KW` | positive at the grid coupling point |

### Reading what peers send

Sending is strict (above); receiving is explicit. Every event value the VEN acts on, and every
one the VTN UI draws, is read through one reader (`lab_core::wire_contract::PayloadReader`):

1. **The event's own `payloadDescriptors`** say what unit a payload type is in.
2. If the event does not say, **the program's `payloadDescriptors`** do.
3. If neither says, **the default in the tables above is assumed, and the assumption is shown**:
   the VEN's `GET /health` lists it under `wire_assumptions` (payload type and how many payloads
   were read that way in the latest poll) and its Dashboard shows a "Wire assumptions" row. A
   peer is never refused for omitting what OpenADR lets it omit, and an assumption does not make
   the VEN degraded.

A payload that declares a unit or currency **other than the one in the tables** is not used: not
converted, not guessed. OpenADR's `Unit` enum has no watt, so there is no neighbouring unit to
convert from; a declaration that differs is a different quantity. Only that payload type of that
event is dropped; the rest of the event and of the poll are read as usual. The refusal is a wire
rejection: `GET /health` reports `wire_conformance` degraded with the event, the payload type,
what was declared and what this profile reads, and the user is notified once.

A value is never rescaled by its size. `0.8` declared `PERCENT` is 0.8 percent.

A payload type these tables do not list is passed through as written. Nothing in the VEN acts on
it, so there is nothing to assume and nothing to refuse.

## Changing this profile

Add a new version heading and a new `#vN` fragment rather than editing an existing version in
place — objects already on the wire point at the version they were created under. Three tables
state this contract and are pinned to this document by tests, so they cannot drift from it:
`lab_core::wire_contract` (`the_table_matches_the_published_profile`), which the VEN and the BFF
read and the VEN's report builder emits from, and `PAYLOAD_CONTRACT` in `scripts/seed_vtn.py`
(`scripts/test_seed_payload_contract.py`), which the seeder emits from.
