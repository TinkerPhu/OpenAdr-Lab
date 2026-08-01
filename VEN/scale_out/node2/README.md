# VEN/scale_out/node2

Deployment config for `ven-4`, a VEN instance running on a **second physical
host** rather than the primary VTN host (currently `Po4`, 192.168.1.104).
Administered by the primary host's VTN (currently `Pi4`, 192.168.1.103) over
the real LAN, since a second machine can't join the primary host's Docker
network or resolve `vtn`/`bff` by Docker DNS.

## Why this lives under `VEN/`, not beside it

This isn't a separate system — it's another deployment instance of the
*same* VEN application defined in `VEN/docker-compose.yml`. A sibling
top-level directory (next to `VEN/`/`VTN/`) would wrongly suggest otherwise.

## Naming: `scale_out/node2`

- **`scale_out`** — the standard infra term for horizontal scaling (adding
  more machines), as opposed to scaling up one machine's resources. That's
  exactly what this is: capacity added by a second host, not a bigger box.
- **`node2`** — numbers *this specific host's deployment*, leaving room for
  `node3/` etc. if a third host joins later. The primary host's own
  `VEN/docker-compose.yml` (ven-1/2/3/ui) deliberately is **not** part of
  this numbering scheme and was not renamed/moved into a `node1/` — it's the
  established, working production path (referenced by the `deploy-pi4`
  skill and `CLAUDE.md`), and moving it purely for numbering symmetry would
  be real churn on something that already works. So numbering here starts
  at 2, not because a `node1` exists somewhere, but because this is
  conceptually "the first additional host beyond the primary."

## What's in this directory

- `docker-compose.yml` — `ven-4` + `ui` services, adapted from
  `VEN/docker-compose.yml`'s `ven-1`/`ui` pattern. Build contexts point back
  up to `VEN/` (`../..`) and `VEN/ui/` (`../../ui`), since the Dockerfiles
  live there, not here.
- `.env` — `PRIMARY_HOST_LAN_IP`, the primary host's real LAN IP (default
  `192.168.1.103`). Committed with real values, same convention as
  `VTN/.env` — this is local-network config, not a secret.
- `nginx/nginx.conf` — a copy of `VEN/ui/nginx.conf` with the
  `/api/vens-registry` proxy target changed from `bff:8090` (Docker DNS,
  primary-host-only) to the primary host's real `BFF` address over the LAN.
  Bind-mounted over the built image's config at container start (see the
  `ui` service's `volumes:` in `docker-compose.yml`) rather than baked in at
  build time, so this node's UI keeps using `VEN/ui`'s own Dockerfile/build
  unmodified.
- `data/` — gitignored (`**/data/` in the repo's `.gitignore`), holds
  `ven-4`'s persisted state (`state.json`). The VEN container's `nonroot`
  user is uid/gid 2000:2000; a plain `mkdir -p` creates this as 1000:1000 —
  run `chown -R 2000:2000 data/ven-4` before the first `docker compose up`.

`ven-4`'s VEN profile (asset mix/physics) lives at `VEN/profiles/ven-4.yaml`
alongside `ven-1.yaml`..`ven-3.yaml`, not in this directory — profiles are
VEN-application config, independent of which host runs them.

## Bringing this up on a fresh host

See `docs/history/project_journal.md` ("Po4 — a second Pi4 extending the
fleet") for the original narrative, and the plan history for the exact
reprovisioning steps (Docker install, sparse git checkout, `chown`, `docker
compose up`, then rerun `scripts/seed_vtn.py` to restore the VTN-side
`DASHBOARD_URL` registration).
