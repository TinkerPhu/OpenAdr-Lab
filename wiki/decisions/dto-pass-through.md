---
title: "Decision: DTO Pass-Through, No Normalisation"
type: decision
created: 2026-07-04
updated: 2026-07-04
synced_commit: 4695762
sources: [.claude/CLAUDE.md, docs/architecture/VTN_ARCHITECTURE.md]
tags: [decision, dto, api]
---

# Decision: DTO Pass-Through, No Normalisation

Upstream field names — i.e. OpenADR spec names like `programName`, `programID`,
`createdDateTime`, `venName`, `eventName` — are passed through **unchanged** across every
layer: VTN, BFF, VEN backend, and both UIs (`.claude/CLAUDE.md` §dto;
docs/architecture/VTN_ARCHITECTURE.md §2 confirms the VTN side).

## Why

One vocabulary everywhere:

- **Less boilerplate** — no mapping layers to write, test, and keep in sync.
- **Less debugging friction** — a field seen in a network trace, a DB row, a BFF log,
  and a React prop has the same name; grep works end-to-end.
- **Spec traceability** — payloads remain directly comparable to [[openadr-3]] spec
  examples and to `VTN/DTO examples/`.

## Consequences

- UI/TS types mirror spec casing (camelCase JSON keys) rather than local naming taste —
  visible in `VEN/ui/src/api/types.ts` ([[ven-ui]]).
- Internally-defined quantities still follow the unit-suffix rule
  ([[sign-convention]]); pass-through applies to *spec-owned* names, not to local
  physical variables.
- The BFF stays a thin proxy ([[vtn-stack]]) — it adds credentials, never reshapes
  payloads.
