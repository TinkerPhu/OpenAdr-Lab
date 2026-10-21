---
title: Vision and Roadmap
type: overview
created: 2026-07-04
updated: 2026-07-04
synced_commit: 474dec3
sources: [docs/BACKLOG_OpenADR_Cert.md, docs/BACKLOG.md, docs/history/project_journal.md]
tags: [vision, roadmap, certification]
---

# Vision and Roadmap

Where [[openadr-lab]] is headed, beyond the current branch.

## Long-term directions

- **Swarm behaviour** — grow beyond three VENs to study how many small HEMS sites behave
  collectively under shared DR signals (stated intent; no implementation yet — unbuilt
  ideas live in `docs/BACKLOG.md`, per wiki policy).
- **OpenADR certification readiness** — `docs/BACKLOG_OpenADR_Cert.md` audits the VEN
  against the OpenADR 3.1 certification requirements. Known blockers include: plain-HTTP
  transport (no TLS — "certification blocker", §2), no `/auth/server` token-endpoint
  discovery (§3), no mDNS discovery (§1). Registration/identity and OAuth basics are
  largely covered. Full spec security model (scopes, object privacy, webhook hardening,
  the TLS MUST): [[openadr-security]].
- **OpenADR 3.1 conformance** — the stack itself runs 3.1 (migrated 2026-09). Three VEN-side
  MUSTs are open: TLS, subscriptions/webhooks and `/auth/server` discovery; see `docs/plans/strategic_roadmap.md` §3.4a and [[openadr-3]].
- **Upstream contributions** — the VTN runs a fork (`TinkerPhu/openleadr-rs`) of the
  upstream `OpenLEADR/openleadr-rs`; improvements worth generalising get PR'd upstream
  after full local test passes (`.claude/CLAUDE.md` upstream rules).
- **Spec-implied use cases** — [[openadr-spec-use-cases]] derives what the spec expects
  a VEN to handle and gap-checks it against the code base (three gap clusters: transport
  modernisation, report-management depth, event arbitration). Operational catalogue:
  [[system-use-cases]].

## How the wiki tracks this

This wiki itself is part of the infrastructure: it stays current via the git-anchored
sync workflow described in [[wiki-maintenance]].

> **OPEN QUESTION** How far should the lab go toward certification-grade behaviour
> (TLS, MQTT transport, discovery) given its lab scope? (docs/BACKLOG_OpenADR_Cert.md)
