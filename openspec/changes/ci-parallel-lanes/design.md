## Context

The runner (`ci/runner.sh`) is one bash process under a flock: it takes the home host's lock, rebases the ticket's
commit, runs the quick tier and then `full_tier`, which is one `run_all_tests.sh --rust --e2e --resilience` on
localhost. E2E and resilience share the compose project `openadr-test` (`tests/docker-compose.test.yml`), so they cannot
overlap on one host; `rust-vtn` uses `openadr-cargo-test`. Hence the unit of parallelism is the host, not the suite.
Last measured full run: 4893 s on Node3; on Node2 (resident VEN fleet) E2E alone took 5189 s against about 3300 s on
Node3. The hosts are 4-core, 3.8 GB Pi 4B class machines. Node3 reaches Node1 and Node2 over ssh as `pi` (checked
2026-10-18).

## Goals / Non-Goals

**Goals:** shorten a ticket's wall time to roughly its longest suite plus tidy-up; let sessions and the runner compete
fairly through the same lock; fall back to today's behaviour whenever a host is not available.

**Non-goals:** testing several tickets at once (merges serialise anyway; that is the batching change); cancelling lanes
on the first failure; moving the quick tier to lanes; making a suite faster (change 4).

## Inventory (one-concept-one-function)

| Concept | Lives today | Here |
|---|---|---|
| Take / release a host lock | `ci/locks/docker_host_lock.sh` (+ `lib/lease_lock_cli.sh`), wrapped by `acquire_lock`/`release_lock` in `runner.sh` (localhost only) | Reused. The wrappers take a host and a wait (`LOCK_MAX_WAIT_SEC=0` is "try once", already supported). No second lock mechanism. |
| Lock owner | `lease_lock_owner` (`LOCK_OWNER` overrides) | The runner sets `LOCK_OWNER=ci-runner@<ticket>` so `status` names the ticket. |
| Leave a host clean | `tidy_host` (name filter `openadr-test`, compose down, checkout main) | Generalised to a host argument, filters on the compose project label, runs over ssh for a lane. One function for home and lanes. |
| Which suites run | `select_suites`, `suite_flags`, `board_suites` | Reused unchanged; lanes only distribute the result. |
| Run a suite on a host | `run_all_tests.sh` (`DOCKER_HOST`/`OPENADR_LAB_HOST`, `LOCK_HELD_BY_CALLER`, pre-flights, `git pull`) | Reused. One new switch `TREE_PREPARED_BY_CALLER=1` skips its `git pull`, which cannot work on a detached HEAD. |
| Timed step | `run_step` (global `STEPS`, unsafe in a background job) | Lane jobs write their steps to a per-lane file; the parent folds them into `STEPS` after `wait`. |
| Median duration | `expected_duration_s` in `ci/web/ci_web.py` (per ticket class) | A different quantity (per suite). New `board_suite_seconds` in `board.sh` reads suite rows from recent `result.md`. The web estimate needs no change: it learns the shorter wall time by itself. |
| Verdict, flaky and failed sections | `write_result` reads `full.log` | Reads every `full*.log`. |

No logic is copied; the only new concepts are lane planning and lane preparation.

## Decisions

