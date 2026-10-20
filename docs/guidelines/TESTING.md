# Testing Guide

## Quick Start

Run everything with one command:

```bash
bash run_all_tests.sh
```

Or pick individual suites:

```bash
bash run_all_tests.sh --local        # UI unit tests only
bash run_all_tests.sh --e2e          # E2E behave integration tests
bash run_all_tests.sh --resilience   # failure recovery tests
bash run_all_tests.sh --rust         # openleadr-rs cargo tests
bash run_all_tests.sh --local --e2e  # combine flags
bash run_all_tests.sh --coverage     # VEN cargo-tarpaulin coverage report (opt-in, see below)
```

### Running the VEN Rust gate without local WSL

`--rust` is **openleadr-rs only**. The VEN's own ~1560 tests run in section 3 of
`run_all_tests.sh`, which is local-only: it shells out to WSL and is *skipped* on a host
without it. On a machine too short of memory for a `cargo` run (WSL's `vmmem` needs well over
a gigabyte on top of whatever else is resident), use the docker harness on a build host
instead — its image already carries `rustfmt` and `clippy`, so overriding the command gives
the whole Rust gate remotely:

```bash
# on the docker host, in /srv/docker/openadr_lab
C=tests/docker-compose.ven-unit-test.yml
docker compose -f $C run --build --rm ven-unit-test                                      # cargo test
docker compose -f $C run --rm ven-unit-test cargo fmt --check
docker compose -f $C run --rm ven-unit-test cargo clippy --all-targets --all-features -- -D warnings
```

The named cache volumes make everything after the first run incremental. Remember the host
lock (`LOCK_HOST=Node2 bash ci/locks/docker_host_lock.sh acquire -m "..."`) — this is a build
on a shared host like any other.

### Coverage (opt-in)

