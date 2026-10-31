---
title: Testing Strategy
type: architecture
created: 2026-07-04
updated: 2026-10-31
synced_commit: 102f7fa
sources: [docs/guidelines/TESTING.md, tests/features/, .claude/CLAUDE.md, run_all_tests.sh]
tags: [testing, bdd, pyramid]
---

# Testing Strategy

Four suites, all green before any merge to main (`.claude/CLAUDE.md` §testing;
full guide docs/guidelines/TESTING.md). Entry point: `bash run_all_tests.sh` (with
`--local`, `--e2e`, `--resilience` flags; `--rust` is the separate openleadr-rs suite, not the VEN's own tests).
The docker suites (3, 4, the openleadr-rs run and the VEN's `ven-unit-test` image) automatically take the
[[docker-host-lease-lock]] so parallel sessions cannot corrupt each other's stacks on a shared host; the
default host is this machine, remote hosts are opt-in (`DOCKER_HOST` / `OPENADR_LAB_HOST`).

| # | Suite | Where | What |
|---|---|---|---|
| 1 | UI unit | local (`VEN/ui`, `VTN/ui`) | Vitest + React Testing Library |
| 2 | Rust unit+integration | local WSL (`wsl cargo test -p ven-app`) or the `ven-unit-test` docker image (fmt, clippy, test — the runner's quick tier); `VTN/bff` and `lab-core` have their own `cargo test` | most tests need no HiGHS |
| 3 | E2E BDD | docker host (Node2 preferred) | behave, 64 feature files, incl. Playwright browser tests |
| 4 | Resilience | docker host | failure-recovery scenarios (`--tags=@resilience`) |

Suite 3 is the only gate that exercises the **built UI bundles** in a real
browser — bundler-level breakage (e.g. an import-interop bug introduced by a
vite major upgrade) passes vitest and tsc and is caught only here
([[ven-ui]]).

The suites are run by the Node3 runner rather than by hand, with suite selection per changed path and
lanes across hosts — see [[ci-pipeline]]. A failed E2E scenario is retried once and flaky ones are listed
in the ticket's `result.md`. Merge also requires the Python audits (file sizes, duplication ratchet,
debt gate, step definitions, VEN architecture). Wire-level behaviour has its own scenarios
(`ven_wire_units.feature`, [[wire-contract]]).

## VEN Rust test pyramid

Four layers that must stay green after any VEN change: **Domain → Use-case →
Adapter-contract → Integration**. Shared mock adapters live in
`VEN/src/services/test_support/` (`#[cfg(test)]`-gated via `services/mod.rs`).
Naming: `<function>_<scenario>` — no `test_` prefix, redundant inside
`#[cfg(test)]`.

## Method rules

- **Test-first**: write the failing test before the implementation — at unit level for
  every new function, at BDD level for every new behaviour (`.claude/CLAUDE.md`).
- **Determinism**: every time-dependent code path takes an injectable clock; already the
  norm in the [[milp-planner]] and [[simulator]]. No sleeps, no wall-clock coupling.
- BDD features in `tests/features/` double as executable use-case documentation —
  they are primary sources for [[system-use-cases]] (e.g. `ven_uc_normal.feature`,
  `ven_uc_stress.feature`, `ven_uc_vtn_coordination.feature`).
- The deployment side of the test infrastructure is described in [[deployment-topology]].
