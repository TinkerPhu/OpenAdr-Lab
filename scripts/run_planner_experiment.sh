#!/usr/bin/env bash
# Run one planner solve-cost benchmark and append its records to the experiment log.
#
# Benchmarks print lines of the form `@@RESULT {json}`; this stamps each with the
# timestamp, the commit it ran against and the result class, then appends to
# experiments/planner_benchmarks/solve_cost.jsonl. See that directory's README.
#
#   bash scripts/run_planner_experiment.sh bench_phase1_vs_tank_slack [class]
#
# class defaults to 1 (deterministic). Pass 2 for anything time-limited or
# observed live — those cannot be regenerated, so the record is the only copy.
set -uo pipefail

BENCH="${1:?usage: run_planner_experiment.sh <bench_fn> [class]}"
CLASS="${2:-1}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LOG="$ROOT/experiments/planner_benchmarks/solve_cost.jsonl"
COMMIT="$(git -C "$ROOT" rev-parse --short HEAD)"
TS="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
DIRTY=""
git -C "$ROOT" diff --quiet -- VEN/src || DIRTY=" (VEN/src dirty)"

mkdir -p "$(dirname "$LOG")"
echo "running $BENCH against $COMMIT$DIRTY (class $CLASS)"

OUT="$(wsl bash -lc "cd /mnt/c/DriveD/Tinker/OpenAdr-Lab/VEN && cargo test -p ven-app $BENCH -- --ignored --nocapture" 2>&1)"
STATUS=$?
echo "$OUT" | grep -vE '^@@RESULT'

COUNT=0
while IFS= read -r line; do
  payload="${line#@@RESULT }"
  # Reject anything that is not valid JSON rather than corrupting the log.
  if ! echo "$payload" | python -c 'import json,sys; json.load(sys.stdin)' 2>/dev/null; then
    echo "  skipping malformed record: $payload" >&2
    continue
  fi
  python - "$payload" "$TS" "$COMMIT" "$BENCH" "$CLASS" >> "$LOG" <<'PY'
import json, sys
payload, ts, commit, bench, cls = sys.argv[1:6]
rec = json.loads(payload)
out = {"ts": ts, "commit": commit, "bench": bench, "class": int(cls)}
out.update(rec)
print(json.dumps(out, separators=(",", ":"), sort_keys=True))
PY
  COUNT=$((COUNT + 1))
done < <(echo "$OUT" | grep -E '^@@RESULT')

echo "appended $COUNT record(s) to ${LOG#$ROOT/}"
exit $STATUS
