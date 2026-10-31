---
title: Docker Host Lease Lock
type: decision
created: 2026-07-17
updated: 2026-10-31
synced_commit: 102f7fa
sources: [ci/locks/docker_host_lock.sh, ci/locks/wsl_lock.sh, ci/locks/lib/, run_all_tests.sh, .claude/CLAUDE.md]
tags: [node1, docker, concurrency, dev-workflow]
---

# Docker Host Lease Lock — serializing a shared docker host

Multiple AI sessions work in parallel worktrees (the pattern kept from
[[superpowers-not-adopted]]), and most tasks end in a docker build or E2E run on the
shared docker hosts described in [[deployment-topology]] (Node1, Node2, Node3). Concurrent
`docker compose build/run` invocations there corrupt each other's stacks and produce
false test failures. `ci/locks/docker_host_lock.sh` provides a cooperative **lease lock** that every docker sequence on a shared
host must hold (`.claude/CLAUDE.md` §node1-lock, §node2-lock). The CI runner ([[ci-pipeline]]) takes the same
lock for its extra lanes and skips a host a session holds.

## Decision: lease lock on the resource, not a queue file

The alternative considered was a queue/board file where each task appends a line and
waits until its entry is first. Rejected for two reasons
(docs/history/project_journal.md §"Node1 lease lock"):

1. **Crash behaviour.** A killed session leaves its queue entry at the head and
   deadlocks everyone behind it — so every entry would need its own expiry anyway,
   at which point the queue is a more complex lock.
2. **Visibility.** Worktrees are separate checkouts; a file in one is invisible to
   the others. The lock therefore lives *on the docker host* at one fixed path
   (`/tmp/openadr_docker_host.lock`), covering every checkout and machine that can reach it, and every SSH alias
   of a machine reaches the same lock (a lock named after the alias once made two aliases two independent
   locks). A host reboot clears `/tmp`, which is the correct outcome for a lock.

## Mechanism (ci/locks/docker_host_lock.sh)

- Mutex = atomic `mkdir` on the host, executed via a single `ssh bash -s` round-trip (no ssh when the host is
  `localhost`, as for the runner on Node3) so check-and-act cannot race. An owner file records
  `user@host:worktree-path#session-<id>` — the checkout *and* the AI session, so two sessions in one checkout
  exclude each other — the **declared lease end** (UTC epoch), and a task description. The mechanism is shared
  with `wsl_lock.sh` in `ci/locks/lib/`.
- `acquire -m "<task>" [-l minutes]` (default 60): the acquirer declares how long it
  expects to need the host. Once the lease end passes, the lock counts as dead
  (crashed session) and the next acquirer steals it with a warning. `refresh
  [-l MIN]` extends a live lease from now; `status` shows holder, task, and lease
  end; `release` is owner-checked. Expiry is compared against the host's clock, so
  clock skew between laptops is irrelevant.
- `acquire` polls every 20 s and exits 2 after ~9 min — deliberately below the
  10-minute tool timeout of AI sessions — with "rerun to keep waiting".
- Re-entrant per owner: re-acquiring while holding renews the lease.
- `run_all_tests.sh` acquires the lock automatically (`-l 180`) before the docker
  suites of [[testing-strategy]] (Rust-in-docker, E2E, resilience) and
  releases it via EXIT trap; manual `ssh Node1 docker …` sequences (including
  [[fleet-tooling]] work) must bracket themselves with acquire/release per the
  `.claude/CLAUDE.md` rule.

## Same pattern for the shared WSL instance (`ci/locks/wsl_lock.sh`)

The dev laptop has only 8 GB RAM (`.claude/CLAUDE.md` §memory-budget); a `wsl cargo
build/check/test/clippy` can exhaust the pagefile and crash WSL, and — same root cause as the
Node1 — multiple worktrees/sessions share the one WSL instance. `wsl_lock.sh` copies
`docker_host_lock.sh`'s mechanism verbatim (self-declared lease as a UTC epoch, re-entrant per owner,
dead-lock stealing, `acquire` polling then exiting 2 after ~9 min) with two differences: the
lock lives *inside* WSL (`wsl bash -s --` instead of `ssh <host> bash -s --`), and the default
lease is 20 min / 10 s poll (vs. 60 min / 20 s for a docker host) — shorter because it's guarding a local
build, not a remote multi-stack docker run. `.claude/CLAUDE.md` §wsl-lock requires it around
every large-memory WSL command.

## Which host

`LOCK_HOST` overrides the host for the lock script alone; `OPENADR_LAB_HOST` is the shared default that
`run_all_tests.sh` also reads (its own override is `DOCKER_HOST`, where an empty value means this machine). One
variable therefore changes the lab's default host, which matters because a rename once touched 29 files.
`DOCKER_HOST=Node2 bash run_all_tests.sh …` sets the lock host to match.

## Limits

- **Cooperative only.** Nothing on the host enforces it; a session that ignores the
  rule can still run docker directly. Honest `-l` values matter: too short invites a
  legitimate mid-run steal, a huge lease from a crashed session blocks others until
  it expires.
- **Not the whole policy.** The user may reserve a host for processes outside the
  lock entirely, so sessions still ask before their first use — the lock
  serializes participants, it does not grant permission.
- Windows-specific implementation traps (MSYS rewriting POSIX-path ssh arguments,
  ssh word-splitting multi-word remote arguments) are recorded in
  docs/history/project_journal.md §"Node1 lease lock"; `*.sh` is pinned to LF in
  `.gitattributes` because the script pipes a heredoc to the host's Linux bash.
