# Tasks

## 1. Measure and check

- [ ] 1.1 Read the per-suite seconds of the next full-tier tickets (the verdict lines of `full.log` carry them since suite selection), or run `--rust`, `--e2e`, `--resilience` once each on Node2 under its lock; record e2e / resilience / rust-vtn medians for Node3 (and Node2 if run) in design.md as the fallbacks. Verify: design.md D3 names three numbers.
- [ ] 1.2 Check `ssh -o BatchMode=yes Node2 true` and `Node1` as the CI user on Node3, and that `/srv/docker/openadr_lab` there is a clean checkout; write the result into design.md Risks. Verify: no "check" left.

## 2. Pure parts (test first, `ci/test_lanes.sh`)

- [ ] 2.1 Failing tests, then `board_suite_seconds <suite> <host>` in `ci/board.sh`: median of that host's rows of the last 10 finished `result.md`; the all-host median when the host has fewer than 3 rows; the fallback when none. Verify: `bash ci/test_lanes.sh` section "medians" green (three cases).
- [ ] 2.2 Failing tests, then `lane_plan` (longest first onto the earliest-finishing lane by each host's own seconds; one suite means one lane; more hosts than suites). Verify: the three "spread longest first" scenarios green.

## 3. Lane lifecycle (test first; ssh and lock stubbed as in `ci/test_handoff.sh`)

- [ ] 3.1 Generalise `acquire_lock`/`release_lock`/`tidy_host` in `ci/runner.sh` to a host (the runner's own host keeps `localhost` and is taken first, waiting as today); try-once mode with `LOCK_MAX_WAIT_SEC=0` for the others; owner `ci-runner@<ticket>`; Node1 load/memory floor. Verify: existing `ci/test_runner.sh` green and new tests: busy host skipped without waiting, owner string in the lock, foreign containers untouched, a session-held Node3 lock requeues the ticket.
- [ ] 3.2 `prepare_lane_tree <host> <ticket> <sha>`: push `refs/ci/<ticket>`, dirty check, fetch plus detached checkout plus submodules; `run_all_tests.sh`: `TREE_PREPARED_BY_CALLER=1` skips its `git pull`. Verify: tests for a dirty host (skipped, nothing reset) and for run_all_tests not pulling; `bash -n run_all_tests.sh`.
- [ ] 3.3 `run_lane` (background job, per-lane log and result file, heartbeat refresh, trap release) and `full_tier` using it when more than one lane was acquired; one lane keeps the existing path. Verify: a two-lane test with stub suites gives the folded `STEPS`, both logs, both locks released, also when a suite fails and when the job is killed.
- [ ] 3.4 Infrastructure failure moves the suites to the home lane (D6). Verify: stub ssh exit 255 reruns them at home and the verdict follows their result; stub suite exit 1 gives red and is not moved.
- [ ] 3.5 Cancel on the first failing suite: lane jobs in their own process group, TERM, then the shared tidy, then lock release; cancelled suites recorded. Verify: a test where lane A fails at once and lane B is a long stub: B is gone within seconds, its tidy ran before its lock was released, `result.md` marks it cancelled; a stub that leaves a labelled `test-runner` container proves tidy removes it and the check finds zero.

## 4. Result and board

- [ ] 4.1 `write_result`: `## Lanes` block, skipped hosts with reasons, scan `full*.log` for flaky and failed. Verify: `ci/test_runner.sh` cases for one and two lanes.
- [ ] 4.2 `lanes` file while running; `ci_web.read_ticket` returns lanes, `index.html` shows a chip per suite with its host, `log/full.<host>`. Verify: `ci/test_web.py` cases; look at the board with a fixture ticket.

## 5. Configuration and rollout

- [ ] 5.1 `ci/systemd/ci-runner.service` gets `EnvironmentFile=-/srv/ci/runner.env`; `ci/install.sh` leaves an example file and warns when a configured lane host does not answer ssh. Verify: `ci/test_deploy.sh` still green; install on Node3 as the CI user.
- [ ] 5.2 Verification on the real hosts: one hand-made `--kind check` ticket on a branch touching `VEN/src` with `CI_LANE_HOSTS="Node3 Node2"`; compare wall time with the single-lane median; confirm Node2's lock and checkout are back afterwards. Then `"Node3 Node2 Node1"` once with Node2's lock held by hand (Node1 must be used, Node2 untouched), and once with a deliberately failing suite to see the cancel and the tidy on both hosts. Verify: all three results in the journal entry.
- [ ] 5.3 Docs: `docs/guidelines/TESTING.md` (Lanes: order, lock, config, what a skipped host means), `ci/README.md`, runner header comment; journal and KEY_LEARNINGS; delete this change per the workflow rule. Verify: `python ci/audits/audit_debt_gate.py` green.
