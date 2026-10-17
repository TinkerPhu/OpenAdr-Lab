# Design

## Context

`ci/runner.sh` classifies a ticket once: `board_docs_only` (`ci/board.sh`) reads the changed paths and
the runner branches on `DOCS_ONLY` and `TIERS`. Quick tier = audits, CI self-tests, VEN cargo (fmt,
clippy, test, audit VEN, audit BFF). Full tier = one step, `run_all_tests.sh --rust --e2e
--resilience`, where `--rust` is the openleadr-rs submodule's cargo tests (own compose file), `--e2e`
builds the whole stack and runs behave, `--resilience` runs `tests/failure_recovery_test.sh`.
`run_all_tests.sh` already takes each suite as a flag.

### Inventory (one-concept-one-function)

| Concept | Lives today in | This change |
|---|---|---|
| "what does this change need" | `board_docs_only` in `ci/board.sh`; `DOCS_ONLY` and `TIERS` tests in `test_and_merge`, `quick_tier($1)` in `ci/runner.sh` | one `board_suites` function and one map; `board_docs_only` is deleted or reduced to `[ -z "$(board_suites)" ]`; `DOCS_ONLY` replaced by the suite set |
| suite names and flags | `run_all_tests.sh` flags, `full_tier` literal flag list | `full_tier` builds the flag list from the set |
| step timing in `result.md` | `run_step`, `write_result` | reused; add a "Suites" block |
| step reading in the web board | `parse_result` in `ci/web/ci_web.py` | extend; same file format |

## Goals / Non-Goals

**Goals:** fewer minutes for changes with a narrow blast radius; no silent loss of coverage; one place
to read and change the rules.

**Non-Goals:** splitting E2E (change 2/4), batching (change 1), running the UI unit tests in CI (filed
as an issue: the runner has never run `vitest`; adding it needs a node image on Node3), per-scenario
selection inside E2E.

## Decisions

1. **Map as data, `ci/suites.map`.** Lines `pattern<TAB>suite[,suite...]` or `-` for none; shell globs
   on repo-relative paths; the first matching rule wins; the final rule `*` lists every suite. Declared
   as data, not a `case` chain (declare-dont-branch). Suites: `cargo` (VEN cargo tier), `rust-vtn`
   (`--rust`), `e2e`, `resilience`. `audits` and the CI self-tests are not in the map.
2. **Union, conservative.** A path's rule selects suites; the ticket's set is the union. No match falls
   through to `*`. Rules are written as "this path can only affect these suites", never as exclusions.
3. **Initial rules** (derived from `tests/docker-compose*.yml` and the Dockerfiles; confirmed by task 1.2):
   docs, wiki, openspec, `.claude`, top-level `*.md`, `ci/` except `ci/audits/` and `ci/locks/` -> none;
   `VEN/ui/ VTN/ui/ lab-ui/` -> `e2e`; `VEN/ lab-core/` (non-ui) -> `cargo,e2e,resilience`;
   `openleadr-rs/ tests/*openleadr-test*` -> `rust-vtn,e2e,resilience`; `VTN/` -> `e2e,resilience`
   plus `cargo` for `VTN/bff` (audit BFF); `tests/features/` -> `e2e`; everything else -> all.
4. **Quick tier selects too.** The VEN cargo steps run only when `cargo` is selected; audits and
   self-tests always run. `quick_tier` takes the set instead of `$1`.
5. **Reporting.** `write_result` gets a `## Suites` block (`run:`, `skipped:`, `decided by:` first 5
   paths). `ci_web.py` parses it into `suites_run`, `suites_skipped`; `index.html` shows chips per ticket.
6. **Safety net.** `jobs.json` gets `full-suite-main`, interval 7 days: hand off a `--kind check
   --tiers quick,full` ticket of a branch equal to `main`. A job, not an automatic ticket, because a
   human or session must read the result.
7. **Measure first.** Task 1.1 reads `full.log` of existing tickets to get per-suite minutes, so the
   claim "rust-vtn takes X minutes" is a number before the map is trusted for savings.

## Risks / Trade-offs

- A hidden coupling makes a skip wrong (e.g. an env file under `VTN/` read by VEN tests). Mitigated by
  union semantics, the `*` fallback, rules written per directory, the weekly full run, and a unit test
  that every directory in the repo root matches a rule or reaches `*` deliberately.
- Savings are concentrated: only changes confined to one area gain. Doc and CI changes already skip.
  The measured saving is reported in the journal after a week, not promised here.
- Map and `run_all_tests.sh` can drift. The runner builds flags from the set, and a test checks that
  every suite named in the map is a flag `run_all_tests.sh` accepts.
