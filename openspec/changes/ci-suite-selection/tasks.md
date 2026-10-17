# Tasks

## 1. Measure and decide

- [x] 1.1 Per-suite minutes: the existing `full.log` files carry no per-suite timing (only the whole step, 3331 s for the last full run), so `run_all_tests.sh` now prints the seconds each suite took on its verdict line; the baseline comes from the next full runs (plan changed, reported to the user)
- [x] 1.2 Confirm each initial rule of design decision 3 against `tests/docker-compose.test.yml`, `tests/Dockerfile*` and `run_all_tests.sh` (what each suite copies/builds); adjust the rule list
- [x] 1.3 File the missing UI unit tests (`vitest`) in `docs/reference/TECHNICAL_DEBTS.md` with Severity, Kind, Cost, Why-open

## 2. The map and the one function (test first)

- [x] 2.1 Write `ci/test_board.sh` cases: VEN-only, UI-only, openleadr-rs-only, docs-only, `ci/audits/`, mixed, unknown top-level path, empty input
- [x] 2.2 Add `ci/suites.map` and `board_suites` in `ci/board.sh`; run the new cases red, then green
- [x] 2.3 Replace `board_docs_only` with the suite set in `board.sh` and its callers; delete the old function and its duplicated cases
- [x] 2.4 Test that every suite in the map is a flag `run_all_tests.sh` accepts and every repo-root directory is covered by a rule or the `*` rule

## 3. Runner

- [x] 3.1 `quick_tier` takes the set: audits and CI self-tests always, VEN cargo steps only with `cargo`
- [x] 3.2 `full_tier` builds the `run_all_tests.sh` flag list from the set; an empty set skips it
- [x] 3.3 `test_and_merge` verdict logic uses the set (docs-only becomes the empty set); keep `--tiers quick` semantics
- [x] 3.4 `write_result` adds the `## Suites` block; extend `ci/test_board.sh` to assert it

## 4. Web board and docs

- [x] 4.1 `ci_web.py` parses the Suites block into `suites_run`, `suites_skipped`; test in `ci/test_web.py`
- [x] 4.2 `index.html` shows run/skipped chips per ticket and in the detail panel
- [x] 4.3 `docs/guidelines/TESTING.md`: how suites are selected, how to change the map, the weekly full run
- [x] 4.4 `jobs.json`: `full-suite-main` (7 days) with a note on how to run it
- [ ] 4.5 Journal and KEY_LEARNINGS entries; run `ci/audits/*` and `ci/test_*.sh`

## 5. Cutover

- [ ] 5.1 Deliver through the same manual cutover as the `ci/` move (the installed runner is the previous version); verify on Node2 with `--kind check` tickets of a UI-only and a VEN-only branch
- [ ] 5.2 After merge: delete this change (waved into TESTING.md), report issues filed, fixed and deferred
