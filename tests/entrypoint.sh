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

# Run main suite first, excluding timing-sensitive @isolated scenarios.
# Then run @isolated scenarios in a second pass so each gets a fresh VEN
# state and is not affected by Pi4 resource contention from prior scenarios.
set +e
python -m behave --tags=~@isolated "$@"
MAIN_EXIT=$?

echo ""
echo "=== Running @isolated scenarios (fresh VEN state) ==="
# Only load feature files that actually contain @isolated scenarios so that
# the other ~240 scenarios are never loaded and never counted as skipped.
ISOLATED_FILES=$(grep -rl "@isolated" features/ | tr '\n' ' ')
python -m behave $ISOLATED_FILES --tags=@isolated "$@"
ISOLATED_EXIT=$?

exit $((MAIN_EXIT | ISOLATED_EXIT))
