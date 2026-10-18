## ADDED Requirements

### Requirement: Extra lanes are taken through the host lock, never waited for
The runner SHALL acquire an extra lane host with `ci/locks/docker_host_lock.sh` (the lock sessions use) in the
order `CI_LANE_HOSTS` gives, trying each once without waiting, and SHALL NOT take a host whose lock another owner
holds.

#### Scenario: First host busy, second free
- **GIVEN** `CI_LANE_HOSTS="Node2 Node1"`, a session holds Node2's lock, Node1's lock is free, three suites are selected
- **WHEN** the full tier starts
- **THEN** Node1 is acquired as the second lane, Node2's lock is untouched, and the run does not wait for Node2

#### Scenario: Both hosts busy
- **WHEN** both extra locks are held by sessions
- **THEN** every selected suite runs on Node3 one after the other, exactly as before, and `result.md` lists the busy hosts as skipped

#### Scenario: No lane hosts configured
- **WHEN** `CI_LANE_HOSTS` is empty or unset
- **THEN** no lock other than Node3's is touched and the run is identical to a single-lane run

### Requirement: Only as many lanes as help
The runner SHALL acquire at most one extra lane per selected suite beyond the first, and SHALL NOT lock a host it has no suite for.

#### Scenario: One suite selected
- **WHEN** only `e2e` is selected
- **THEN** no extra host is locked

### Requirement: Suites are spread longest first
The runner SHALL assign each selected suite, longest first, to the acquired lane whose projected finish is earliest, using
the median seconds of that suite over the last finished tickets and a documented fallback for a suite never timed.

#### Scenario: Three suites, three lanes
- **GIVEN** medians e2e 3300 s, resilience 900 s, rust-vtn 600 s
- **THEN** e2e runs on Node3, resilience on the first extra lane, rust-vtn on the second

#### Scenario: Three suites, two lanes
- **THEN** e2e runs alone on Node3 and resilience then rust-vtn run on the extra lane

### Requirement: A lane host is prepared and returned under its lock
Before running on an extra host the runner SHALL fetch the tested commit from `refs/ci/<ticket>` on origin and check it
out detached, and SHALL refuse the host if its working tree has uncommitted changes. After the lane it SHALL remove the
containers of the `openadr-test` compose project (and no other container), check none are left, return the checkout to
`main`, release the lock and delete the ref.

#### Scenario: Dirty checkout
- **WHEN** the host's tree has uncommitted changes
- **THEN** the lane is not used, nothing is reset, and `result.md` says why

#### Scenario: Foreign containers are never touched
- **GIVEN** the host runs production or fleet containers
- **THEN** only containers of the `openadr-test` compose project are removed

### Requirement: A lane that fails the infrastructure does not fail the ticket
If an extra lane cannot run its suites for a reason outside them (ssh drop, tree preparation, pre-flight abort 3/4/5),
the runner SHALL run its suites on the home lane after the home lane's own and record the move.

#### Scenario: ssh drops mid-lane
- **WHEN** the connection to Node2 fails with exit 255 during its suite
- **THEN** that suite is rerun on Node3 and `result.md` records "moved from Node2: infrastructure"

### Requirement: Any suite failing makes the ticket red, and every lane finishes
The verdict SHALL be red when any suite fails on any lane, and the runner SHALL let the other lanes finish so one run
reports all failures.

#### Scenario: One lane red, one green
- **WHEN** resilience fails on Node2 while e2e passes on Node3
- **THEN** the verdict is red, `result.md` shows both outcomes, and nothing merges

### Requirement: Locks cannot outlive the runner
A lane lock SHALL carry a short lease that a heartbeat renews while the lane runs, and SHALL be released on every exit path.

#### Scenario: Runner killed
- **WHEN** the runner dies during a lane
- **THEN** the host's lock counts as dead after its lease and the next acquirer takes it

### Requirement: Lanes are visible
`result.md` SHALL list each lane's host, suites, lock wait, seconds and outcome, the web board SHALL show the same for a running and a
finished ticket, and each lane's log SHALL be readable.

#### Scenario: Finished two-lane ticket
- **THEN** `result.md` has a `## Lanes` block with one row per lane and the board shows a chip per suite with its host
