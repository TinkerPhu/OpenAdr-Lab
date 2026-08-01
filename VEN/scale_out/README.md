# VEN/scale_out

Deployment configs for VEN instances running on **additional physical hosts**
beyond the primary VTN host — horizontal scale-out, adding more machines
rather than more capacity on one machine. Each such host gets its own numbered
subdirectory here (`node2/`, `node3/`, ...).

## Why this lives under `VEN/`, not beside it

A scale-out host isn't a separate system — it's another deployment instance
of the *same* VEN application defined in `VEN/docker-compose.yml`. A sibling
top-level directory (next to `VEN/`/`VTN/`) would wrongly suggest otherwise.

## Naming: `scale_out/nodeN`

- **`scale_out`** — the standard infra term for horizontal scaling (adding
  more machines), as opposed to scaling up one machine's resources.
- **`nodeN`** numbers *each additional host's deployment*, starting at 2. The
  primary host's own `VEN/docker-compose.yml` (ven-1/2/3/ui) deliberately is
  **not** part of this numbering scheme and was not renamed/moved into a
  `node1/` — it's the established, working production path (referenced by
  the `deploy-pi4` skill and `CLAUDE.md`), and moving it purely for numbering
  symmetry would be real churn on something that already works. So
  numbering starts at 2, not because a `node1` exists somewhere, but because
  each `nodeN/` here is conceptually "the Nth additional host beyond the
  primary."

## What a `nodeN/` directory contains

Using `node2/` (currently `Po4`, 192.168.1.104, running `ven-4`, administered
by the primary host's VTN — currently `Pi4`, 192.168.1.103 — over the real
LAN, since a second machine can't join the primary host's Docker network or
resolve `vtn`/`bff` by Docker DNS) as the concrete example:

- `docker-compose.yml` — the VEN + `ui` services for that host, adapted from
  `VEN/docker-compose.yml`'s `ven-1`/`ui` pattern. Build contexts point back
  up to `VEN/` (`../..`) and `VEN/ui/` (`../../ui`), since the Dockerfiles
  live there, not in `nodeN/`.
- `.env` — `PRIMARY_HOST_LAN_IP`, the primary host's real LAN IP (default
  `192.168.1.103`). Committed with real values, same convention as
  `VTN/.env` — this is local-network config, not a secret.
- `nginx/nginx.conf` — a copy of `VEN/ui/nginx.conf` with the
  `/api/vens-registry` proxy target changed from `bff:8090` (Docker DNS,
  primary-host-only) to the primary host's real BFF address over the LAN.
  Bind-mounted over the built image's config at container start (see the
  `ui` service's `volumes:` in `docker-compose.yml`) rather than baked in at
  build time, so each node's UI keeps using `VEN/ui`'s own Dockerfile/build
  unmodified.
- `data/` — gitignored (`**/data/` in the repo's `.gitignore`), holds the
  VEN's persisted state (`state.json`). The VEN container's `nonroot` user is
  uid/gid 2000:2000; a plain `mkdir -p` creates this as 1000:1000 — run
  `chown -R 2000:2000 data/<ven-name>` before the first `docker compose up`.

Each node's VEN profile (asset mix/physics) lives at `VEN/profiles/`
alongside the primary trio (e.g. `ven-4.yaml`), not inside `nodeN/` —
profiles are VEN-application config, independent of which host runs them.

## Bringing a node up on a fresh host

See `docs/history/project_journal.md` ("Po4 — a second Pi4 extending the
fleet") for `node2`'s original narrative, and the plan history for the exact
reprovisioning steps (Docker install, sparse git checkout, `chown`, `docker
compose up`, then rerun `scripts/seed_vtn.py` to restore the VTN-side
`DASHBOARD_URL` registration). The same shape applies to any future `nodeN/`.
