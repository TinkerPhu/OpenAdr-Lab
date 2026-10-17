# Plan: one `ci/` directory, a CI web board, and a repo that runs on one machine

Branch: `refactor/ci-board-web`. Status: approved and implemented on the branch; cutover on Node3 pending (below).

## Goals

1. **Dedicated directory.** All CI/CD lives under a top-level `ci/`, including the new web
   board (API + view). Today it is spread over `scripts/ci/`, `scripts/docker_host_lock.sh`,
   `scripts/wsl_lock.sh`, `scripts/lib/`, five `scripts/audit_*.py` gates and their tests.
2. **CI web board.** Read-only API and single-page view on Node3 showing the queue, running
   ticket and step, history timeline, step durations, logs, and an **estimated finish time for
   every queued/running ticket**.
3. **Runs without Node2 and Node3.** A fresh clone of the public repo works on one machine with
   local docker. Node1/2/3 are one deployment of the lab, never a requirement.

## Target layout

```
ci/
  README.md            what the CI is, how to run it on one machine or on a CI host
  runner.sh board.sh ci-handoff.sh ci-next.sh deploy.sh install.sh
  systemd/             ci-runner.{service,timer}, ci-web.service
  test_*.sh            tests of the mechanism (board, handoff, deploy, web)
  web/                 ci_web.py (stdlib API), index.html (view), no dependencies
  locks/               docker_host_lock.sh, wsl_lock.sh, test_lease_lock.sh, lib/lease_lock_*.sh
  audits/              audit_file_sizes, audit_duplication (+ baseline), audit_debt_gate,
                       audit_ven_architecture, audit_step_definitions, test_audit_*.py
```

`run_all_tests.sh` stays at the repo root (the user-facing entry point). Not moved: the audits
that are not merge gates (`audit_compose_env`, `audit_module_graph`, `gen_module_graph`,
`check_*`) and `jobs.py`; they are not CI mechanism.

## Inventory (one-concept-one-function)

| Concept | Lives today in | Plan |
|---|---|---|
| Finished statuses | `board.sh` `BOARD_FINISHED` | `ci_web.py` reads it from `board.sh`; `test_web.py` pins that |
| Ticket file reading | `board.sh` (`board_get`, `board_status`) and now `ci_web.py` | accepted second reader (python, read-only); the file format is documented in `board.sh` and pinned by `test_web.py` fixtures |
| "which host, local or remote" | `run_all_tests.sh` (`TEST_HOST`, `_DOCKER_IS_LOCAL`), `docker_host_lock.sh` (`LOCK_HOST`), `ci-next.sh` (`CI_HOST`), `deploy.sh` | one rule documented in `ci/README.md`; no hostname default in code (below) |
| Lock script path | `run_all_tests.sh`, `runner.sh` (`$CI_BIN`), `deploy.sh`, docs | `ci/locks/docker_host_lock.sh` everywhere; runner finds it relative to itself |
| Step duration / estimate | `result.md` table only | `ci_web.expected_duration_s` is the single estimator (median per class) |

## Steps

1. **Move** with `git mv` (history kept): `scripts/ci/*` → `ci/`, locks and `scripts/lib/` →
   `ci/locks/`, the five audits, the baseline and `test_audit_*` → `ci/audits/`.
2. **Fix paths inside the moved files:** `REPO_ROOT`/`REPO` (`parent.parent` → `parents[2]`),
   `audit_duplication` baseline lookup, `install.sh` `ROOT`, `test_deploy.sh` `cd`, the
   `source .../lib/lease_lock_cli.sh` lines, `runner.sh` `CI_BIN`.
3. **Rewrite references** (explicit file names, not a blanket `scripts/audit_` pattern, so the
   non-gate audits are left alone): `.claude/CLAUDE.md`, `.claude/skills/ci-handoff`,
   `docs/guidelines/TESTING.md`, `KEY_LEARNINGS`, `TECHNICAL_DEBTS`, `ISSUE_KINDS`, wiki pages,
   `jobs.json`, `run_all_tests.sh`, `tests/retry_failed.sh`, Rust/TS comments that cite a
   script. `docs/history/` is a journal and keeps the old paths.
