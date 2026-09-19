#!/bin/sh
set -e

# Reload nginx in test-ui to force DNS re-resolution of backend upstreams.
# Without this, nginx caches the BFF IP at container startup; if the BFF
# container is ever recreated (different IP), nginx keeps connecting to the
# stale IP and every /api/* call returns 502.
echo "Reloading test-ui nginx (refresh upstream DNS)..."
docker exec openadr-test-test-ui-1 nginx -s reload 2>/dev/null || true
echo "nginx reloaded."

# OpenADR 3.1: the lab owns this fixture and it seeds exactly one user, the
# bl-client business credential. Everything else -- the VEN users, their
# credentials and their VEN objects -- is created through the API below, so the
# VTN hashes each secret itself and no password hash is maintained in the repo.
#
# The legacy ven-1 cleanup that used to live here is gone with 3.0: it deleted
# from user_ven, a table the 3.1 migration drops, and it existed to undo rows
# seeded by upstream's shared test fixture, which this stack no longer loads.
echo "Loading the bl-client fixture into the VTN database..."
PGPASSWORD=openadr psql -h test-db -U openadr -d openadr -v ON_ERROR_STOP=1 -f /fixtures/01_bl_client.sql
echo "Fixture loaded."

echo "Provisioning ven-1 via API..."
python provision_ven.py ven-1
echo "Provisioning ven-2 via API..."
python provision_ven.py ven-2
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
