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

# GB-02/GB-03: the fixture (shared with openleadr-rs's own CI/tests, so left
# untouched) seeds ven-1 with a legacy literal id "ven-1" and venName
# "ven-1-name". Clear those rows so ven-1 can be re-provisioned via the API
# below and get a real UUID id + uniform "ven-1" venName, same as ven-2/ven-3.
echo "Clearing legacy fixture-seeded ven-1 rows..."
PGPASSWORD=openadr psql -h test-db -U openadr -d openadr <<'SQL'
DELETE FROM user_ven WHERE ven_id = 'ven-1';
DELETE FROM user_credentials WHERE user_id = 'ven-1-user';
DELETE FROM "user" WHERE id = 'ven-1-user';
DELETE FROM ven WHERE id = 'ven-1';
SQL
echo "Legacy ven-1 rows cleared."

echo "Provisioning ven-1 via API..."
python provision_ven1.py
echo "Provisioning ven-2 via API..."
python provision_ven2.py
echo "Provisioning done."

# Both timing-sensitive @isolated scenarios and the resilience suite
# (features/ven_resilience.feature, run separately via `--tags=@resilience`)
# flake under Node1 resource contention from whatever ran just before this
# container started (a prior E2E/rust/UI section in the same run_all_tests.sh
# invocation). Containers share the host kernel, so /proc/loadavg is the real
# Node1 load. Wait for it to settle (1-min load < 2.0), capped at 8 minutes,
# before any behave pass that hasn't already paid this cost this invocation.
wait_for_load_to_settle() {
  label="$1"
  echo "=== Waiting for host load to settle before $label ==="
  SETTLE_DEADLINE=$(( $(date +%s) + 480 ))
  while :; do
    LOAD1=$(cut -d' ' -f1 /proc/loadavg)
    if [ "$(awk -v l="$LOAD1" 'BEGIN { print (l < 2.0) ? 1 : 0 }')" = "1" ]; then
      echo "Host load settled at $LOAD1."
      break
    fi
    if [ "$(date +%s)" -ge "$SETTLE_DEADLINE" ]; then
      echo "Host load still $LOAD1 after 8 min — proceeding anyway."
      break
    fi
    echo "  load $LOAD1 >= 2.0 — waiting 15 s"
    sleep 15
  done
}

wait_for_load_to_settle "main pass"

# Run main suite first, excluding timing-sensitive @isolated scenarios.
# Then run @isolated scenarios in a second pass so each gets a fresh VEN
# state and is not affected by Node1 resource contention from prior scenarios.
set +e
python -m behave --tags=~@isolated --exclude "isolated" "$@"
MAIN_EXIT=$?

echo ""
wait_for_load_to_settle "@isolated pass"

echo "=== Running @isolated scenarios (fresh VEN state) ==="
# Point directly at the dedicated isolated/ subdirectory. Every scenario in
# that directory is @isolated, so nothing is loaded-but-skipped: zero structural
# skips in the summary.
python -m behave features/isolated/ "$@"
ISOLATED_EXIT=$?

exit $((MAIN_EXIT | ISOLATED_EXIT))
