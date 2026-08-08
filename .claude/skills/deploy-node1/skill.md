# Skill: Deploy to Node1

Deploy changed files to Node1 via scp, then rebuild the affected Docker service.
Do NOT commit or push before deploying — scp lets us test on Node1 without polluting git history.

## Golden rule: Node1 never originates changes

All edits happen locally. Node1 only receives files via scp — it never edits them.
This is essential: after committing locally and pushing, `git pull` on Node1 requires a clean working tree.
Any scp'd file that differs from the repo HEAD will block the pull with a merge conflict.
The fix is always `git checkout -- <file>` on Node1 before pulling — which is only safe because Node1 never made its own edits.

## Step 1 — Identify changed files

Use `git diff --name-only` (and `git ls-files --others --exclude-standard` for untracked) to find what changed.
If the user named specific files, use those directly.

## Step 2 — scp each file

Mirror the local path under `/srv/docker/openadr_lab/` on Node1:

```bash
scp <local-path> Node1:/srv/docker/openadr_lab/<same-relative-path>
```

Example:
```bash
scp VEN/ui/src/components/controller-v2/charts/TariffChart.tsx \
    Node1:/srv/docker/openadr_lab/VEN/ui/src/components/controller-v2/charts/TariffChart.tsx
```

## Step 3 — Rebuild the affected service

**TEMPORARY, while the ven-1 PV-injection mystery is open (docs/history/project_journal.md,
"round 3"; remove once resolved, tracked in docs/BACKLOG.md): if `ven-1` is among the
services being rebuilt, run `bash scripts/capture_ven1_logs.sh` first.** Rebuilding
recreates the container and Docker deletes its logs on removal — Node1 has no log
aggregator, so any undiscovered `/sim/inject` evidence sitting in the live container's
logs is lost the moment you rebuild. The script archives them to
`/srv/docker/ven1_log_archive/` on Node1 and prints any `sim/inject` lines inline; it
takes a few seconds.

Determine which Docker service needs rebuilding from the file paths:

| Changed path prefix | Compose dir | Service(s) |
|---|---|---|
| `VEN/ui/src/` | `VEN/` | `ui` |
| `VEN/src/` or `VEN/Cargo.*` | `VEN/` | `ven-1 ven-2 ven-3` |
| `VTN/` | `VTN/` | `vtn` |
| `scripts/` | — | no rebuild; run directly via ssh python3 |

```bash
ssh Node1 "cd /srv/docker/openadr_lab/<compose-dir> && docker compose build <service> && docker compose up -d <service>"
```

**Important:** nginx caches upstream hostnames at startup. After rebuilding any `ven-*` backend service, **always restart `ui`** so nginx re-resolves the new container IPs — otherwise the proxy may route requests to the wrong container:

```bash
ssh Node1 "cd /srv/docker/openadr_lab/VEN && docker compose restart ui"
```

## Step 4 — Verify

```bash
ssh Node1 "docker ps --filter name=<service> --format '{{.Names}} {{.Status}}'"
```

Container should show `Up X seconds/minutes`.

## Step 5 — After testing, commit and sync

Once the change is confirmed working:
1. Commit locally as usual
2. `git push`
3. `ssh Node1 "cd /srv/docker/openadr_lab && git checkout -- <scp'd files> && git pull"`
   (the checkout discards the scp copy so pull can fast-forward cleanly)
