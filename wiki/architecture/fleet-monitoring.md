---
title: Fleet Monitoring and Control Channel
type: architecture
created: 2026-10-21
updated: 2026-10-21
synced_commit: f8f6775
sources: [lab-core/src/fleet_channel.rs, VTN/bff/src/fleet.rs, VTN/bff/src/fleet_command.rs, VTN/bff/src/fleet_plan.rs, VTN/bff/src/fleet_reactions.rs, VTN/bff/src/fleet_store.rs, docs/architecture/VTN_ARCHITECTURE.md, VTN/ui/src/pages/Fleet.tsx]
tags: [fleet, mqtt, bff, telemetry, lab-core]
---

# Fleet Monitoring and Control Channel

With N=20 VENs ([[fleet-tooling]]), the question "what is every site doing and intending right now"
cannot be answered by asking twenty VENs. The fleet channel answers it from one place: the lab's own
MQTT broker, read by the BFF ([[vtn-stack]]), shown on the VTN UI's Fleet page.

## The channel

`lab-mqtt` (Mosquitto, host port 1884) is separate from the house broker on :1883, which carries what the
site *measures* ([[real-measurement-mqtt]]); the lab broker carries what the lab *generates*. Topic shape
`openadr-lab/fleet/<venName>/<leaf>` is declared once in `lab-core/src/fleet_channel.rs`, used by both
the VEN publisher and the BFF subscriber:

| Leaf | Content | Delivery |
|---|---|---|
| `telemetry` | live site state | QoS 0, retained |
| `trace` | controller decisions (what it saw, when it replanned) | QoS 1 |
| `status` | `online`/`offline`; the broker publishes `offline` as the last will | retained |
| `plan` | the adopted plan, a projection of `GET /plan` | |
| `objective` / `command/objective` | the VEN's declared objectives and current one / the BFF's command | retained |

Authorisation is per client: each VEN may only read/write its own subtree through one ACL pattern
(`openadr-lab/fleet/%u/#`), the BFF may only **read** `fleet/#` plus publish the objective command, so a
spoofed reading from one VEN is impossible. Passwords are derived (HMAC of a root secret and the VEN
name) so a twenty-first VEN needs no broker edit (docs/architecture/VTN_ARCHITECTURE.md §6).

## What the BFF does with it

- **Live state** (`fleet.rs`): holds the latest message per VEN. Deliberately lossy; the VTN's reports
  remain the durable record. It stores what a VEN said and interprets nothing — a site's capability or
  forecast is the site's to state (`asset-competence-assurance`, see [[asset-layer]]).
- **Storage** (`fleet_store.rs`): telemetry, trace and plan writers share one batch loop and write to
  `lab_recorder.*` ([[history-store]]).
- **Reactions** (`fleet_reactions.rs`): for one published event, when each VEN said it saw it, when it
  next replanned, and its mean power a minute before and after. No threshold decides "reacted" — the
  reader judges.
- **Plan feed** (`fleet_plan.rs`): the fleet's *intended* import/export as of any instant, summed over
  the latest plan per VEN with `lab_core::time_series`; "now" and "an earlier instant" are one function so
  the tariff editor's before/after comparison cannot add the two sides differently. A VEN is absent past
  its plan's horizon, never zero.
- **Objective command** (`fleet_command.rs`): publishes `command/objective` retained; the VEN applies it
  through its own setter and reports back on `objective`, so "commanded" and "reported" both come from the
  broker. A command is valid only against the objectives the VEN itself declares. This is how the fleet is
  switched between planning and the [[opportunistic-objective]].

## UI surface

The Fleet page (`VTN/ui/src/pages/Fleet.tsx`) shows per-VEN power curves on one axis, what the fleet was
charged beside what it did, day/night shading, the dashboard health card for the feed, and a before/after
planned-energy comparison for the newest fleet-wide objective switch (the same `PlanComparison` the Tariff
page uses). Scenarios: `tests/features/fleet_telemetry.feature`, `fleet_plan_feed.feature`,
`fleet_objective_control.feature`.

## lab-core: one rule for both services

`lab-core/` is the crate the VEN and the BFF both depend on, holding rules that are *the same question on
both sides*: `event_timing` (when does interval i of an event run), `time_series`/`time_window`,
`units`, `vtn_auth` (`ClientCredentials`, the only code that attaches a token), `wire_contract`
([[wire-contract]]) and `fleet_channel`. It exists because GB-48 found that question decided in seven
parsers with four rules. What does not belong: anything either side is sole authority for.
