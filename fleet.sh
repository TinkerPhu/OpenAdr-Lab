#!/usr/bin/env bash
#
# Phase 2 (WP2.5) — fleet lifecycle for arbitrary-N VEN bring-up.
#
# Usage:
#   bash fleet.sh up N [--seed S] [--fresh] [--personas eco:0.4,comfort:0.4,commuter:0.2]
#   bash fleet.sh down [--purge]              # stop the fleet (--purge also removes data/profiles)
#   bash fleet.sh status                      # per-VEN health + VTN registration check
#
# Run from the repo root, directly on the docker host (matches this repo's
# existing run_all_tests.sh / deploy-pi4 convention — no SSH wrapping here).
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
VEN_DIR="$REPO_ROOT/VEN"
COMPOSE_FILE="$VEN_DIR/docker-compose.fleet.yml"
MANIFEST="$VEN_DIR/fleet/manifest.json"
BASE_PORT=8300

usage() {
    sed -n '2,7p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
}

cmd_up() {
    if [[ $# -lt 1 ]]; then
        usage
        exit 1
    fi
    local count="$1"; shift
    local seed=42
    local fresh=false
    local personas=""
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --seed) seed="$2"; shift 2 ;;
            --fresh) fresh=true; shift ;;
            --personas) personas="$2"; shift 2 ;;
            *) echo "unknown option: $1"; usage; exit 1 ;;
        esac
    done

    if $fresh; then
        bash "$REPO_ROOT/scripts/db_reset.sh"
    fi

    if [[ -n "$personas" ]]; then
        python3 "$REPO_ROOT/scripts/gen_fleet_profiles.py" --count "$count" --seed "$seed" --personas "$personas"
    else
        python3 "$REPO_ROOT/scripts/gen_fleet_profiles.py" --count "$count" --seed "$seed"
    fi

    (cd "$VEN_DIR" && docker compose -f docker-compose.fleet.yml up -d --build)

    echo "Waiting for fleet VENs to become healthy ..."
    local i name port deadline
    for ((i = 0; i < count; i++)); do
        name=$(printf "fleet-ven-%03d" "$i")
        port=$((BASE_PORT + i))
        deadline=$((SECONDS + 120))
        until curl -sf "http://127.0.0.1:${port}/health" >/dev/null 2>&1; do
            if [[ $SECONDS -ge $deadline ]]; then
                echo "FAIL: $name did not become healthy within 120s"
                exit 1
            fi
            sleep 2
        done
        echo "  $name healthy (port $port)"
    done
    echo "Fleet of $count VENs is up."
}

cmd_down() {
    local purge=false
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --purge) purge=true; shift ;;
            *) echo "unknown option: $1"; usage; exit 1 ;;
        esac
    done

    if [[ ! -f "$COMPOSE_FILE" ]]; then
        echo "No fleet compose file found ($COMPOSE_FILE) — nothing to tear down."
        exit 0
    fi

    if $purge; then
        (cd "$VEN_DIR" && docker compose -f docker-compose.fleet.yml down -v)
        rm -rf "$VEN_DIR"/data/fleet-ven-*
        rm -f "$VEN_DIR"/profiles/fleet-ven-*.yaml
        rm -f "$COMPOSE_FILE" "$MANIFEST"
        echo "Fleet stopped and purged (data, profiles, compose file, manifest removed)."
    else
        (cd "$VEN_DIR" && docker compose -f docker-compose.fleet.yml down)
        echo "Fleet stopped (profiles/data/manifest kept — use --purge to remove)."
    fi
}

cmd_status() {
    python3 "$REPO_ROOT/scripts/fleet_status.py"
}

case "${1:-}" in
    up) shift; cmd_up "$@" ;;
    down) shift; cmd_down "$@" ;;
    status) shift; cmd_status "$@" ;;
    *) usage; exit 1 ;;
esac
