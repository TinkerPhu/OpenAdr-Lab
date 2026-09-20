# OpenADR Lab wire profile

What a number sent by this lab means. Programs point here through a namespaced
`attributes` entry, so a reader can resolve the contract from the object itself rather than
from a convention it has to already know:

```json
{ "type": "openadr-lab.profile",
  "values": ["https://github.com/TinkerPhu/OpenAdr-Lab/blob/main/docs/reference/WIRE_PROFILE.md#v1"] }
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

### Power is kW and energy is kWh, always

OpenADR's `Unit` enum has no watt. That is not a gap to work around: a value in watts is a value
that **cannot be declared**, and under the `wire-contracts` rule anything we cannot declare we do
not send. Power is kilowatts, energy is kilowatt-hours, and a number whose unit cannot be named
on the wire is a bug rather than a formatting choice.

This is the resolution of GB-50, where four parts of this project disagreed about the same
numbers: reports emitted watts, capacity limits were read as kilowatts, a fixture claimed `KW`
for `USAGE` (which the spec defines as energy), and an analysis script carried a `× duration`
compensation for the mismatch.

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

## Changing this profile

Add a new version heading and a new `#vN` fragment rather than editing v1 in place — objects
already on the wire point at the version they were created under. The `PAYLOAD_CONTRACT` table in
`scripts/seed_vtn.py` is the machine-readable half of this document; the two change together or
the contract is no longer one copy.
