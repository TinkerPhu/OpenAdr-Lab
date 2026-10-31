---
title: VEN Code vs Documentation Audit
type: query
created: 2026-07-05
updated: 2026-10-31
synced_commit: 102f7fa
sources: [ci/audits/audit_ven_architecture.py, ci/audits/audit_file_sizes.py, docs/reference/TECHNICAL_DEBTS.md, VEN/src/controller/milp_planner/session_flexibility.rs]
tags: [audit, drift, refactoring, ven]
---

# VEN Code vs Documentation Audit

> Question: does the VEN's code match its documentation, and where should refactoring effort go?

The first answer was a full read of `VEN/src/` in July 2026. Its findings became backlog and debt items, and
every one has since been closed or is tracked in `docs/reference/TECHNICAL_DEBTS.md`, so this page now records
what keeps the answer true rather than a list of findings.

## What keeps code and documentation aligned

- **Ring discipline is enforced, not reviewed.** `python ci/audits/audit_ven_architecture.py` applies the
  dependency rules to production code on every ticket of the Node3 runner ([[ci-pipeline]]): profile and asset
  import bans, no `Utc::now()` below the adapters, no hand-authenticated request, no inline unit conversion, no
  asset kind in the planner core, one ranking of events by priority, and more. The list is in
  [[ven-hexagonal-architecture]]; every violation a review finds becomes a rule.
- **File size is gated.** `ci/audits/audit_file_sizes.py` caps `VEN/src/` at 500 and `tasks/` at 200 production
  lines with an empty allowlist ([[hexagonal-refactoring]]).
- **Duplication and debt are gated.** The duplication ratchet and the debt gate fail a branch that adds a
  duplicated cluster or leaves a small open debt row in files it touched (`docs/reference/TECHNICAL_DEBTS.md`).
- **Vocabulary that is not behaviour is quarantined.** Unbuilt design vocabulary sits in
  `entities/design_vocabulary.rs` under a not-current-behaviour banner; `StaleRatePolicy`, once only
  vocabulary, now drives the planner's stale-rate pricing ([[tariffs-and-capacity]]).
- **The UI cannot drift from the API types.** The VEN UI's TypeScript types are generated from the Rust types
  (ts-rs).

## What this lab's components look like now

Where the July audit found documentation describing something other than the code, the component pages now state
the code: the [[dispatcher]] (a pure base-setpoint function followed by the arbiter and dispatch stages), the
event translation table in [[openadr-interface]] (alerts, dispatch setpoints, export subscriptions and
reservations handled; `CHARGE_STATE_SETPOINT` received and not applied), the asset abstraction in [[asset-layer]]
(a closed `AssetKind` declared by each asset, per-asset MILP and plan parts), the error table in
[[reliability-and-config]], and event `priority` resolution (one function for the VEN and the BFF,
[[tariffs-and-capacity]]).

## Still open

- `milp_planner/session_flexibility.rs` writes the same acceptable-rate band (0.35 / 0.05 €/kWh) and an effectively
  unlimited remaining budget (1.0e9 €) into every `SessionFlexibility` it reports, whatever the session;
  a flexibility figure that claims a price limit nobody set is a wire-contract question as much as a magic number
  (filed in `wiki/review.md`).
