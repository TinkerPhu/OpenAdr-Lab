---
title: Fleet Tooling
type: component
created: 2026-07-11
updated: 2026-10-22
synced_commit: 97a6abc
sources: [fleet.sh, scripts/gen_fleet_profiles.py, scripts/fleet_status.py, scripts/db_reset.sh, scripts/seed_vtn.py, scripts/personas.py, scripts/gen_fleet_mqtt_secrets.py, VEN/scale_out/README.md, VEN/scale_out/node2/docker-compose.yml, VEN/docker-compose.yml, VEN/profiles/README.md, VEN/src/config.rs, VEN/src/profile/polling.rs]
tags: [fleet, docker, provisioning, ven, phase2]
---

# Fleet Tooling

There are two ways a lab fleet exists, and they differ in how much of the deployment is
generated:

- **The standing fleet** — `ven-1` … `ven-20` with hand-written profiles
  (`VEN/profiles/ven-N.yaml`, varied asset mixes) and hand-written compose files: `ven-1`…`ven-3` in
  `VEN/docker-compose.yml` on the primary host, `ven-4`…`ven-20` in `VEN/scale_out/node2/docker-compose.yml`
  on the second ([[deployment-topology]]). This is what the experiments and fleet monitoring run against.
- **The generated fleet** — `fleet.sh up N`, an arbitrary-size disposable fleet of `fleet-ven-NNN` for load and
  scaling work, layered alongside the standing one (Phase 2, "Fleet Enablement").

## The standing fleet and scale-out

`VEN/scale_out/nodeN/` holds one additional physical host's deployment. The numbering starts at 2 because
each `nodeN` is the Nth host *beyond* the primary, whose own compose file is deliberately not renamed. A
second host cannot join the primary's docker network or resolve `vtn`/`bff` by DNS, so its compose reaches the
primary over the LAN (`PRIMARY_HOST_LAN_IP` in its `.env`) and bind-mounts its own nginx config that points the
UI's registry proxy at the primary's BFF. Profiles stay in `VEN/profiles/`, because they are application
configuration independent of the host that runs them; `data/` is git-ignored and must be owned by the VEN's
non-root uid (2000:2000).

Every VEN in both compose files reaches the lab's fleet broker for live telemetry (`FLEET_MQTT_HOST/PORT/
USERNAME`; [[fleet-monitoring]]). The password is **derived per VEN**, not shared: `scripts/gen_fleet_mqtt_secrets.py`
computes `HMAC-SHA256(root secret, ven name)`, so one root secret yields any number of VEN credentials and the
same VEN gets the same password on both hosts without exchanging anything. The root never leaves the operator;
the script emits the broker's account list (`--env-file`) and, per VEN, a file carrying only that VEN's
`FLEET_MQTT_PASSWORD` (`--per-ven-dir`) — a file per VEN rather than one aggregate because compose's
`env_file:` feeds the container but is not a source for `${...}` interpolation in the same service, so an
interpolated per-VEN variable resolves to the empty string silently. The files are git-ignored and optional, so
a bare checkout starts and the VEN fails to authenticate loudly instead of sharing a credential quietly
(an ACL pins each credential to its own topic; `scripts/test_fleet_acl.sh`).

## Provisioning: why bulk, with a business credential

The phase's original plan had each VEN self-register at startup. That is blocked by the VTN: creating VEN objects
needs a business-level credential, which a VEN must never hold (a fleet-wide over-privilege that grows with the
fleet). So registration is done **once, in bulk, before the containers start**, by
`provision_vens()` in `scripts/seed_vtn.py` — the same idempotent function that provisions the standing 20.

On OpenADR 3.1 it is three calls under the lab's one bootstrap business client (`bl-client`, the only user in
SQL, `VTN/fixtures/01_bl_client.sql`): create the user **with its scopes already set**, give it a credential,
create the VEN object carrying the `clientID`. Setting the scopes in the first call closes the window in which
a VEN could mint a scope-less token and then poll an empty world for the token's lifetime (GB-49,
[[openadr-interface]]). An already-provisioned VEN (its credential answers) is reconciled rather than skipped.
`seed_vtn.py` also seeds the demo programs and events and declares every payload's quantity and unit on the wire,
each program pointing at the published profile document ([[wire-contract]]).

## `fleet.sh` — the generated fleet

- **`up N [--seed S] [--fresh] [--personas …]`** runs `scripts/gen_fleet_profiles.py`, which writes N seeded
  profiles (`fleet-ven-NNN`: PV and base load always, EV and battery by probability), a
  `VEN/docker-compose.fleet.yml` and a `VEN/fleet/manifest.json`, then bulk-provisions through
  `provision_vens()`; then `docker compose up -d --build` and a health poll per instance (ports from 8300).
  `--fresh` runs `db_reset.sh` first.
- **`down [--purge]`** stops the fleet; `--purge` also removes the generated data (through a throwaway root
  container, since bind-mount directories are created root-owned), profiles, compose file and manifest.
- **`status`** (`scripts/fleet_status.py`): per-VEN `/health` cross-checked against the VTN's own VEN list, so a
  container that is up but never registered shows as a mismatch.

Personas (`scripts/personas.py`, the single source of truth, self-checking) are `eco` (opportunistic EV, aggressive
comfort curve), `comfort` (as-soon-as-possible EV, flat high curve) and `commuter` (by-deadline EV ready at 07:00).
`--personas eco:0.4,comfort:0.4,commuter:0.2` assigns them by seeded largest-remainder split, nudges the generated
asset mix and records the persona in the manifest; the experiment harness then gives each VEN its persona's EV
session and comfort override at run time ([[experiment-harness]]).

> **DRIFT** The generator has not followed the VEN's later changes, so `fleet.sh up` is not known to work today.
> (1) The generated compose uses build context `.` (the `VEN/` directory) with `Dockerfile`, but the VEN image now
> builds from the repository root (`VEN/Dockerfile` copies `lab-core/`), as the hand-written compose files do.
> (2) It sets `POLL_STARTUP_JITTER_S`, which `VEN/src/config.rs` no longer reads: startup jitter is now the profile's
> `polling.startup_jitter_fixed_pct` / `startup_jitter_random_max_pct` (a share of `events_secs`; the
> `POLL_STARTUP_JITTER_*_PCT` env vars are test overrides), so the staggering is silently inert and a generated fleet
> brought up together polls in lockstep ([[reliability-and-config]]). (3) It carries no `FLEET_MQTT_*` settings, so
> generated VENs are invisible to [[fleet-monitoring]]. Re-verification needs a docker host; see [[ven-code-vs-docs-audit]] for the wiki's
> record of other open drifts.

## `scripts/db_reset.sh`

Drops and recreates the VTN's `public` schema (re-migrated on VTN restart) and reloads the bootstrap client fixture,
replacing a manual `psql < fixtures.sql` step. **`lab_recorder` is preserved by default**: it holds the BFF recorder's
history, over a million `reports_received` rows accumulated across months, and nothing regenerates it. A clean VTN
bring-up has no reason to destroy it; the script once dropped it unconditionally. `--wipe-recorder` drops it too,
printing the row count first ([[history-store]] for the VEN-side counterpart, [[vtn-stack]]).

## Capacity

Per-VEN memory is modest; the constraint on a shared host is CPU, because concurrent MILP solves across many VENs
compete with the other production containers ([[milp-planner]]). That is why the standing fleet is split across two
hosts, and why the N=10 exit demonstration was left to a scheduled low-usage window rather than run ad hoc.
