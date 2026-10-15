# Plan: Node3 as build/test/deploy node with a session handoff board

**Status: planned, not started.** Agreed with the user on 2026-10-15, including the merge policy:
the runner is a merge queue and the only thing that moves `main`.

## Context

Every pre-merge check is a manual command today (there is no CI, `.github/workflows/` was
removed). A branch is compiled and quick-tested on the laptop, then the full suite runs on a
docker host for about an hour while the session waits. One branch on 2026-10-14/15 showed where
that breaks:

- The laptop's quick tier failed three times for reasons outside the code: two low-memory kills
  of the background shell and one `Permission denied` removing a cargo temp archive under
  `/mnt/c`. The WSL lock was also held by another session for an hour.
- The session's watcher on the remote run was killed by the low-memory reaper while idle.
- Four full runs were needed for one fix. One failure was real (a wall-clock dependent BDD step);
  the other three re-runs were caused by `main` moving (two large planner merges) while the
  branch was under test, and every rebase hit the same append conflict in
  `docs/history/project_journal.md`.

Node3 (third Raspberry Pi 4B, same hardware class as Node1/Node2, runs nothing else) was set up
and benchmarked for this. Measured, warm, with the build caching merged in `34c80af9`:

| Step | Time |
|---|---|
| Test images after a one-line VEN or lab-core edit | ~24 s |
| openleadr-rs Rust tests | ~80 s |
| Full suite (Rust + 323 E2E + 16 @isolated + resilience) | 55-65 min |
| `cargo test -p ven-app` (1799 tests, after the first cold build) | ~1 min test time |

## Target workflow

1. A session works on a branch as today: `cargo check` and single tests locally while coding.
2. When the branch is done, including its journal and key-learnings entries (they are tested and
   merged with the code), it pushes and **hands off**: it writes a ticket onto the board on Node3
   and closes (or simply stops; nothing depends on it staying alive).
3. The **runner** on Node3 takes tickets oldest first, holds the host lock, rebases the ticket's
   SHA onto the current `main`, runs the quick tier, then (if green) the full tier on that rebased
   commit. Green: it fast-forwards `main` to exactly the commit it tested. It writes the result
   into the ticket and optionally sends a phone push with a one-line result.
4. The user runs **`ci-next`** on the laptop. It lists finished tickets and resumes the session
   that wrote the chosen one, with the result as its first message.
5. Red or rebase conflict: the resumed session fixes and hands off again. Merged: it removes its
   worktree and branch and, only if the user asks, writes a deploy request onto the board.

No step pushes into a session. A session that died, was closed or was reaped loses nothing:
the result waits on the board. Keeping a session open does not keep its prompt cache warm (the
cache is server-side, keyed on content, and expires after its TTL either way; a full run is about
as long as the TTL), and it costs laptop RAM, so sessions should close after handing off.

## Components

### 1. The board (`/srv/ci/board/` on Node3, outside the repo)

One directory per handoff, named `<UTC yyyymmddTHHMMZ>_<session id, 8 chars>_<branch slug>`, so
`ls` order is queue order:

```
request.env   written by the session, atomically (write a temp file, then mv)
status        queued | running | merged | red | conflict | flaky   (runner)
result.md     tiers run, pass/fail counts, failing scenarios, log path, timings   (runner)
ack           written by ci-next before it resumes the session
```

`request.env` holds `SESSION_ID` (the full id, not the 8-character lock form), `RESUME_DIR` (the
directory the session was **started** in, which is where Claude Code stores it, not the worktree
it works in), `WORKTREE`, `BRANCH`, `SHA`, `TIERS` (`quick`, `quick,full`) and nothing else.
Tickets older than 14 days with an `ack` move to `/srv/ci/archive/`.

A deploy request is a ticket with `KIND=deploy`, `STACK` and `SERVICES` instead of tiers.

### 2. The runner (`scripts/ci/runner.sh`, a systemd timer on Node3)

- Every minute: if no ticket is `running`, take the oldest `queued` one.
- Acquire the Node3 lock through `scripts/docker_host_lock.sh` with `LOCK_OWNER=ci-runner`, so
  sessions doing manual work on Node3 and the runner exclude each other. Release it on exit.
- Check out the ticket's SHA (not the branch head: the session asked for that commit) and rebase
  it onto `origin/main`. A conflict ends the ticket as `conflict`; the runner never resolves one.
- Quick tier: `cargo fmt --check`, `cargo clippy --all-targets --all-features -D warnings`,
  `cargo test -p ven-app`, `cargo audit` (VEN and BFF), the five `scripts/audit_*.py`. Runs in
  a `rust:<toolchain>` container with named volumes for the registry and `target/`, the same
  way the 2026-10-15 check ran.
- Full tier: `run_all_tests.sh --rust --e2e --resilience` in host mode.
- A failing E2E scenario is re-run once on its own. Pass on retry → `flaky`, with the scenario
  named in `result.md`; the session must still look at it (a flaky test is a bug in the test or
  the code), but the branch is not reported red for it.
