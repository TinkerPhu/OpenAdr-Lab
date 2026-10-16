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
lock (`LOCK_HOST=Node2 bash scripts/docker_host_lock.sh acquire -m "..."`) — this is a build
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

# Only a specific feature
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
close the session                             runner, started by a systemd timer every minute:
                                                rebases the commit onto origin/main
                                                quick tier, then full tier
                                                all green and kind=merge: origin/main fast-forwarded
ci-next.sh  ◄──── status + result.md ───────  verdict written into the ticket
resumes the session with the result
```

### Handing a branch over

Commit everything first, **including the journal and key-learnings entries** (they are tested and
merged with the code), push, then hand off and stop:

```bash
git push -f origin HEAD:<branch>
bash scripts/ci/ci-handoff.sh                    # test everything, merge if green
bash scripts/ci/ci-handoff.sh --kind check       # test only, never merge (A/B runs, experiments)
bash scripts/ci/ci-handoff.sh --tiers quick      # audits, fmt, clippy, cargo test only; never merges code
```

The ticket records the session id and the directory the session was started in. The session can
be closed: nothing waits for it, and the result stays on the board. Keeping a session open does
not keep its prompt cache warm (the cache is server-side and expires on its own), and costs RAM.

### Picking the result up

```bash
bash scripts/ci/ci-next.sh            # finished tickets; resumes the session of the one you choose
bash scripts/ci/ci-next.sh --list     # only list         --all    every ticket and its status
bash scripts/ci/ci-next.sh --new      # a fresh session in the ticket's worktree (cheaper than
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
- A change the suites cannot see (docs, wiki, openspec, scripts no test runs) needs no Rust or E2E
  run; the runner's docs-only path takes it in minutes.
- While working run the narrowest test that proves the change; the full gate belongs to the runner.
- A run lost to the environment (killed shell, restarted WSL, a lock held elsewhere) is understood
  before it is started again.

### On the host, by hand

`run_all_tests.sh` runs on the docker host itself with `OPENADR_LAB_HOST=localhost` (or
`DOCKER_HOST=""`): docker commands run directly, `DOCKER_HOST` is unset for them (it is also the
docker CLI's daemon address), and the host lock is taken. It tests what is checked out there and
never pulls. `LOCK_HELD_BY_CALLER=1` skips the lock for a caller that holds it already.

### Operating it

`bash scripts/ci/install.sh` on Node3 (from a checkout of `main`) installs the runner into
`/srv/ci/bin`, the `ci-runner.timer`, and warms the unit-test image. The runner is a snapshot:
changes to `scripts/ci/` take effect after the next install, and a ticket that changes them is
tested by the previous version.

- Board and logs: `/srv/ci/board/<ticket>/{request.env,status,result.md,run.log,quick.log,full.log}`.
- Pause: `sudo systemctl stop ci-runner.timer` (a running ticket finishes). Is it alive:
  `systemctl is-active ci-runner.timer`; timer log: `journalctl -u ci-runner`.
- Push access: `/srv/ci/deploy_key` is a write-enabled deploy key for this repository only.
- Tests of the mechanism: `scripts/ci/test_board.sh`, `scripts/ci/test_handoff.sh`,
  `scripts/test_lease_lock.sh`.

Flaky versus red: `tests/entrypoint.sh` re-runs each failed scenario once on its own
(`tests/retry_failed.sh`, pinned by `tests/test_retry_failed.sh`). A scenario that passes on the
retry does not fail the pass; it is named in a `FLAKY_SCENARIO:` log line and the runner lists it
under "Flaky" in `result.md`, and the ticket still merges. One that fails again is red.

Not built yet: deploying a tested build to Node1/Node2 from the runner
(`docs/plans/ci-node3-handoff.md`).

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