**D1 Lanes and order.** One lane per docker host: the runner's own host (named by `CI_HOME_NAME`, default its hostname;
lab: `Node3`) is always lane 1 and `CI_LANE_HOSTS` lists the others in priority order (default empty, so a fresh clone has
one lane; lab: `CI_HOME_NAME=Node3`, `CI_LANE_HOSTS="Node2 Node1"` in `/srv/ci/runner.env`, which `ci-runner.service`
reads through `EnvironmentFile=-`), giving the priority Node3, Node2, Node1. All hosts use the same `ci/locks/docker_host_lock.sh` and the same lock path, Node3 included; a
session locks Node3 with `LOCK_HOST=Node3` exactly like Node2 (checked 2026-10-18: `status` answers on all three). Node3
differs only in that the runner lives there: its lock is taken first and for the whole ticket (the quick tier and the merge
need Node3's docker and checkout anyway), so it is always lane 1; if a session holds it, the ticket is requeued as today.
Node1 serves the live stack, so it comes last and additionally needs `loadavg1 <= CI_LANE_MAX_LOAD` (default 2.0) and
`MemAvailable >= 1500 MB` when it is acquired, else it is skipped like a busy host.

**D2 Acquire once, at the start of the full tier.** For each host in order, until suites-1 extra lanes are held:
`LOCK_OWNER=ci-runner@<ticket> LOCK_HOST=<h> LOCK_MAX_WAIT_SEC=0 docker_host_lock.sh acquire -l 45`. Exit 2 means
somebody else holds it, so the host is skipped. No waiting, no stealing beyond the lock's own expired-lease rule.
A heartbeat in each lane job runs `refresh -l 45` every 15 minutes, so a dead runner frees its hosts within 45
minutes. The lock is released on every exit path (trap in the lane job).
*Rejected:* waiting for a busy host. A session run can hold a host for hours; waiting would make a ticket slower than today.

**D3 Planning.** `lane_plan` is a pure function: suites, lane hosts and seconds per suite and host in, `host: suites` out.
Longest processing time first onto the lane with the earliest projected end. Node3 is the first lane, so the longest
suite (e2e) lands on Node3, the host without a resident fleet. Seconds come from `board_suite_seconds <suite> <host>`:
the median of that suite on that host over the last 10 finished tickets (rows are named
`run_all_tests.sh --<suite> [<host>]`); with fewer than 3 rows for the host, the suite's median over all hosts; with none,
the fallbacks measured in task 1.1. A suite's length for ordering is its median on the first lane.

**D4 Getting the commit to a lane host.** The tested commit is Node3's rebased local commit and exists on no other
machine. The runner pushes it to `refs/ci/<ticket>` on origin (the deploy key already has write access; the ref lives
outside `refs/heads`, so no branch appears) and deletes it after the ticket. Per lane, under the lock:
`git status --porcelain` must be empty (else the host is skipped; someone's work is never reset), then
`git fetch origin refs/ci/<ticket> && git checkout --detach FETCH_HEAD && git submodule update --init --recursive`.
*Rejected:* pushing into the host's checkout or a bundle over ssh: more moving parts for the same result.
The sparse-checkout completeness check stays in `run_all_tests.sh`; its exit 4 is an infrastructure failure (D6).

**D5 Concurrency inside a bash runner.** Each lane is a backgrounded function `run_lane <host> <suites...>` with its own
log (`full.log` for the home lane, `full.<host>.log` for the others) and a result file (`lane.<host>`: one
`step|exit|seconds` row per suite plus lock wait and outcome). The parent `wait`s for all jobs, then appends the rows
to `STEPS` named `run_all_tests.sh --e2e [Node3]`, so `step_class` still sees `full`. With one lane the code path is
the existing `full_tier` (no subshell, no behaviour change).

**D6 Failures.** A test failure (suite exit 1) makes the ticket red and cancels the other lanes at once. Cancelling
must be safe, because killing a compose run orphans the `test-runner` container (KEY_LEARNINGS "E2E teardown leaves the
runner"): each lane job runs in its own process group (`setsid`); the cancel sends TERM to that group, then runs the lane's
tidy (D7: remove the project's containers by label, `compose down -v`, verify none are left), and only then releases the
lock. The cancelled suites are listed in `## Lanes` as `cancelled`. Tidy is the same function a finished lane uses, so
cancel has no code of its own beyond the TERM. An infrastructure failure of an extra lane (ssh 255, dirty or unreachable host, `run_all_tests.sh` exit 3/4/5, tree
preparation failed) appends its unfinished suites to the home lane, where they run afterwards; `## Lanes` records
"moved from <host>: <reason>". A home-lane infrastructure failure stays `error`, as today.

**D7 Tidy.** After a lane: remove containers labelled `com.docker.compose.project=openadr-test` or
`openadr-cargo-test`, `compose down -v`, verify none remain, `git checkout main && git reset --hard origin/main`, then
release the lock. Never any other container (Node1 production, Node2 fleet).

**D8 Visibility.** `result.md` gets a `## Lanes` table (host, suites, lock wait s, seconds, outcome or moved-from) and the
skipped hosts with the reason (busy by `<owner>`, load, dirty). While running, the runner keeps `$TICKET_DIR/lanes`
(`host|suites|state`); `ci_web.read_ticket` returns it and `index.html` shows a chip per suite with its host. The web
log endpoint accepts `full.<host>`.

## Risks / Trade-offs

- **Node2's fleet slows E2E** -> E2E goes to Node3 by D3; the capacity pre-flight (GB-24) in `run_all_tests.sh` stays on.
- **Node1 runs live services** -> last in order, a load and memory floor, only the test projects are touched; switching it off is one line in `runner.env`.
- **A session grabs a host between our check and our checkout** -> the lock is the check; checkout happens only after `acquire`.
- **Flakes** -> each host is independent; flaky scenarios stay visible in `## Flaky` (all logs are scanned).
- **Median wrong on the first runs** -> fallbacks from the measurement; it corrects itself after ten tickets.
- **Bootstrap** -> the installed runner is the previous version, so this change is tested by the old runner and takes effect after `ci/install.sh` (run as the CI user, not under sudo).

## Migration

Merge with `CI_LANE_HOSTS` unset (nothing changes). On Node3 write `/srv/ci/runner.env` with `CI_LANE_HOSTS="Node2"`,
run `ci/install.sh`, watch one ticket, then add `Node1`.

## Decisions taken with the user (2026-10-18)

Cancel the other lanes on the first failing suite; Node1 is a lane, last (Node3, Node2, Node1); an infrastructure failure
reruns the suite on Node3 afterwards; durations are medians per suite and host. Node3 is locked by the same script as the
others, as one lane among three.

## Open Questions

None.
