#!/usr/bin/env bash
# GB-06 (Phase 2, WP2.5) — drop and re-seed the VTN's Postgres database for a
# clean bring-up, instead of the manual `docker exec ... psql < fixtures.sql`
# steps in VTN/vtn_setup_from_blog_step_by_step.md. Idempotent: safe to run
# against a fresh DB (nothing to drop yet) or a populated one.
#
# Usage: bash scripts/db_reset.sh
#
# Drops and recreates the `public` schema (openleadr-rs's own tables,
# SQLx-migrated back in on VTN restart) and the `lab_recorder` schema
# (Phase 1 BFF recorder, re-created on BFF restart). Reloads the
# test-credential fixture (ven-manager/any-business/user-manager users) that
# `provision_vens`/`fleet.sh` authenticate as.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VTN_DIR="$REPO_ROOT/VTN"
PG_USER="${PG_USER:-openadr}"
PG_DB="${PG_DB:-openadr}"
FIXTURE="$REPO_ROOT/openleadr-rs/fixtures/test_user_credentials.sql"

if [[ ! -f "$FIXTURE" ]]; then
    echo "FAIL: fixture file not found at $FIXTURE (is the openleadr-rs submodule checked out?)"
    exit 1
fi

cd "$VTN_DIR"

echo "Stopping vtn + bff (db stays up) ..."
docker compose stop vtn bff

echo "Dropping and recreating public + lab_recorder schemas ..."
docker compose exec -T db psql -U "$PG_USER" "$PG_DB" -c \
    "DROP SCHEMA public CASCADE; CREATE SCHEMA public; DROP SCHEMA IF EXISTS lab_recorder CASCADE;"

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
