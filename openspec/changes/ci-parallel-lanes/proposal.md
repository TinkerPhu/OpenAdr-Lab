## Why

A full-tier run is `run_all_tests.sh --rust --e2e --resilience` on Node3, one suite after the other: about 81 minutes
for the last measured ticket (4893 s), during which Node3 is the only host working. Node2 and Node1 are
build/test-capable docker hosts that sit idle most of the time, and the three suites are independent of one another.
Second of four sequenced CI-speed changes (suite selection is merged; batching and a shorter E2E follow).

## What Changes

- The full tier runs its suites on **lanes**: one per docker host, in the priority order Node3, Node2, Node1
  (`CI_LANE_HOSTS`). All three are locked with the same lease-lock script and the same lock file path; Node3 is a
  lane like the others and additionally hosts the CI runner. A lane runs its share of the suites one after the
  other, as today.
- Extra lanes are taken with the **same lease lock the sessions use** (`ci/locks/docker_host_lock.sh`), without
  waiting and without ever preempting: a host a session holds is simply not used for this ticket.
- The suites are spread over the lanes that were acquired, longest first, by the median duration of each suite
  on each host (falling back to the suite's median over all hosts).
- The tested commit reaches a lane host as `refs/ci/<ticket>` on origin; the host is put on it under the lock
  and returned to `main` afterwards, as the one-checkout-per-host rule asks.
- A lane failing the infrastructure (host dirty, ssh drop, pre-flight abort) moves its suites to the home lane;
  a suite failing a test makes the ticket red and the other lanes are cancelled at once (their hosts are tidied
  and their locks released), so a red ticket frees the hosts early.
- `result.md` gets a `## Lanes` block; the web board shows where each suite runs and ran.
- `CI_LANE_HOSTS` unset (a fresh clone, a lab with only Node3) means one lane: today's behaviour, unchanged.

## Capabilities

### New Capabilities
- `ci-parallel-lanes`: how the runner spreads a ticket's suites over lock-guarded hosts and reports it.

### Modified Capabilities
None (suite selection stays as it is and feeds this change its suite list).

## Impact

- `ci/runner.sh` (`full_tier`, lane planning, lane runner, `write_result`), `ci/board.sh` (per-suite median),
  `run_all_tests.sh` (skip the `git pull` when the caller prepared the tree), `ci/install.sh` +
  `ci/systemd/ci-runner.service` (`/srv/ci/runner.env`), `ci/web/` (lanes in the ticket view).
- New `ci/test_lanes.sh`; docs in `docs/guidelines/TESTING.md`, `ci/README.md`.
- Node1 and Node2 checkouts are moved to a detached commit and back while the lock is held; a run on Node1 competes
  with the live stack (guarded by a load/memory floor and last place in the order).
