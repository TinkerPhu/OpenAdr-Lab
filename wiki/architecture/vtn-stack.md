---
title: VTN Stack — openleadr-rs, BFF, UI
type: architecture
created: 2026-07-04
updated: 2026-07-04
synced_commit: 4695762
sources: [docs/architecture/VTN_ARCHITECTURE.md, VTN/, openleadr-rs/]
tags: [vtn, bff, openleadr-rs, architecture]
---

# VTN Stack — openleadr-rs, BFF, UI

The server side of [[openadr-lab]] (docs/architecture/VTN_ARCHITECTURE.md).

## Components

| Component | Tech | Port | Role |
|---|---|---|---|
| VTN server | `openleadr-rs` (git submodule, fork `TinkerPhu/openleadr-rs`) | 8200 | OAuth2 server + OpenADR 3 REST: programs, events, reports, VENs, resources |
| Database | PostgreSQL 16 (`vtn-db-1`) | 8201 | 15 tables, SQLx auto-migration on first boot |
| BFF | Rust/Axum proxy | 8220 | Dual-credential pattern (below) |
| VTN UI | React + nginx | 8221 | Operator console |

## The dual-credential BFF pattern

VTN RBAC separates roles: `any-business` can manage `/programs`, `/events`, `/reports`
but **not** `/vens` (403); `ven-manager` is the reverse. No single credential can do both,
so the BFF holds two `VtnClient` instances with independent OAuth tokens (refreshed on 401)
and presents one unified API to the UI. The browser never holds OAuth secrets — it uses
session-scoped API keys (docs/architecture/VTN_ARCHITECTURE.md §3). Rationale recorded in
[[dto-pass-through]]'s sibling rule set; field names pass through unnormalised
(`programName`, `createdDateTime`, …).

One constraint worth remembering: `POST /reports` requires the **VEN role** — reports can
only be created by VENs themselves, never by the BFF's business credential (§3).

## Protocol behaviour

Event distribution is poll-based: operator creates/updates/deletes an event via the UI →
BFF proxies to VTN → each VEN discovers the change on its next 30 s poll and re-plans
(§4.2–4.4). OpenADR 3 has no cancel status — deletion *is* cancellation ([[openadr-3]]).
The VEN-side counterpart is [[openadr-interface]].

Auth facts that cost debugging time (journal-confirmed): token endpoint is
`POST /auth/token` (not `/oauth/token`); token TTL 30 days; fixture users
`any-business`, `ven-manager`, `user-manager`, `business-1`, `ven-1`.
