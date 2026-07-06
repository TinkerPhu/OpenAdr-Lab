---
title: Testing Strategy
type: architecture
created: 2026-07-04
updated: 2026-07-06
synced_commit: ae4a1ed
sources: [docs/guidelines/TESTING.md, tests/features/, .claude/CLAUDE.md, run_all_tests.sh]
tags: [testing, bdd, pyramid]
---

# Testing Strategy

Four suites, all green before any merge to main (`.claude/CLAUDE.md` §testing;
full guide docs/guidelines/TESTING.md). Entry point: `bash run_all_tests.sh` (with
`--local`, `--e2e`, `--resilience`, `--rust` flags).

| # | Suite | Where | What |
|---|---|---|---|
| 1 | UI unit | local (`VEN/ui`, `VTN/ui`) | Vitest + React Testing Library |
| 2 | Rust unit+integration | local WSL (`wsl cargo test -p ven`) | most tests need no HiGHS |
| 3 | E2E BDD | Pi4 docker | behave, ~40 feature files / ~49 scenarios, incl. Playwright UI tests |
| 4 | Resilience | Pi4 docker | failure-recovery scenarios |

## VEN Rust test pyramid

Four layers that must stay green after any VEN change: **Domain → Use-case →
Adapter-contract → Integration**. Shared mock adapters live in
`VEN/src/services/test_support/` (deliberately *not* `cfg(test)` so contract tests can
reuse them). Naming: `test_<function>_<scenario>`.

## Method rules

- **Test-first**: write the failing test before the implementation — at unit level for
  every new function, at BDD level for every new behaviour (`.claude/CLAUDE.md`).
- **Determinism**: every time-dependent code path takes an injectable clock; already the
  norm in the [[milp-planner]] and [[simulator]]. No sleeps, no wall-clock coupling.
- BDD features in `tests/features/` double as executable use-case documentation —
  they are primary sources for [[system-use-cases]] (e.g. `ven_uc_normal.feature`,
  `ven_uc_stress.feature`, `ven_uc_vtn_coordination.feature`).
- The deployment side of the test infrastructure is described in [[deployment-topology]].