4. **Installed layout mirrors the repo:** `install.sh` copies the `ci/` tree to
   `/srv/ci/bin/ci/`, so `runner.sh` finds `locks/` and `audits` relative to itself. It also
   installs and enables `ci-web.service` (port 8480, `BOARD_DIR=/srv/ci/board`).
5. **Web board:** done (`ci_web.py`, `index.html`, `test_web.py`); add `ci-web.service`, the
   `test_web.sh` entry in the quick tier's self-test list, and a "CI board" section in
   `docs/guidelines/TESTING.md`.
6. **Standalone run** (below), `ci/README.md`, README "Running on one machine".
7. Journal + KEY_LEARNINGS entries, `audit_file_sizes`/duplication/debt gate green locally.

## Running without Node2 and Node3

- `run_all_tests.sh`: the default host is **local docker** (`OPENADR_LAB_HOST` unset means
  local). Remote hosts are opt-in through `OPENADR_LAB_HOST`/`DOCKER_HOST`, exactly as today
  but without a hardcoded `Node1` fallback. Our own workflow sets `OPENADR_LAB_HOST=Node1` in
  the shell profile (CLAUDE.md `docker:`/`test-host-preference` say so).
- The lock is taken on whichever host is used, including `localhost`, so one machine is
  covered already.
- `DOCKER_DIR=/srv/docker/openadr_lab` stays an overridable variable, not a requirement.
- The sparse-checkout pre-flight already only runs for remote hosts.
- The Node2 fleet capacity check only triggers when `node2` compose containers exist, so it is
  inert on one machine; the fleet (`VEN/scale_out/node2/`) stays an optional extra, documented
  as such.
- The CI runner and the web board are optional: `ci/README.md` shows
  `bash ci/install.sh` for a CI host and `BOARD_DIR=/tmp/board python3 ci/web/ci_web.py` to
  look at a board anywhere; `ci-next.sh`/`ci-handoff.sh` take `CI_HOST` from the environment
  (default stays `Node3` only in our CLAUDE.md, not in the script: **decision needed**, see below).
- Verification: a fresh `git clone` into a temp dir on this laptop, `DOCKER_HOST=""`-free run of
  `bash ci/test_board.sh ci/test_handoff.sh ci/test_web.sh`, the audits, and the quick unit
  tier; the full docker suite is then run on one host without naming Node2/Node3 anywhere.

## Bootstrap problem (the CI cannot test its own move)

The installed runner is the previous version and calls `scripts/audit_*.py`; on this branch
those paths are gone, so a hand-off would come back red. Therefore: test the branch by hand on
Node2 (non-merging run, `test-host-preference`), merge to main with a rebase + fast-forward,
then run `bash ci/install.sh` on Node3 so the runner and `ci-web.service` pick up the new
layout. Sessions with open worktrees must rebase; announce it before merging.

## Decisions (made)

- `CI_HOST` is required by `ci-handoff.sh` / `ci-next.sh`; no `Node3` default in code.
- Estimates are the median per class; docs-only is not predicted at hand-off.

## Cutover on Node3 (the queue must be empty first)

The installed runner keeps working until then: `/srv/ci/bin` is untouched by this branch, and every
ticket of another branch is tested with its own checkout's paths. Merging this branch moves
`scripts/audit_*` away, so a ticket rebased onto the new main would be red under the OLD runner.
Therefore: (1) `ci-next.sh --all` shows no queued/running ticket, (2) `systemctl stop ci-runner.timer`,
(3) merge by hand (rebase + fast-forward), (4) on Node3 `git pull` the checkout and `bash ci/install.sh`
(swaps `/srv/ci/bin/ci` whole, installs `ci-web.service`), (5) `systemctl start ci-runner.timer`,
(6) open `http://Node3:8480`. Rollback: `/srv/ci/bin/ci.old` is the previous tree.

## Out of scope

Authentication on the board (it is read-only and LAN-only), writing to the board from the web
view, notifications.
