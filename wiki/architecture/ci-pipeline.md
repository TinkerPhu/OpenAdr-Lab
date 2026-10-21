---
title: CI Pipeline — the Node3 Runner
type: architecture
created: 2026-10-21
updated: 2026-10-21
synced_commit: f8f6775
sources: [ci/README.md, ci/suites.map, ci/runner.sh, ci/lanes.sh, ci/deploy.sh, ci/audits/, docs/guidelines/TESTING.md]
tags: [ci, testing, node3, merge-gate]
---

# CI Pipeline — the Node3 Runner

One host, **Node3**, tests what sessions hand over and is the only thing that merges to `main`
(`docs/guidelines/TESTING.md` "CI on Node3"). There is no GitHub Actions runner: the repository is
public, and a home-network runner executing fork pull requests would be an open door. Everything
that tests, gates, merges or deploys lives in `ci/`; none of it is needed to build or run the lab —
a fresh clone runs `bash run_all_tests.sh` on its own docker (`ci/README.md`).

## Flow

A session pushes its branch and runs `ci/ci-handoff.sh`, which writes a **ticket** onto the board
(`/srv/ci/board/<ticket>/`). A systemd timer starts `ci/runner.sh` every minute: it rebases the
ticket's commit onto `origin/main`, runs the **quick tier** (audits, fmt, clippy, cargo test), then
the **full tier** (E2E, resilience, openleadr-rs tests); all green and `kind=merge` fast-forwards
`main`. The session starts `ci/ci-wait.sh <ticket> --ack` as a background command and ends its turn;
the verdict wakes it, or `ci/ci-next.sh` brings it by hand later. `CI_HOST` is mandatory for the
hand-off scripts so a fresh clone never dials a host that does not exist.

Ticket kinds: `merge` (default), `check` (test only, never merge — A/B runs, milestones),
`--tiers quick` (never merges code; a quick ticket that selected a full-tier suite ends `green`, not
merged, because a suite the change needs did not run).

## Suite selection

`ci/suites.map` names, per path pattern, which suites a change there can affect (`cargo`, `rust-vtn`,
`e2e`, `resilience`, `all`, `-`); the first matching rule wins and a ticket runs the union over its
changed paths. A path no rule names runs everything, so a new directory is tested in full until
someone writes a rule for it. Rules say what a path *cannot* affect. Because a wrong skip would pass
unnoticed, a weekly `full-suite-main` job runs everything on `main`. This is the mechanism behind the
project's `no-needless-runs` rule: docs-only changes cost no suite.

## Lanes

The selected full-tier suites run on **lanes**: Node3 (always lane 1) plus `CI_LANE_HOSTS` in priority
order (the lab: Node2, then Node1). Extra hosts are taken with the same lease lock sessions use
([[docker-host-lease-lock]]), once and without waiting; a host a session holds is skipped, never taken
over. Node1 serves the live stack, so it is last and skipped under load or low memory. Longest suite
first, onto the lane expected to finish earliest by the median of that suite on that host
(`board_suite_seconds`). A red lane cancels the others.

## Deploy tickets

`ci/deploy.sh` builds release images of `origin/main` on Node3 and switches a production host over, so
production images come from the tested commit rather than from a hand-built checkout
(see [[deployment-topology]]).

## The board

`ci/web/ci_web.py` (stdlib only, port 8480) serves a read-only view: queue, running ticket and step,
24-hour timeline, per-step bars, log tails and an estimated finish per ticket (median of finished
tickets of the same class, chained through the queue). Clearing finished tickets is opt-in behind a
bearer token; merged tickets archive themselves after 24 h. It has no authentication — LAN only.

## Merge gates the runner enforces

`cargo fmt`, `clippy --all-targets --all-features -D warnings`, `cargo audit`, and the Python audits
in `ci/audits/`: file sizes, the duplication ratchet, the debt gate, the step-definition check and
`audit_ven_architecture.py` (the invariants of [[ven-hexagonal-architecture]]). The CI mechanism has
its own tests (`ci/test_*.sh`, `ci/test_web.py`). How these suites are shaped:
[[testing-strategy]].