- Both tiers green: `git push origin <tested sha>:main`, fast-forward only (never forced), then
  push the rebased commit to the ticket's branch so the session sees what landed; `merged`.
- Docs-only tickets (no file outside `docs/`, `wiki/`, `*.md`) run the quick tier's audits only and
  skip the hour-long queue position behind code tickets.
- Writes `status` and `result.md`, then the optional phone push (ntfy or similar), which carries
  the result line and the `ci-next` hint.

### 3. `ci-next` (laptop, `scripts/ci/ci-next.sh` plus a PowerShell wrapper)

- `ssh Node3 ls /srv/ci/board`, show finished tickets without `ack`, oldest first:
  `[1] 05:58 green fix/capacity-event-trigger e8cc9d03 (session 2e1cc119)`.
- On a choice: write `ack`, `cd RESUME_DIR`, `claude --resume SESSION_ID "<result line>. Read
  result.md on the board and continue: merge if green, fix if red."`. The exact flag form is
  verified on a throwaway session first.
- `--list` only shows; `--new` starts a fresh session in `WORKTREE` with the ticket as its first
  message, for a session that cannot be resumed or would be expensive to wake (a long transcript
  is re-sent in full on resume).
- The resumed session's first duty: `git fetch` and reset its worktree to what the ticket reports
  (the merged commit, or the rebased commit that failed), so it fixes the code that was tested.

### 4. Deploy (`scripts/ci/deploy.sh <stack> <services>`, run by the runner for `KIND=deploy`)

- E2E tests a `ci`-profile build (`tests/docker-compose.test.yml` passes `CARGO_PROFILE: ci`), not
  the `release` binary production runs. A deploy therefore builds `release` images on Node3 from
  `main`, with the same BuildKit caches.
- Node1, Node2 and Node3 are the same CPU architecture, so the image built once on Node3 is shipped
  (`docker save | ssh <host> docker load`, or a registry on Node3) instead of being rebuilt on a
  production host under load.
- The previous image keeps a `:previous` tag on the target host; rollback is one command.
- Node3 needs an SSH key for Node1 and Node2. It only ever runs commits that sessions pushed to
  this repo's branches; it never runs fork pull requests.

## Prerequisites (each under an hour)

1. `run_all_tests.sh` host mode, found on Node3 2026-10-13:
   - `DOCKER_HOST=<name>` doubles as the docker CLI's daemon address, so on the host itself every
     docker call goes to `tcp://<name>:2375`. Use a different variable for the target host.
   - Local mode skips the lock (`run_all_tests.sh:142`) and the `git pull` (`:311`, `:333`).
2. The two planner goldens fail on ARM on `main` too (`model_fingerprint`, `plans_golden`): they
   compare the order in which objective terms are sent and floats at full precision, and the
   solver library's hash-map order differs between x86 and aarch64. Proposed: keep the `sorted=`
   digest, drop `sent=`, compare floats to ~9 significant digits. This changes the form of a
   test, so it needs the user's agreement before it is done.
3. `.gitattributes`: `docs/history/project_journal.md merge=union`, so two branches appending
   journal entries no longer conflict on every rebase.

## Merge policy: the runner is a merge queue (decided 2026-10-15)

Sessions no longer merge to `main`; the runner does, and nothing else moves `main`. Because it
rebases before testing and nobody can merge while it tests, the commit it tested is exactly the
commit that lands, and each branch needs one full run (unless red). The cost is queue waiting:
with three branches ready at once, the third lands about three hours later.

Rejected:
- **Merge after the fact.** The runner rebases a green result onto the moved `main`, merges
  without a run and tests `main` afterwards. One run per branch, but `main` holds an untested
  combination for about an hour and stays broken for everyone if that run is red.
- **Strict re-run, as today.** Every move of `main` during a run means a rebase and another full
  run. Every merged commit is tested, but one branch on 2026-10-14/15 needed five full runs, four
  of them only because `main` moved.

## Throughput limit

One full tier at a time, ~1 hour each: about 15-20 merges a day. Enough for this project's merge
rate; on a busy day tickets queue, but a move of `main` never costs an extra run.

## What this reuses

- `scripts/lib/lease_lock_core.sh` and `scripts/docker_host_lock.sh`: the lock, unchanged.
- `run_all_tests.sh`: the full tier, after prerequisite 1.
- `tests/settle.sh`: the load-settle gate between sections.
- The BuildKit caches and the `ci` profile (VEN/BFF Dockerfiles, `[profile.ci]`).
- The ad-hoc scripts used for the 2026-10-14/15 runs exist only on Node3 (not in the repo); the
  runner replaces them.

## Order of work

1. Prerequisites 1 and 3; prerequisite 2 once agreed.
2. Board, runner (quick and full tier, rebase, fast-forward merge), `ci-next` without push.
   Until this lands, sessions keep merging by hand as today.
3. Phone push.
4. Deploy.
5. Then fold the result into `docs/guidelines/TESTING.md` and the CLAUDE.md workflow, and delete
   this plan.
