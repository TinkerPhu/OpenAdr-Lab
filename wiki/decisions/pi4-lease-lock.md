---
title: Pi4 Lease Lock
type: decision
created: 2026-07-17
updated: 2026-07-31
synced_commit: e9f5207
sources: [scripts/pi4_lock.sh, scripts/wsl_lock.sh, run_all_tests.sh, .claude/CLAUDE.md]
tags: [pi4, docker, concurrency, dev-workflow]
---

# Pi4 Lease Lock — serializing the shared docker host

Multiple AI sessions work in parallel worktrees (the pattern kept from
[[superpowers-not-adopted]]), and most tasks end in a docker build or E2E run on the
single shared Pi4 described in [[deployment-topology]]. Concurrent
`docker compose build/run` invocations there corrupt each other's stacks and produce
false test failures. Since 2026-07-17, `scripts/pi4_lock.sh` provides a cooperative
**lease lock** that every Pi4 docker sequence must hold (`.claude/CLAUDE.md`
§pi4-lock).

## Decision: lease lock on the resource, not a queue file

The alternative considered was a queue/board file where each task appends a line and
waits until its entry is first. Rejected for two reasons
(docs/history/project_journal.md §"Pi4 lease lock"):

1. **Crash behaviour.** A killed session leaves its queue entry at the head and
   deadlocks everyone behind it — so every entry would need its own expiry anyway,
   at which point the queue is a more complex lock.
2. **Visibility.** Worktrees are separate checkouts; a file in one is invisible to
   the others. The lock therefore lives *on the Pi4* (`/tmp/openadr_pi4.lock`),
   covering every checkout and machine that can reach the host. A Pi reboot clears
   `/tmp`, which is the correct outcome for a lock.

## Mechanism (scripts/pi4_lock.sh)

- Mutex = atomic `mkdir` on the Pi4, executed via a single `ssh bash -s` round-trip
  so check-and-act cannot race. An owner file records `user@host:worktree-path`, the
  **declared lease end** (UTC epoch), and a task description.
- `acquire -m "<task>" [-l minutes]` (default 60): the acquirer declares how long it
  expects to need the Pi4. Once the lease end passes, the lock counts as dead
  (crashed session) and the next acquirer steals it with a warning. `refresh
  [-l MIN]` extends a live lease from now; `status` shows holder, task, and lease
  end; `release` is owner-checked. Expiry is compared against the Pi4's clock, so
  clock skew between laptops is irrelevant.
- `acquire` polls every 20 s and exits 2 after ~9 min — deliberately below the
  10-minute tool timeout of AI sessions — with "rerun to keep waiting".
- Re-entrant per owner: re-acquiring while holding renews the lease.
- `run_all_tests.sh` acquires the lock automatically (`-l 180`) before the remote
  docker suites of [[testing-strategy]] (Rust-in-docker, E2E, resilience) and
  releases it via EXIT trap; manual `ssh Pi4 docker …` sequences (including
  [[fleet-tooling]] work) must bracket themselves with acquire/release per the
  `.claude/CLAUDE.md` rule.

## Same pattern for the shared WSL instance (`scripts/wsl_lock.sh`)

The dev laptop has only 8 GB RAM (`.claude/CLAUDE.md` §memory-budget); a `wsl cargo
build/check/test/clippy` can exhaust the pagefile and crash WSL, and — same root cause as the
Pi4 — multiple worktrees/sessions share the one WSL instance. `wsl_lock.sh` copies
`pi4_lock.sh`'s mechanism verbatim (self-declared lease as a UTC epoch, re-entrant per owner,
dead-lock stealing, `acquire` polling then exiting 2 after ~9 min) with two differences: the
lock lives *inside* WSL (`wsl bash -s --` instead of `ssh <host> bash -s --`), and the default
lease is 20 min / 10 s poll (vs. Pi4's 60 min / 20 s) — shorter because it's guarding a local
build, not a remote multi-stack docker run. `.claude/CLAUDE.md` §wsl-lock requires it around
every large-memory WSL command.

## Shared hostname variable (`OPENADR_LAB_HOST`)

`pi4_lock.sh` and `run_all_tests.sh` each independently hardcoded the same "Pi4" default
behind their own script-specific env var (`PI4_LOCK_HOST`, `DOCKER_HOST`) — the exact
duplication that made the SSH alias rename from `Pi4-Server` to `Pi4` touch 29 files across
the repo. Both scripts now fall back to a shared `OPENADR_LAB_HOST` env var before their own
hardcoded default, so a future hostname change needs only one variable set (or one grep
target, if it must be a permanent rename again); existing script-specific overrides still
take precedence for anyone already relying on them.

## Limits

- **Cooperative only.** Nothing on the Pi4 enforces it; a session that ignores the
  rule can still run docker directly. Honest `-l` values matter: too short invites a
  legitimate mid-run steal, a huge lease from a crashed session blocks others until
  it expires.
- **Not the whole policy.** The user may reserve the Pi4 for processes outside the
  lock entirely, so sessions still ask before their first Pi4 use — the lock
  serializes participants, it does not grant permission.
- Windows-specific implementation traps (MSYS rewriting POSIX-path ssh arguments,
  ssh word-splitting multi-word remote arguments) are recorded in
  docs/history/project_journal.md §"Pi4 lease lock"; `*.sh` is pinned to LF in
  `.gitattributes` because the script pipes a heredoc to the Pi4's Linux bash.
