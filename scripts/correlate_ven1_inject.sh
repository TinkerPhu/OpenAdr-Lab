#!/usr/bin/env bash
#
# correlate_ven1_inject.sh — TEMPORARY: cross-reference every /sim/inject
# attempt on ven-1 against Node1's git commit history, to attribute
# unexplained calls to whatever branch/session was active at the time.
#
# Context: the ven-1 PV-injection mystery (docs/history/project_journal.md,
# "round 3") turned out to correlate almost exactly (2-3 min lead time) with
# commits on parallel UI-refactor branches (044/045/046) — consistent with
# "manually verify a chart/forecast visually against the live VEN, then
# commit" during iterative development. This script reproduces that
# correlation automatically instead of by hand. Remove once the mystery is
# resolved (tracked in docs/BACKLOG.md, GB-17).
#
# Usage:
#   bash scripts/correlate_ven1_inject.sh [SINCE] [UNTIL]
#   SINCE/UNTIL: git-log-compatible date strings, default: last 48h
#
set -euo pipefail

REMOTE_HOST="${LOCK_HOST:-Node1}"
SINCE="${1:-48 hours ago}"
UNTIL="${2:-now}"

echo "=== /sim/inject attempts on ven-1 (current container log) ==="
ATTEMPTS="$(ssh "$REMOTE_HOST" "cd /srv/docker/openadr_lab/VEN && docker compose logs ven-1 2>&1 | grep -i 'sim/inject' | grep -oP '(?<=\"timestamp\":\")[^\"]+'" || true)"
echo "$ATTEMPTS"

echo
echo "=== Node1 commits in range ($SINCE .. $UNTIL) ==="
ssh "$REMOTE_HOST" "cd /srv/docker/openadr_lab && git log --since='$SINCE' --until='$UNTIL' --format='%H|%cI|%s'"

echo
echo "=== nearest commit AFTER each attempt (git-log timestamps are ISO 8601 UTC via %cI) ==="
COMMITS="$(ssh "$REMOTE_HOST" "cd /srv/docker/openadr_lab && git log --since='$SINCE' --until='$UNTIL' --format='%cI|%s'")"
while IFS= read -r ts; do
  [ -z "$ts" ] && continue
  best=""
  best_delta=999999999
  while IFS='|' read -r cts subj; do
    [ -z "$cts" ] && continue
    d=$(( $(date -u -d "$cts" +%s 2>/dev/null || echo 0) - $(date -u -d "$ts" +%s) ))
    if [ "$d" -ge 0 ] && [ "$d" -lt "$best_delta" ]; then
      best_delta=$d
      best="$subj (+${d}s)"
    fi
  done <<< "$COMMITS"
  echo "$ts -> ${best:-no later commit in range}"
done <<< "$ATTEMPTS"
