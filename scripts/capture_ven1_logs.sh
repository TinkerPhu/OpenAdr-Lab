#!/usr/bin/env bash
#
# capture_ven1_logs.sh — TEMPORARY: archive ven-1's current container logs on
# Node1's host filesystem before any rebuild/recreate, and flag any
# /sim/inject activity found in them.
#
# Why: docker's json-file log driver deletes a container's log when the
# container is recreated (`docker compose build && up -d`), and Node1 has no
# log aggregator. During the ven-1 PV-injection mystery
# (docs/history/project_journal.md, "round 3"), two legitimate redeploys by
# unrelated work wiped potential evidence before it could be inspected. Run
# this before any `docker compose build/up -d ven-1` on Node1 — takes a few
# seconds. Remove this script + its call sites once the mystery is resolved
# (tracked in docs/BACKLOG.md).
#
# Usage:
#   bash scripts/capture_ven1_logs.sh
#
set -euo pipefail

REMOTE_HOST="${LOCK_HOST:-Node1}"
ARCHIVE_DIR="/srv/docker/ven1_log_archive"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"

ssh "$REMOTE_HOST" "
  set -euo pipefail
  mkdir -p '$ARCHIVE_DIR'
  cd /srv/docker/openadr_lab/VEN
  docker compose logs ven-1 > '$ARCHIVE_DIR/ven1_$STAMP.log' 2>&1
  # Retention: keep the newest 50 archives only.
  ls -1t '$ARCHIVE_DIR'/ven1_*.log | tail -n +51 | xargs -r rm -f
  echo \"archived: $ARCHIVE_DIR/ven1_$STAMP.log\"
  echo '--- sim/inject lines in this archive ---'
  grep -i 'sim/inject' '$ARCHIVE_DIR/ven1_$STAMP.log' || echo '(none found)'
"
