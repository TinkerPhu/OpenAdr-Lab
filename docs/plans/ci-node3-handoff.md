# Plan: what is left of the Node3 CI (deploy)

**Status: partly implemented** (flaky retry is built too, see TESTING.md). The board, the runner, `ci-handoff.sh`, `ci-next.sh`, the merge
queue and the host-mode fixes are built and described in `docs/guidelines/TESTING.md`
("CI on Node3"). This plan keeps only what is not built yet.

## 1. Deploy from the runner

A deploy request is a ticket with `KIND=deploy`, `STACK` and `SERVICES` instead of tiers, written
by a session after the user said so; the runner builds and ships, nothing deploys by itself.

- E2E tests a `ci`-profile build (`tests/docker-compose.test.yml` passes `CARGO_PROFILE: ci`), not
  the `release` binary production runs. A deploy therefore builds `release` images from `main`
  with the same BuildKit caches.
- Node1, Node2 and Node3 are the same CPU architecture, so the image built once on Node3 is shipped
  (`docker save | ssh <host> docker load`, or a registry on Node3) instead of being rebuilt on a
  production host under load.
- The previous image keeps a `:previous` tag on the target host; rollback is one command.
- Node3 needs an SSH key for Node1 and Node2. It only ever runs commits that sessions pushed to
  this repository's branches; it never runs fork pull requests.
- Existing deploy procedure to replace or call: `.claude/skills/deploy-node1/SKILL.md`.

## Reuses

`scripts/ci/board.sh` (tickets), `scripts/ci/runner.sh`, `scripts/docker_host_lock.sh`,
`VEN/Dockerfile` and `VTN/bff/Dockerfile` (cache mounts, `CARGO_PROFILE` build argument).

## When this is done

Fold deploy into `docs/guidelines/TESTING.md` and delete this plan.
