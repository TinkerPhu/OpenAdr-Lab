# Proposal

## Why

Every non-docs ticket runs everything: quick tier (audits, VEN cargo fmt/clippy/test/audit) plus the
full tier (`--rust` openleadr-rs tests, `--e2e`, `--resilience`), about 67 minutes on Node3 of which
E2E is about 55. Many changes cannot affect most of that: a VEN-only change cannot affect the
openleadr-rs unit tests, a UI-only change cannot affect the VEN Rust tests or the resilience suite.
The runner knows only one such case (docs-only). First of four sequenced CI-speed changes
(suite selection, parallel lanes, batching, shorter E2E).

Checked against the code before planning: E2E builds the VEN, VTN, BFF, both UIs and the test runner
(`tests/docker-compose.test.yml`) and has browser steps (`planner_ui_steps`, `ven_ui_*_steps`), so a
UI change DOES need E2E. The saving is therefore not "UI-only skips E2E": it is skipping the
suites that are demonstrably independent of what changed (openleadr-rs tests, VEN cargo tier,
resilience). Also found: the runner never runs the UI unit tests (`vitest`); only E2E reaches the UIs.

## What Changes

- One declarative path-to-suites map (`ci/suites.map`) and one function that turns a changed-path list
  into the set of suites to run. `board_docs_only` becomes the special case "empty set".
- The runner runs only the selected suites, and records which it ran, which it skipped, and the
  paths that decided it, in `result.md` and the ticket.
- The CI web board shows run/skipped suites per ticket.
- Unknown path means all suites (fail safe). A recurring full run of `main` (jobs.json) catches
  anything a skip let through.
- **No change** to what a green merge means: every suite the map selects passed on the rebased commit.

## Capabilities

### New Capabilities
- `ci-suite-selection`: which suites the CI runner runs for a change, how that is declared, how it is
  reported.

### Modified Capabilities
<!-- none: no openspec/specs capability describes the runner today -->

## Impact

`ci/board.sh` (generalise `board_docs_only`), `ci/runner.sh` (`quick_tier`, `full_tier`,
`test_and_merge`), `ci/suites.map` (new), `ci/web/ci_web.py` + `index.html`, `ci/test_board.sh`,
`run_all_tests.sh` (no new flags needed: `--rust --e2e --resilience` already select suites),
`docs/guidelines/TESTING.md`, `jobs.json`. No VEN/VTN code.
