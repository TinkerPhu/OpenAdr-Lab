---
title: Wire Contract — Declared Units and Meanings
type: concept
created: 2026-10-21
updated: 2026-10-21
synced_commit: f8f6775
sources: [docs/reference/WIRE_PROFILE.md, lab-core/src/wire_contract.rs, lab-core/src/event_timing.rs, tests/features/ven_wire_units.feature, .claude/CLAUDE.md]
tags: [openadr-3, wire, units, payload-descriptors]
---

# Wire Contract — Declared Units and Meanings

A number on the wire is only usable if its quantity, unit and sign are written next to it. OpenADR 3
makes `payloadDescriptors` / `reportDescriptors` optional; this project treats every optional protocol
field as mandatory (`wire-contracts` rule in `.claude/CLAUDE.md`). The profile is **stricter than the
spec, never a variant of it** — a peer that ignores it still reads every object correctly
(`docs/reference/WIRE_PROFILE.md`).

## Emit strictly

Event descriptors are *derived* from the payloads present (`payload_descriptors_for` in
`scripts/seed_vtn.py`), so an undeclared payload cannot be built. Reports likewise
(`controller::report_payload::descriptors_for`). Meanings: capacity limits are power in `KW` (positive
at the grid coupling point), `PRICE`/`EXPORT_PRICE` are per-`KWH` with `currency: EUR`,
`CHARGE_STATE_SETPOINT` is `PERCENT`, `GHG` is grams CO₂e per kWh, `USAGE` is **energy over the
interval** (the spec's reading — an earlier lab convention of sending watts under it is gone, and
`experiments/kpi.py` reads `units` from the report). Programs point at the profile document through a
namespaced `openadr-lab.profile` attribute, so the contract is resolvable from the object itself.

## Accept explicitly

Incoming values are read through one reader, `lab_core::wire_contract::PayloadReader`, which applies
what the event and its program declare. A conformant peer may omit a descriptor: the reader then applies
the documented profile default **and surfaces that it did** — both UIs show the assumption or refusal.
The architecture audit forbids raw `.numeric()` / `PayloadValue::Number` reads of an incoming value
(see [[ven-hexagonal-architecture]]). A request is a guarantee: a peer asking for units or a report
payload type gets exactly that, or a visible failure.

## Where the rules live

`EVENT_QUANTITIES` in `lab-core/src/wire_contract.rs` is the single table; the VEN resolves against it
and the BFF's `/api/signals/registry` exposes it to the Tariff editor ([[vtn-stack]]). Interval timing
is the sibling rule: `lab_core::event_timing` decides when interval *i* runs, including
`event.duration` looping and `randomizeStart` (per VEN, dispatch only). VEN-side parsing uses
`openleadr-wire` types so a field cannot be silently dropped ([[openadr-interface]]). Related:
[[dto-pass-through]], [[openadr-3]].
