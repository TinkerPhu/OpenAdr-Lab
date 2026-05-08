#!/bin/sh
set -e

# Reload nginx in test-ui to force DNS re-resolution of backend upstreams.
# Without this, nginx caches the BFF IP at container startup; if the BFF
# container is ever recreated (different IP), nginx keeps connecting to the
# stale IP and every /api/* call returns 502.
echo "Reloading test-ui nginx (refresh upstream DNS)..."
docker exec openadr-test-test-ui-1 nginx -s reload 2>/dev/null || true
echo "nginx reloaded."

echo "Loading test fixtures into VTN database..."
PGPASSWORD=openadr psql -h test-db -U openadr -d openadr \
  -f /fixtures/test_user_credentials.sql
echo "Fixtures loaded."

echo "Provisioning ven-2 via API..."
python provision_ven2.py
echo "Provisioning done."

exec python -m behave "$@"
