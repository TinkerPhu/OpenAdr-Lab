---
title: OpenADR 3
type: concept
created: 2026-07-04
updated: 2026-07-04
synced_commit: 4695762
sources: [docs/REQUIREMENTS.md, docs/openadr_3_1_specs/]
tags: [openadr, protocol, spec]
---

# OpenADR 3

Open standard for communicating demand-response signals between utilities/aggregators
(**VTN**, server) and customer energy systems (**VEN**, client) — the protocol at the heart
of [[openadr-lab]]. Spec entities are used as-is, never redefined locally: Program, Event,
Report, VEN, Resource, Interval, Payload (docs/REQUIREMENTS.md §3.1, citing spec
`docs/openadr_3_1_specs/2_OpenADR 3.1.0_Definition_20250801.md` §5.1–5.5).

## Event types used in this lab

`PRICE`, `EXPORT_PRICE` (€/kWh), `GHG` (gCO₂/kWh), `IMPORT_/EXPORT_CAPACITY_LIMIT` (kW),
`*_CAPACITY_SUBSCRIPTION`/`*_RESERVATION`, `SIMPLE` (levels 0–3), `DISPATCH_SETPOINT`,
`CHARGE_STATE_SETPOINT`, `ALERT_*` — inbound handling per type is tabulated in
[[openadr-interface]]. Report payload types: `USAGE`, `DEMAND`, `BASELINE`,
`STORAGE_CHARGE_LEVEL`, `OPERATING_STATE`, `USAGE_FORECAST`, capacity reservations, etc.
(REQUIREMENTS.md §3.1).

## Facts that matter in practice

- **No cancel status**: events are cancelled by `DELETE /events/{id}`; VENs detect absence
  on the next poll (REQUIREMENTS.md §3.1).
- **Certification profiles** (new in OpenADR 3): *Continuous Pricing* (VEN optimises
  locally on `PRICE`/`GHG`/`ALERT`) and *Baseline Profile* (direct control/dispatch).
  They exist to disambiguate payloads like `SIMPLE`, whose meaning varied by deployment in
  2.0b. This lab uses raw payload types without profile-based certification
  (REQUIREMENTS.md §3.1).
- **ISO 8601 durations**: `M` before `T` is months, after `T` is minutes — `P2M` ≠ `PT2M`
  (REQUIREMENTS.md §2.7).

> **CONTRADICTION** (version skew, intentional): the spec markdown in
> `docs/openadr_3_1_specs/` is **OpenADR 3.1**, which contains breaking changes relative
> to the 3.0-era implementation this lab targets (`openleadr-rs`). Treat spec citations as
> 3.1; treat the running system as 3.0-generation. Migration to 3.1 is a distant goal —
> see [[vision-and-roadmap]].

The DR business context (roles, baselines, M&V) is in [[demand-response]].
