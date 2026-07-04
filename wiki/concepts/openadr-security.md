---
title: OpenADR 3 Security Model
type: concept
created: 2026-07-04
updated: 2026-07-04
synced_commit: eb8831a
sources: [docs/openadr_3_1_specs/]
tags: [security, oauth2, tls, openadr, spec]
---

# OpenADR 3 Security Model

What the spec normatively requires for Authentication/Authorization, transport, and
object-level privacy (Definitions §Security). This is the spec's model; the lab's concrete
implementation of it is [[vtn-stack]]'s dual-credential BFF pattern.

## Three pillars

**Authentication** — every REST request carries a token (servers are stateless, no
sessions). **Authorization** — a VTN limits each requestor to the resources/operations
its identity permits. **Common security model** — OAuth2 client-credentials, not custom
protocols, because VEN client platforms range from cloud services to consumer appliances
where provisioning x.509/PKI per OpenADR 2.0b is impractical (Definitions §Assumptions).

## OAuth2 client-credential flow

`ClientId`/`ClientSecret` (provisioned out-of-band) → `POST` to a token endpoint →
short-lived bearer token → `Authorization: Bearer <token>` on every subsequent request.
`/auth/server` (required) advertises the token endpoint location; `/auth/token` (optional)
may serve it directly, or point to an external auth service. The VTN resolves the token to
a **role** (BL vs VEN) and a set of **scopes**, and rejects out-of-scope requests with 403.

**Scopes** (from the OpenAPI `securitySchemes`):

| Scope | Grants |
|---|---|
| `read_all` | BL: read all resources |
| `read_targets` | VEN: read only objects whose targets match the VEN's granted targets |
| `read_ven_objects` | VEN: read only objects whose `clientID` matches its own |
| `write_programs` / `write_events` | BL only |
| `write_reports` | VEN only |
| `write_subscriptions` / `write_vens` | VEN and BL |

A VTN **MAY** run non-authenticating for public-information programs (e.g. published
tariffs) — no `clientID`/`clientSecret`, no token, open to any client.

## Object Privacy (new in 3.1 — issues 272/287/321/328)

Two mechanisms enforce that a VEN sees only what it owns or has been granted:

- **`clientID` ownership**: a VTN writes the requestor's `clientID` (from the token) into
  `ven`/`subscription`/`report` objects a VEN creates. On read/update/delete of
  `/vens`, `/resources`, `/subscriptions`, `/reports`, the VTN filters to matching
  `clientID` — a VEN cannot enumerate another VEN's objects. BL has unrestricted read
  access.
- **Targeting**: BL grants `targets` (plain strings, e.g. `"ven_0999"`, `"group1"`) to a
  `ven` object; `program`/`event` objects created with matching targets are then readable
  only by VENs holding the intersection. **Target hiding** means a VEN's response never
  reveals targets it wasn't granted (prevents target enumeration). Only BL may write
  targets to `ven`/`resource` objects.

> **DRIFT** The spec's 3.1 `targets` shape is a flat string array (User Guide §8.13
> examples: `"targets": ["ven_0999", "group1"]`) — Change Log issue 316 "refactor targets
> from valueMap to strings". `.claude/CLAUDE.md`'s documented convention for this lab is
> the pre-3.1 shape, `targets: [{type: "VEN_NAME", values: [...]}]` (an array of
> type/values maps), matching the 3.0-generation `openleadr-rs` this lab runs
> ([[openadr-3]]'s version-skew note). This is a concrete field-shape instance of that
> skew — a 3.1 migration must flatten every `targets` payload across VTN, BFF, and both
> UIs, not just add fields. See [[dto-pass-through]] for the pass-through rule this would
> apply to.

## Transport

- **HTTPS/TLS is a MUST** for both VTN and VEN, TLS ≥ 1.2. A VEN **SHOULD** verify the
  VTN's certificate by default and refuse unverified connections (with an opt-out setting
  for private-network self-signed certs). MQTT clients **MUST** use MQTTS.
- This lab's Pi4 stack runs plain HTTP between BFF, VTN, and VENs — tracked as a
  certification blocker in [[vision-and-roadmap]], not duplicated here.

## Webhook hardening (new/tightened in 3.1 — issue 128, 226)

A VTN sending webhook notifications to a VEN's callback URL must: require HTTPS on the
callback URL; verify the callback is live and owned by the requestor via an echo challenge
(`GET` with a random `echo` query param, VEN must reflect it back) before accepting the
subscription. Recommended hardening beyond the MUSTs: reject private/reserved-IP callback
targets and redirects to them (SSRF mitigation), sign payloads with HMAC, publish origin
IPs, and back off / mark endpoints "broken" after repeated delivery failures. The lab's
VEN-side counterpart for consuming notifications (currently poll-based, no webhook
receiver) is tracked as a gap in [[openadr-spec-use-cases]].

## Where this lab's implementation sits

[[vtn-stack]]'s dual-credential BFF (`any-business` vs `ven-manager`) is this lab's
concrete realisation of the BL-vs-VEN role split described here — `any-business` maps to
the BL role (`read_all`, `write_programs`, `write_events`), `ven-manager` to the VEN role
(`write_vens`). `POST /reports` requiring the VEN role specifically enforces
`write_reports`. Token endpoint is `/auth/token` directly (no external auth service, no
`/auth/server` discovery — a cert gap already tracked in [[vision-and-roadmap]]).
