#!/bin/sh
set -e

# Ensure test VEN/BFF/UI service containers are running under their standard
# names (openadr-test-test-ven-1-1 etc.) regardless of whether they were started
# by `docker compose up` or as implicit dependencies of `docker compose run`.
# `docker compose up -d` is idempotent: already-running containers are left alone.
echo "Ensuring service containers are up with standard names..."
docker compose -f /tests/docker-compose.test.yml up -d \
  test-ven-1 test-ven-2 test-ven-no-pv test-vtn test-db test-bff test-ui test-ven-ui \
  2>/dev/null || true
echo "Services up."

echo "Loading test fixtures into VTN database..."
PGPASSWORD=openadr psql -h test-db -U openadr -d openadr \
  -f /fixtures/test_user_credentials.sql
echo "Fixtures loaded."

echo "Provisioning ven-2 via API..."
python provision_ven2.py
echo "Provisioning done."

exec python -m behave "$@"
