---
title: Fleet Tooling
type: component
created: 2026-07-11
updated: 2026-07-11
synced_commit: c5a1d03
sources: [fleet.sh, scripts/gen_fleet_profiles.py, scripts/fleet_status.py, scripts/db_reset.sh, scripts/seed_vtn.py, VEN/src/config.rs, docs/plans/roadmap/phase-2-fleet-enablement.md]
tags: [fleet, docker, provisioning, ven, phase2]
---

# Fleet Tooling

Phase 2 ("Fleet Enablement") replaces the hand-seeded trio ([[deployment-topology]]'s
`ven-{1,2,3}`) with `fleet.sh up N` — an arbitrary-size, disposable VEN fleet for load
and scaling work, layered *alongside* the seeded trio rather than replacing it.

## Why this shape: the self-registration dead end

The phase's original plan had each VEN self-register on startup (`POST /vens` with its
own credential). Investigation before writing any code found this is architecturally
blocked in this project's openleadr-rs fork: `POST /vens`
(`openleadr-rs/openleadr-vtn/src/api/ven.rs`) is gated by a hardcoded `VenManagerUser`
extractor, not an OAuth scope — a VEN's own credential (role `VEN`) can never call it,
regardless of what scope it's granted.

Three options were weighed: give every fleet VEN a VenManager credential (a real
over-privilege that gets worse as fleet size grows), patch the openleadr-rs fork (real
upstream-fork surgery for a project trying to diverge from it as little as possible),
or register the whole fleet in bulk with a VenManager credential *before* any VEN
container starts. The third was chosen — no fleet VEN ever holds an elevated
credential; the registration step lives entirely in `gen_fleet_profiles.py`, reusing
the existing, already-idempotent `provision_vens()` from `scripts/seed_vtn.py` (the
same function that provisions `ven-1`/`ven-2`/`ven-3`).

## `fleet.sh` — the three subcommands

- **`up N [--seed S] [--fresh]`**: `scripts/gen_fleet_profiles.py` generates N
  profiles (`fleet-ven-{i:03d}`) with a randomized-but-seeded asset mix (EV/battery
  presence, capacities, PV/base-load scale — same shape as the hand-written
  `ven-1/2/3` profiles), a `VEN/docker-compose.fleet.yml` override, and a
  `VEN/fleet/manifest.json`; bulk-provisions all N via `provision_vens()`; then
  `docker compose up -d --build` and health-polls each instance. `--fresh` runs
  `db_reset.sh` first.
- **`down [--purge]`**: stops the fleet containers; `--purge` also removes the
  generated data directories, profiles, compose file, and manifest — a clean slate
  for the next `up`.
- **`status`**: `scripts/fleet_status.py` — per-VEN `/health` plus a cross-check
  against the VTN's own `GET /vens` (using the `ven-manager` fixture credential) so a
  container that's "up" but never actually registered is visible as a mismatch.

## Personas (Phase 4, WP4.5)

`fleet.sh up N --personas eco:0.4,comfort:0.4,commuter:0.2` assigns each
generated VEN a persona (seeded largest-remainder split — `scripts/personas.py`
is the single source of truth, with a `python3 scripts/personas.py` self-check).
The persona nudges the generated asset mix (EV/battery probability, base-load
range) and lands in `VEN/fleet/manifest.json`; at experiment time
`run_experiment.py --personas` gives each fleet VEN its persona's EV session
(request mode / target / departure / budget) and comfort-curve override, and
`kpi.py --manifest` segments KPIs per persona ([[experiment-harness]]).
Presets are pure configuration over the Phase-4 features — documented in
`VEN/profiles/README.md`.

## GB-09: poll-startup jitter, not per-profile intervals

The backlog item asked for poll intervals to become a profile key ("useful for
testing"). Investigation found the real goal — "N VENs don't align their polls" — is
met more simply by a one-time startup delay: `VEN/src/config.rs` gained
`poll_startup_jitter_s` (env `POLL_STARTUP_JITTER_S`), threaded into all three poll
spawners ([[reliability-and-config]]) as a sleep before their first poll attempt. The
generator staggers this by instance index (4s stride). Moving poll intervals into the
profile schema itself was judged unnecessary scope for what the goal actually needed.

## GB-06: `scripts/db_reset.sh`

Drops and recreates the VTN's `public` (openleadr-rs's own tables, re-applied via
SQLx auto-migration on VTN restart) and `lab_recorder` ([[history-store]]'s VTN-side
recorder schema) Postgres schemas, then reloads
`openleadr-rs/fixtures/test_user_credentials.sql` — replacing the manual `docker exec
psql < fixtures.sql` step documented in `VTN/vtn_setup_from_blog_step_by_step.md`.

## Verified live on Pi4; N=10 deliberately deferred

A full `up 3` → `status` → idempotent second `up` (all three VENs correctly reported
"already provisioned — skipping") → `down --purge` cycle ran clean, including real
MILP plan generation on a fleet VEN and the per-instance jitter visible in its logs.
Per-VEN memory is modest (13–80MB at N=3) — not the constraint. What is: this Pi4
([[deployment-topology]]) already runs roughly 20 unrelated production containers
with only ~660MB free RAM and a load average around 3 *before* the fleet starts, and
a single VEN's MILP solve alone briefly used 109% CPU. Concurrent solves across 10
VENs on this shared quad-core box risk starving those other services — a CPU, not
memory, constraint. The full N=10 exit demonstration from
`docs/plans/roadmap/phase-2-fleet-enablement.md` is deferred to a deliberately
scheduled low-usage window rather than run ad hoc.

Phase 3's [[experiment-harness]] builds on this tooling: `run_experiment.py`
follows the same run-on-the-docker-host convention and its scenarios drive the
same stack the fleet brings up.
