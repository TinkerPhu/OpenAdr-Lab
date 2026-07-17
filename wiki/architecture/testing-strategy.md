---
title: Testing Strategy
type: architecture
created: 2026-07-04
updated: 2026-07-17
synced_commit: f08e469
sources: [docs/guidelines/TESTING.md, tests/features/, .claude/CLAUDE.md, run_all_tests.sh]
tags: [testing, bdd, pyramid]
---

# Testing Strategy

Four suites, all green before any merge to main (`.claude/CLAUDE.md` §testing;
full guide docs/guidelines/TESTING.md). Entry point: `bash run_all_tests.sh` (with
`--local`, `--e2e`, `--resilience`, `--rust` flags). The remote docker suites
(3, 4, and Rust-in-docker) automatically take the [[pi4-lease-lock]] so parallel
sessions cannot corrupt each other's stacks on the shared Pi4.

| # | Suite | Where | What |
|---|---|---|---|
| 1 | UI unit | local (`VEN/ui`, `VTN/ui`) | Vitest + React Testing Library |
| 2 | Rust unit+integration | local WSL (`wsl cargo test -p ven-app`; `VTN/bff` has its own `cargo test`) | most tests need no HiGHS |
| 3 | E2E BDD | Pi4 docker | behave, 48 feature files / ~262 scenarios, incl. Playwright browser tests |
| 4 | Resilience | Pi4 docker | failure-recovery scenarios (`--tags=@resilience`) |

Suite 3 is the only gate that exercises the **built UI bundles** in a real
browser — bundler-level breakage (e.g. an import-interop bug introduced by a
vite major upgrade) passes vitest and tsc and is caught only here
([[ven-ui]]).

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
