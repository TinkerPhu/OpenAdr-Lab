#!/usr/bin/env bash
# GB-07 — bring up the full lab stack (VTN + DB + BFF + VTN UI, seed demo
# data, then the base VEN stack) in one command. fleet.sh already covers
# scale-out fleet VEN bring-up (VEN/scale_out); this script is for the
# README "Quick Start" stack it doesn't touch — VTN + the 3 base VENs —
# which previously needed 3 separate manual `docker compose up` invocations
# across VTN/ and VEN/ plus a manual seed step in between.
#
# Usage:
#   bash scripts/setup_all.sh [--fresh] [--skip-seed]
#     --fresh       reset the VTN database (scripts/db_reset.sh) after the
#                   VTN stack is up, before seeding
#     --skip-seed   don't run scripts/seed_vtn.py (seeding is safe to
#                   re-run otherwise — it skips programs that already exist)
#
# Run from the repo root, directly on the docker host (matches this repo's
# existing run_all_tests.sh / fleet.sh / deploy-node1 convention — no SSH
# wrapping here; wrap the whole invocation in
# `ssh <host> "cd <path> && bash scripts/setup_all.sh ..."` for a remote host).
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VTN_DIR="$REPO_ROOT/VTN"
VEN_DIR="$REPO_ROOT/VEN"
VTN_PORT="${VTN_PORT:-8200}"
VEN_PORTS=(8211 8212 8213)

fresh=false
seed=true
while [[ $# -gt 0 ]]; do
    case "$1" in
        --fresh) fresh=true; shift ;;
        --skip-seed) seed=false; shift ;;
        *) echo "unknown option: $1"; exit 1 ;;
    esac
done

wait_healthy() {
    local label="$1" port="$2" deadline=$((SECONDS + 300))
    until curl -sf "http://127.0.0.1:${port}/health" >/dev/null 2>&1; do
        if [[ $SECONDS -ge $deadline ]]; then
            echo "FAIL: $label did not become healthy within 300s"
            exit 1
        fi
        sleep 3
    done
    echo "  $label healthy (port $port)"
}

echo "=== Deploying VTN stack (db, vtn, bff, ui) ==="
(cd "$VTN_DIR" && docker compose up -d --build)
echo "Waiting for VTN to become healthy ..."
wait_healthy "vtn" "$VTN_PORT"

if $fresh; then
    echo "=== Resetting VTN database ==="
    bash "$REPO_ROOT/scripts/db_reset.sh"
fi

if $seed; then
    echo "=== Seeding demo programs and events ==="
    python3 "$REPO_ROOT/scripts/seed_vtn.py" --vtn-url "http://127.0.0.1:${VTN_PORT}"
fi

echo "=== Deploying VEN stack (ven-1..3, ui) ==="
(cd "$VEN_DIR" && docker compose up -d --build)
echo "Waiting for VENs to become healthy ..."
for port in "${VEN_PORTS[@]}"; do
    wait_healthy "ven ($port)" "$port"
done

echo "Full lab stack is up."
echo "  VTN Operator UI: http://localhost:8221"
echo "  VEN Device UI:   http://localhost:8214"
