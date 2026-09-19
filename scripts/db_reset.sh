#!/usr/bin/env bash
# GB-06 (Phase 2, WP2.5) — drop and re-seed the VTN's Postgres database for a
# clean bring-up, instead of the manual `docker exec ... psql < fixtures.sql`
# steps in VTN/vtn_setup_from_blog_step_by_step.md. Idempotent: safe to run
# against a fresh DB (nothing to drop yet) or a populated one.
#
# Usage: bash scripts/db_reset.sh
#
# Drops and recreates the `public` schema (openleadr-rs's own tables, SQLx-migrated
# back in on VTN restart) and reloads the credential fixture that the seed script
# and `fleet.sh` authenticate as.
#
# `lab_recorder` is PRESERVED by default. It holds the BFF recorder's history --
# over a million `reports_received` rows accumulated across months of fleet runs --
# and nothing regenerates it. No OpenADR migration touches that schema, so a
# "clean VTN bring-up" has no reason to destroy it. This script used to drop it
# unconditionally, which made `fleet.sh` and `setup_all.sh --fresh` silently
# destroy the lab's entire measurement history.
#
# Pass --wipe-recorder to drop it as well, when you genuinely want an empty
# history (a throwaway host, or a recorder schema change).
set -euo pipefail

wipe_recorder=false
while [[ $# -gt 0 ]]; do
    case "$1" in
        --wipe-recorder) wipe_recorder=true; shift ;;
        *) echo "unknown option: $1 (accepts --wipe-recorder)"; exit 1 ;;
    esac
done

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VTN_DIR="$REPO_ROOT/VTN"
PG_USER="${PG_USER:-openadr}"
PG_DB="${PG_DB:-openadr}"
# OpenADR 3.1: the lab owns this fixture now. Upstream's 3.0
# fixtures/test_user_credentials.sql is gone -- roles were replaced by scopes carried on
# the user object. Only the bootstrap business client lives in SQL; the 20 VEN users are
# created through POST /users by scripts/seed_vtn.py, so no password hash is maintained here.
FIXTURE="$REPO_ROOT/VTN/fixtures/01_bl_client.sql"

if [[ ! -f "$FIXTURE" ]]; then
    echo "FAIL: fixture file not found at $FIXTURE (is the openleadr-rs submodule checked out?)"
    exit 1
fi

cd "$VTN_DIR"

echo "Stopping vtn + bff (db stays up) ..."
docker compose stop vtn bff

echo "Dropping and recreating the public schema ..."
PUBLIC_RESET_SQL="DROP SCHEMA public CASCADE; CREATE SCHEMA public;"
docker compose exec -T db psql -U "$PG_USER" "$PG_DB" -c "$PUBLIC_RESET_SQL"

if [[ "$wipe_recorder" == true ]]; then
    ROWS_SQL="SELECT coalesce(sum(n_live_tup), 0) FROM pg_stat_user_tables WHERE schemaname = 'lab_recorder'"
    rows=$(docker compose exec -T db psql -U "$PG_USER" "$PG_DB" -tAc "$ROWS_SQL" 2>/dev/null | tr -d "[:space:]")
    echo "--wipe-recorder: dropping lab_recorder (${rows:-unknown} recorded rows, not recoverable) ..."
    docker compose exec -T db psql -U "$PG_USER" "$PG_DB" -c "DROP SCHEMA IF EXISTS lab_recorder CASCADE;"
else
    echo "Preserving lab_recorder (pass --wipe-recorder to drop it too)."
fi

echo "Starting vtn (re-applies SQLx migrations on boot) ..."
docker compose up -d vtn

echo "Waiting for vtn to become healthy ..."
deadline=$((SECONDS + 60))
until curl -sf http://127.0.0.1:"${VTN_PORT:-8200}"/health >/dev/null 2>&1; do
    if [[ $SECONDS -ge $deadline ]]; then
        echo "FAIL: vtn did not become healthy within 60s"
        exit 1
    fi
    sleep 2
done

echo "Reloading test-credential fixture ..."
docker compose exec -T db psql -U "$PG_USER" "$PG_DB" < "$FIXTURE"

echo "Starting bff ..."
docker compose up -d bff

echo "Done. VTN database reset and re-seeded."