`--coverage` runs the VEN Rust test pyramid under `cargo-tarpaulin` instrumentation and
writes an HTML + JSON report to `coverage/ven/` on the docker host (gitignored, host-local).
It is **never** part of the bare/no-flag "everything" run and has its own docker cache
volumes (`tests/docker-compose.ven-unit-test.yml`'s `ven-coverage` service) — tarpaulin's
instrumented build uses different `RUSTFLAGS` than a plain `cargo test`, so it can't share
the normal `ven-unit-test` cache, and toggling it costs a full recompile either way.

A committed, consolidated snapshot lives at `docs/history/coverage_report_<date>.md`
(regenerate rather than hand-edit — see that file's own "Regenerating" section). Coverage is
not an enforced gate for this project (`.claude/CLAUDE.md`: "no enforced coverage floor —
keep domain and application layer tests meaningful"); this is a visibility tool, not a merge
requirement.

---

## Test Suites

### 1. UI Unit Tests (local)

React component tests using Vitest + React Testing Library. Run locally, no Docker needed.

| Suite | Directory | Files | Command |
|---|---|---|---|
| VEN UI | `VEN/ui/` | see `VEN/ui/src/__tests__/` | `cd VEN/ui && npm test` |
| VTN UI | `VTN/ui/` | see `VTN/ui/src/__tests__/` | `cd VTN/ui && npm test` |

**Prerequisites:** Node.js, `npm install` in each UI directory.

**Generated API types (VEN UI):** `VEN/ui/src/api/generated/` is written from the Rust types (R-133).
After changing a Rust type the UI reads, run `UPDATE_UI_TYPES=1 cargo test ui_types` in `VEN/` and
commit the regenerated files; `cargo test` fails (`generated_ui_types_are_current`) until you do. Then
`npm run build` (its `tsc`) shows every UI place the change reaches.

**Windows note:** If using a subst drive (D: -> C:\DriveD), vitest may resolve paths through the real filesystem. Run from the real path if you get module resolution errors.

### 2. openleadr-rs Cargo Tests (Node1)

Rust unit and integration tests for the VTN library, client library, and wire protocol.

| Package | What it tests |
|---|---|
| `openleadr-vtn` | JWT, database queries, API handlers |
| `openleadr-client` | Client operations, event/program parsing |
| `openleadr-wire` | OpenADR 3 serialization/deserialization |

**Run directly (requires Rust toolchain + PostgreSQL):**

```bash
cd openleadr-rs
docker compose up db --wait -d
cargo sqlx migrate run --source openleadr-vtn/migrations
psql -U openadr -h localhost openadr < fixtures/test_user_credentials.sql
cargo test --workspace
docker compose down
```

**Run offline (no DB, compile-check only):**

```bash
cd openleadr-rs
SQLX_OFFLINE=true cargo test --workspace
```

### 3. E2E Integration Tests (Node1)

Behave (Python) scenarios testing the full stack: VTN, VENs, BFF, and UI through real HTTP calls. Runs inside Docker Compose with an ephemeral database.

Feature files live in `tests/features/` (`ls tests/features/*.feature` for the
current count — hard numbers rot). Coverage:
- VTN auth, programs, events CRUD
- VEN health, integration, sensors, reports
- BFF proxy endpoints
- Enrollment targeting
- VEN isolation (multi-tenant)
- 8 end-to-end use cases (UC1-UC8)
- Browser-based UI tests via Playwright

**Run on Node1:**

```bash
ssh Node1
cd /srv/docker/openadr_lab
docker compose -f tests/docker-compose.test.yml run --build --rm test-runner
docker compose -f tests/docker-compose.test.yml down -v
```

**What keeps a rebuild short.** The VEN and BFF images keep the cargo registry and `target/`
in BuildKit cache mounts, so cargo rebuilds only what changed — a `Cargo.lock` change no longer
rebuilds HiGHS and every other dependency. The E2E stack builds them with the `ci` cargo profile
(`VEN/Cargo.toml`): our own crates at opt-level 1 and incremental, every dependency (the solver
included) at release optimisation; production images build `release`. npm and pip downloads are
cached the same way, and the healthchecks probe every second during start-up. Measured on a
Node-class Pi (2026-10-14): a one-line VEN edit rebuilds the test images in 24 s instead of
265 s; the first build into empty caches takes about an hour. Before each timing-sensitive
pass, `tests/settle.sh` waits until the host's CPU has drained, sampled from `/proc/stat`.

**Run specific tags:**

```bash
# Only UI tests
docker compose -f tests/docker-compose.test.yml run --rm test-runner --tags=@ui

# Only a specific feature: with paths given only they run, each in its pass
# (a path under features/isolated/ in the isolated one), and a retry reruns only its scenario
docker compose -f tests/docker-compose.test.yml run --rm test-runner features/enrollment.feature

# Upstream-pending tests (skipped by default, need PR #365 merged)
docker compose -f tests/docker-compose.test.yml run --rm test-runner --tags=@upstream_pending
```

**Every scenario leaves the VENs as it found them.** `tests/features/helpers/scenario_state.py`
is the one place that knows how to undo a scenario: `api_client`'s mutating VEN helpers report
each call to it, and `after_scenario` puts back what was changed (user requests cancelled, injects
reset, arbiter settings, baseline and comfort overrides cleared, battery SoC and config restored
to their value before the scenario). The default VEN always gets the UI-reachable resets, since
browser scenarios bypass `api_client`. A step that mutates a new VEN route needs an entry in its
`RESETS` table (how to undo) or `NO_RESET` table (why nothing needs undoing); otherwise the log
says `[cleanup] no reset for ...`. What cannot be undone is printed as `[cleanup] LEAK: ...` —
chiefly a started shiftable load, which runs to the end of its duration. So a scenario whose load
must start keeps it short and uses a window only the current slot can satisfy (`... that can only
start in the current slot`), otherwise cost moves the start to a later slot. Physical state a
scenario merely caused (heater temperature, EV SoC) is not forced back: writing a temperature under
a running heater would latch it. `scenario_cleanup.feature` pins the restore.

**Excluded by default** (via `tests/behave.ini`):
- `@upstream_pending` — VEN report isolation tests awaiting upstream fix
- `@resilience` — requires Docker socket access, run separately

### 4. Resilience / Failure Recovery Tests (Node1)

Tests that services recover from restarts and outages. Uses Docker socket to stop/start compose services mid-test.

**4 behave scenarios:**
- VEN retains cached events when VTN goes down
- VEN re-syncs after VTN restart
- Both VENs converge after VTN restart
- VEN recovers after its own restart

**Run:**

```bash
ssh Node1
cd /srv/docker/openadr_lab
docker compose -f tests/docker-compose.test.yml run --build --rm test-runner --tags=@resilience
docker compose -f tests/docker-compose.test.yml down -v
```

**Standalone script** (alternative, runs outside Docker):

```bash
ssh Node1
cd /srv/docker/openadr_lab
# Start the test stack first
docker compose -f tests/docker-compose.test.yml up -d --build
# Run the script
bash tests/failure_recovery_test.sh
# Tear down
docker compose -f tests/docker-compose.test.yml down -v
```

---

## CI on Node3

Node3 tests what sessions hand it and is the only thing that merges to `main`. There is no
GitHub Actions runner: the repository is public, and a runner on the home network that executes
fork pull requests would be an open door. (`.github/workflows/` stays empty; the openleadr-rs
submodule has its own upstream CI.)

```
session / laptop                              Node3
push the branch
ci-handoff.sh  ───── ticket on the board ──►  /srv/ci/board/<ticket>/
ci-wait.sh (background poll, or close         runner, started by a systemd timer every minute:
                                                rebases the commit onto origin/main
                                                quick tier, then full tier
                                                all green and kind=merge: origin/main fast-forwarded
ci-wait.sh / ci-next.sh  ◄─ status + result.md ─  verdict written into the ticket
wakes / resumes the session with the result
```

### Handing a branch over

`ci-handoff.sh` and `ci-next.sh` need `CI_HOST` (the CI host's ssh name; our lab: `export CI_HOST=Node3`) and refuse
to run without it, so a fresh clone never dials a host that does not exist. The suites use `OPENADR_LAB_HOST`
(unset means the local docker). See `ci/README.md`.

Commit everything first, **including the journal and key-learnings entries** (they are tested and
merged with the code), push, then hand off. The hand-off prints a `ci-wait.sh <ticket> --ack` line:
the session runs it as a background command and ends its turn; the session is woken when the ticket
is finished (see "Picking the result up"). Finished work: nothing else to do until then. A
`--kind check` hand-off of an unfinished branch's milestone: keep working on new commits (the ticket
tests the commit it recorded) and do not hand off again while it is in flight:

```bash
git push -f origin HEAD:<branch>
bash ci/ci-handoff.sh                    # test everything, merge if green
bash ci/ci-handoff.sh --kind check       # test only, never merge (A/B runs, experiments)
bash ci/ci-handoff.sh --tiers quick      # audits, fmt, clippy, cargo test only; never merges code
```

The ticket records the session id and the directory the session was started in. The session can
be closed: nothing waits for it, and the result stays on the board. Keeping a session open does
not keep its prompt cache warm (the cache is server-side and expires on its own), and costs RAM.

### Watching the queue

The CI host serves a read-only board on port 8480 (`ci/web/`, `ci-web.service`): queue, running ticket and step,
timeline, step durations, log tails, and an estimated finish time per ticket (median of finished tickets of the
same class, chained through the queue). The same data is at `/api/summary` and `/api/tickets`; the routes are in
`ci/README.md`, the behaviour is pinned by `ci/test_web.py`.

### Suite selection

The runner does not run every suite for every ticket. `ci/suites.map` names, per path pattern, the
suites a change there can affect (`cargo` = the VEN cargo tier, `rust-vtn` = `--rust`, `e2e`,
`resilience`, `all`, or `-` for none); the first matching rule wins and a ticket runs the union over
its changed paths. A path no rule names runs everything (the map ends in `*  all`), so a new
directory is tested in full until someone writes a rule for it. Rules say what a path *cannot*
affect, so write one only when you can name that, e.g. a UI change cannot affect the VEN cargo tier
or the openleadr-rs tests, but it does reach E2E (the stack builds both UIs and drives them in a
browser). The audits and the CI mechanism's own tests (`ci/test_*.sh`) always run.

`result.md` and the CI board show, per ticket, the suites run, the suites skipped and the paths that
decided it. A `--tiers quick` ticket that selected `e2e`, `resilience` or `rust-vtn` is `green`, not
merged: a suite the change needs did not run. Because a wrong skip would pass unnoticed, the
`full-suite-main` job in `jobs.json` runs the full suite on `main` every 7 days.
`run_all_tests.sh` prints the seconds each suite took on its verdict line; read them from `full.log`
to see what a skipped suite would have cost. Behaviour is pinned by `ci/test_board.sh` (map and
selection), `ci/test_runner.sh` (runner and `result.md`) and `ci/test_web.py` (board).

The board can clear tickets: see "Clearing" in `ci/README.md` (token, archive, 24-hour rule). Pinned by `ci/test_web.py`.

### Lanes: the full tier on several hosts

The selected full-tier suites (`e2e`, `resilience`, `rust-vtn`) run on **lanes**: Node3 itself (always
lane 1; its lock is held for the whole ticket) plus the hosts in `CI_LANE_HOSTS`, in that priority
order (our lab: `Node2 Node1` in `/srv/ci/runner.env`, with `CI_HOME_NAME=Node3`). With none configured
there is one lane and the suites run one after the other. The mechanism is `ci/lanes.sh`.

- **Same lock as the sessions.** An extra host is taken with `ci/locks/docker_host_lock.sh`, once and
  without waiting. A host a session holds (`status` shows the holder) is skipped for this ticket and never
  taken over; the board and `result.md` list it under "Not used". Node3, Node2 and Node1 are locked by the same
  script and the same lock path, so a session locks Node3 exactly as it locks the others. One caveat: a
  session holding Node3 makes the ticket wait ("host lock busy"), as before.
- **How many.** At most one extra lane per selected suite beyond the first; one selected suite touches no
  other host. Node1 serves the live stack, so it is last and is skipped when its load is above
  `CI_LANE_MAX_LOAD` (2.0) or its free memory is under `CI_LANE_MIN_MEM_MB` (1500).
- **Who runs what.** Longest suite first, onto the lane that would finish earliest, by the median of that
  suite on that host over the last 10 finished tickets (`board_suite_seconds`; any host's median with fewer than
  3 rows, then a fixed fallback). So E2E lands on Node3, the host without a resident fleet.
- **The commit.** The tested commit is Node3's rebased local commit, so the runner pushes it to
  `refs/ci/<ticket>` on origin (deleted after the ticket); an extra host fetches it and checks it out
  detached under its lock, and is returned to `main` afterwards. A host whose checkout has uncommitted
  changes is skipped, never reset.
- **Failures.** A suite failing a test makes the ticket red and stops the other lanes at once; they are
  tidied (only containers of the `openadr-test` and `openadr-cargo-test` compose projects are removed, never
  production or fleet containers) and unlocked, and `result.md` marks them `cancelled`. A lane that fails
  the infrastructure (ssh drop, pre-flight abort, a suite that reports no PASS) has its suites rerun on
  Node3 afterwards, noted as "moved".
- **Locks cannot outlive the runner.** An extra lane's lease is 45 minutes and is renewed every 15 while the
  lane runs; a runner that dies frees its hosts within 45 minutes.
- **Seeing it.** `result.md` has a `## Lanes` table; the board shows a chip per lane and a log per extra lane
  (`full.<host>.log`). The ticket list has a Node column (where a running ticket works now, or where a finished one
  ran) and each step in the detail chart carries its node, so the durations can be read per host. `ci/install.sh` leaves an example `/srv/ci/runner.env` and warns when a configured
  lane host does not answer over ssh as the CI user.

Pinned by `ci/test_lanes.sh` (planning, acquisition, cancel, infrastructure move, `result.md`) and `ci/test_web.py`.

### Picking the result up

Automatic, while the session lives: `bash ci/ci-wait.sh <ticket> --ack` run in the background polls the
board over ssh (every 30 s, retrying dropped connections, no model turns spent) and exits with the
verdict and the first 80 lines of `result.md`; exit 0 merged/deployed/green, 1 red/conflict/stale/error,
3 gave up (`--max-wait`, default 24 h). Nothing is consumed: the ticket keeps its status and
`result.md`, so if the wait, the session or the laptop is gone the manual route below still works
(`--ack` only hides the ticket from the `ci-next.sh` list). Pinned by `ci/test_wait.sh`.

```bash
bash ci/ci-next.sh            # finished tickets; resumes the session of the one you choose
bash ci/ci-next.sh --list     # only list         --all    every ticket and its status
bash ci/ci-next.sh --new      # a fresh session in the ticket's worktree (cheaper than
                                      # waking a long session just to say "merged")
```

The resumed (or new) session gets the runner's `result.md` as its first message. A ticket is
handed over once. **Merged:** delete the branch (local and origin), remove the worktree, return
any docker host checkout to main. **Anything else:** fix, push, hand off again.

| Status | Meaning |
|---|---|
| `queued` / `running` | waiting for the runner / being tested |
| `merged` | tested after the rebase, `origin/main` fast-forwarded to that commit |
| `green` | tested, not merged (`--kind check`, or `--tiers quick` on code) |
| `red` | a step failed; `result.md` names it and shows the failing scenarios or the log tail |
| `conflict` | the rebase onto `origin/main` conflicts: rebase locally, resolve, push, hand off again |
| `stale` | `origin/main` kept moving under the run (someone merged by hand) three times |
| `error` | the runner could not run the ticket (commit not on origin, push refused, ...) |

### What the runner does

1. Takes the Node3 lock (`LOCK_OWNER=ci-runner`), fetches, checks out the commit and rebases it
   onto `origin/main`. What is tested is exactly what would land; nobody else moves `main`.
2. **Docs-only change** (only `docs/`, `wiki/`, `openspec/`, top-level `*.md`): the five audits,
   then merge. **Code:** the audits, then in `tests/docker-compose.ven-unit-test.yml`
   `cargo fmt --check`, `clippy -D warnings`, `cargo test`, `cargo audit` (VEN and BFF).
3. Full tier unless `--tiers quick`: `run_all_tests.sh --rust --e2e --resilience` on the host.
4. Everything green and `--kind merge`: `git push origin <tested sha>:main`, fast-forward only.
   If `origin/main` moved while testing (a manual merge), the ticket is re-queued; three times is `stale`.

A full run is 55-65 minutes warm, so the runner merges at most about 15-20 branches a day; tickets
queue, oldest first. One ticket never costs an extra run because `main` moved.

### Rules for runs

Every build and test run costs minutes to hours on shared hardware, so start none that is not needed:

- Do not repeat a check that already passed on the same commit.
- Do not start a run while one is in flight. Wait for it, or ask.
- Look for warm caches and volumes before any build that would start cold. A new compose project
  or a renamed volume starts empty: the first runner ticket compiled every dependency twice for
  that reason.
- A change the suites cannot see needs only the suites it can reach: the runner reads the changed
  paths against `ci/suites.map` (see "Suite selection" below). Docs, wiki, openspec, `.claude/` and
  `ci/` need no Rust or E2E run; the runner takes them in minutes.
- The board is one lane: a run that will not merge (A/B, experiment) goes to Node2 by hand, not
  onto the board (`test-host-preference`).
- While working run the narrowest test that proves the change; the full gate belongs to the runner.
- A run lost to the environment (killed shell, restarted WSL, a lock held elsewhere) is understood
  before it is started again.

### On the host, by hand

`run_all_tests.sh` runs on the docker host itself with `OPENADR_LAB_HOST=localhost` (or
`DOCKER_HOST=""`): docker commands run directly, `DOCKER_HOST` is unset for them (it is also the
docker CLI's daemon address), and the host lock is taken. It tests what is checked out there and
never pulls. `LOCK_HELD_BY_CALLER=1` skips the lock for a caller that holds it already.

### Operating it

`bash ci/install.sh` on Node3 (from a checkout of `main`) installs the runner into
`/srv/ci/bin`, the `ci-runner.timer`, and warms the unit-test image. The runner is a snapshot:
changes to `ci/` take effect after the next install, and a ticket that changes them is
tested by the previous version.

- Board and logs: `/srv/ci/board/<ticket>/{request.env,status,result.md,run.log,quick.log,full.log}`.
- Pause: `sudo systemctl stop ci-runner.timer` (a running ticket finishes). Is it alive:
  `systemctl is-active ci-runner.timer`; timer log: `journalctl -u ci-runner`.
- Push access: `/srv/ci/deploy_key` is a write-enabled deploy key for this repository only.
- Tests of the mechanism: `ci/test_board.sh`, `ci/test_handoff.sh`, `ci/test_wait.sh`, `ci/test_deploy.sh`,
  `tests/test_retry_failed.sh`,
  `ci/locks/test_lease_lock.sh`.

Flaky versus red: `tests/entrypoint.sh` re-runs each failed scenario once on its own
(`tests/retry_failed.sh`, pinned by `tests/test_retry_failed.sh`). A scenario that passes on the
retry does not fail the pass; it is named in a `FLAKY_SCENARIO:` log line and the runner lists it
under "Flaky" in `result.md`, and the ticket still merges. One that fails again is red.

### Deploying from the runner

Only after the user said so, a session hands off a deploy instead of a branch:

```bash
bash ci/ci-handoff.sh --deploy Node1 VEN "ven-2 ven-3"     # add --rollback to go back
```

The runner builds release images of `origin/main` (what E2E tested was the `ci` profile) on Node3,
ships them to the target (same CPU architecture on all three hosts), takes the target's lock, keeps
the running image as `:previous`, and switches the services over with `up -d --no-build`
(`ci/deploy.sh`; allowlisted targets Node1/Node2 and stacks VEN, VTN,
VEN/scale_out/node2; pinned by `ci/test_deploy.sh`). Status `deployed` or `red`. Needs an SSH key
of the runner user on Node3 that Node1 and Node2 accept: `ci/install.sh` creates
`~/.ssh/ci_deploy_ed25519` and the `Node1`/`Node2` ssh aliases and prints the public key, which
must be added to `authorized_keys` on both hosts (never commit the key). Rebuilding Node3 means
`ci/install.sh` plus registering the printed GitHub deploy key and these two host keys. This
replaces the scp-based `deploy-node1` skill for tested builds; that skill stays for trying
uncommitted edits.

---

## Test Infrastructure

```
tests/
  docker-compose.test.yml   # 7-service ephemeral test stack
  Dockerfile                # Python test runner (behave + playwright + docker CLI)
  entrypoint.sh             # Loads SQL fixtures, provisions VEN-2, runs behave
  behave.ini                # Default tag exclusions
  requirements.txt          # behave, requests, playwright
  provision_ven2.py         # Idempotent VEN-2 setup via API
  failure_recovery_test.sh  # Standalone resilience script
  nginx-test.conf           # Nginx config for test UI
  features/
    environment.py          # Behave hooks (service waits, Playwright lifecycle)
    helpers/
      api_client.py         # HTTP helpers for VTN, VEN, BFF
      wait.py               # Polling/retry helpers
      ui.py                 # Playwright page object for VTN UI
      docker_ctl.py         # Docker compose stop/start/restart
    steps/                  # 13 step definition files
    *.feature               # 16 feature files
```

### Test Stack Services

| Service | Image | Role |
|---|---|---|
| test-db | postgres:16-alpine | Ephemeral database (no volume) |
| test-vtn | openleadr-rs | VTN server |
| test-ven-1 | VEN app | VEN with 5s polling |
| test-ven-2 | VEN app | Second VEN for enrollment/isolation tests |
| test-ven-no-pv, test-ven-multi, test-ven-penalty, test-ven-usage-sim, test-ven-usage-forecast | VEN app | One profile each (`the VEN is running with profile "..."`), each under its own VTN identity named after the service without `test-` |
| test-bff | BFF app | Dual-credential API proxy |
| test-ui | VTN UI (nginx) | React frontend |
| test-runner | Python test image | Runs behave + playwright |

---

## 5. VEN Rust — 4-Layer Test Pyramid

Required by Constitution Principle VI. All four layers must stay green after every VEN refactoring
phase. They complement the BDD suite — BDD is the safety net; these layers are the test surface that
each refactoring phase *creates*.

| Layer | Location | Framework | What it tests | Speed |
|-------|----------|-----------|---------------|-------|
| 1 — Domain | `#[cfg(test)]` in `entities/`, `controller/` | cargo test | Pure transformations — no I/O, no profile, no simulator | ms |
| 2 — Use case | `#[cfg(test)]` in `services/` + `MockSimulator`/`MockSolver` | cargo test | Service orchestration, trigger conditions, error paths, state transitions | ms |
| 3 — Adapter contract | `#[cfg(test)]` in `simulator/`, `vtn.rs`, `controller/milp/` | cargo test | Real adapter satisfies its port contract (no network — uses fixture JSON) | ms |
| 4 — Integration | `VEN/tests/*.rs` | cargo test / axum `oneshot` | Full HTTP roundtrip with real router, driven adapters real or doubled | s |

**Mock placement rule**: Shared mock adapters live in `VEN/src/services/test_support/`
(not `#[cfg(test)]` — Rust cannot import `cfg(test)` types across module boundaries).

**Test naming convention**: `test_<function>_<scenario>`, e.g. `test_build_setpoints_ev_plugged`.

**Run all four layers**: `cargo test -p ven`

New test surface unlocked per refactoring phase:

| Phase | Functions that become testable |
|-------|-------------------------------|
| 1 — `tasks/` split | `detect_event_changes()` as a pure function; each tick phase in isolation |
| 2 — `SimSnapshot` (controller functions take the flattened snapshot, not `SimState`) | `dispatcher::build_setpoints()`, `apply_surplus_ev_overlay()`, `absorber::apply_deviation_absorption()`, `monitor::record_tick()` (`site_headroom::compute_site_headroom()` moved off this port in a 2026-09-07 follow-up — it needs `&SimState`/`Asset::max_effort_setpoint` directly, not the flattened snapshot) |
| 3 — `AssetMilpContext` + `milp/` split | `build_milp_inputs()`, `translate_solution()`, per-phase constraint builders |
| 4 — Profile decoupled | All domain tests stop loading YAML; `BatteryParams::default()` replaces profile fixture wiring |
| 5 — Services | Full use case suite per service (`PlanningService`, `UserRequestService`, `ObligationService`, `HvacService`) |
