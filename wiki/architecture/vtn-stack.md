---
title: VTN Stack — openleadr-rs, BFF, UI
type: architecture
created: 2026-07-04
updated: 2026-10-21
synced_commit: f8f6775
sources: [docs/architecture/VTN_ARCHITECTURE.md, VTN/, openleadr-rs/, VTN/bff/src/, lab-core/src/]
tags: [vtn, bff, openleadr-rs, architecture]
---

# VTN Stack — openleadr-rs, BFF, UI

The server side of [[openadr-lab]] (docs/architecture/VTN_ARCHITECTURE.md). The stack speaks
OpenADR 3.1 ([[openadr-3]]).

## Components

| Component | Tech | Port | Role |
|---|---|---|---|
| VTN server | `openleadr-rs` (git submodule, fork `TinkerPhu/openleadr-rs`, built with `internal-oauth`) | 8200 | OAuth2 server + OpenADR REST: programs, events, reports, VENs, users |
| Database | PostgreSQL 16 (`vtn-db-1`) | 8201 | SQLx auto-migration on first boot |
| Lab broker | Mosquitto `lab-mqtt` | 1884 | VTN subscription notifiers + the fleet channel ([[fleet-monitoring]]) |
| BFF | Rust/Axum proxy | 8220 | One business credential (below), signal workbench, fleet routes |
| VTN UI | React + nginx | 8221 | Operator console: Events, Programs, VENs, Reports, Tariff, Signals, Fleet, Metrics |

## One credential: `bl-client`

3.1 carries scopes on the user object, so one client holds everything the BFF needs
(`read_all write_programs write_events write_vens_bl write_reports_bl write_users`). The 3.0
two-credential split no longer exists. Scopes are written in full: the bare `write_vens` /
`write_reports` are aliases for the *VEN* variant, whose code path overwrites `clientID` with the
caller's token subject — a BFF granted the alias would create every VEN owned by itself and collide on
`ven_client_id_unique`. `bl-client` is the only fixture user; every VEN user is created through the API
with its scopes in the creation call, so no password hash is committed. Token handling is
`lab_core::vtn_auth::ClientCredentials`, the same code the VEN uses: caches the token, re-authenticates
once on 401/403, and is the only place a request is authenticated (an architecture-audit rule). The
browser never holds OAuth secrets. Scope semantics: [[openadr-security]]. Field names pass through
unnormalised ([[dto-pass-through]]).

## Protocol behaviour

Event distribution is poll-based (per-profile interval with startup jitter): operator creates/updates/
deletes an event → BFF proxies → each VEN sees the change on its next poll and replans. OpenADR 3 has no
cancel status — deletion *is* cancellation. Reports carry `eventID` as their only object link (3.1
removed `programID`). VEN side: [[openadr-interface]]; what a value means on the wire: [[wire-contract]].

## BFF modules (`VTN/bff/src/`)

- `signal_workbench.rs` — `GET /api/signals/registry` (units per payload type, from
  `lab_core::wire_contract`) and `POST /api/signals/preview` (a draft event expanded as receivers expand
  it, via `lab_core::event_timing`). Backs the Tariff page's drag-on-chart editor, operators, templates
  and forever-repeating tariffs (`P9999Y` duration); nothing here publishes.
- `fleet*.rs` — live telemetry, trace, plan feed, reactions, objective command: [[fleet-monitoring]].
- `recorder.rs` — optional (`DATABASE_URL`) write-once log of programs/events/reports/VEN snapshots in
  the `lab_recorder.*` schema ([[history-store]]).
- `cache.rs`, `error.rs`, `vtn_client.rs` — TTL cache, error mapping, the VTN client (tested against a
  throwaway axum server, real HTTP).

The BFF depends on `lab-core` for every rule the VEN also needs (interval timing, time series, units,
token handling).
