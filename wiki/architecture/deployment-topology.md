---
title: Deployment Topology
type: architecture
created: 2026-07-04
updated: 2026-08-09
synced_commit: 4c4f149
sources: [docs/architecture/VTN_ARCHITECTURE.md, .claude/CLAUDE.md, docs/guidelines/TESTING.md, fleet.sh, VEN/scale_out/node2/]
tags: [deployment, docker, node1]
---

# Deployment Topology

Everything runs in Docker on **Node1** (reached via ssh), directory
`/srv/docker/openadr_lab`, on the shared external network `vtn_openadr-net`
(docs/architecture/VTN_ARCHITECTURE.md §1).

## Port map

| Service | Container | Host port |
|---|---|---|
| VTN server | `vtn-vtn-1` | 8200 |
| VTN database | `vtn-db-1` | 8201 |
| VTN BFF | — | 8220 |
| VTN UI | — | 8221 |
| VEN 1–3 | `ven-ven-{1,2,3}-1` | 8211–8213 |
| VEN UI | `ven-ui-1` | 8214 |
| Fleet VENs (Phase 2, optional) | `ven-fleet-ven-{i:03d}-1` | 8300+i |

The [[vtn-stack]] and the three VEN containers are separate compose stacks joined by the
external network. Caution from `.claude/CLAUDE.md`: the Pi also hosts **productive
containers unrelated to this project — never stop them**. The Pi is also shared
between parallel dev sessions: any docker build or test run there must hold the
[[docker-host-lease-lock]] first (`scripts/docker_host_lock.sh`, `.claude/CLAUDE.md` §node1-lock). This is also the reason
[[fleet-tooling]]'s live verification deliberately stopped at N=3 rather than N=10 —
the Pi already runs ~20 of those unrelated containers with limited headroom.

## Node2: build/test offload, not a second production stack

**Node2** is a second docker host (`.claude/CLAUDE.md` §node2-lock) that runs `ven-4`
through `ven-13` (`VEN/scale_out/node2/docker-compose.yml`) plus build/test load taken off
Node1 — it shares the same [[docker-host-lease-lock]] mechanism as Node1, its own
independent lock keyed by host, so the two can be used concurrently by different sessions.
Node2 has no local VEN UI container: it proxies `/api/vens-registry` to Node1's real BFF
over the LAN instead (`nginx/nginx.conf` there) rather than running a redundant nginx+UI
image whose only reachable backend was Node1's anyway — one operator UI (on Node1) for the
whole fleet, not one per host. `VEN/scale_out/node2/` also has `tests/`/`VTN/` sparse-checked
out and the `openleadr-rs` submodule initialized, so all four [[testing-strategy]] suites run
there, not just Rust unit tests — see `test-host-preference` in `.claude/CLAUDE.md` for when
to prefer Node2 over Node1 for a build/test run.

## Development environments

- **Local Rust**: native Windows cargo lacks cmake/HiGHS, so all Rust compilation goes
  through WSL (`wsl cargo check` / `wsl cargo test`) (`.claude/CLAUDE.md` §local-rust).
  The HiGHS dependency comes from the [[milp-planner]].
- **Local UI**: `cd VEN/ui && npm test` / `npm run build` (same for `VTN/ui`).
- **Full-stack runs**: only on Node1 (`docker compose build/up`), including the E2E and
  resilience suites described in [[testing-strategy]].
- Deployments follow git pull on the Pi; builds are ARM64 (first VTN source build took
  ~25 min, cached afterwards — docs/history/project_journal.md §1).
