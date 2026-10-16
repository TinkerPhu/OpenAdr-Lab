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

# The load-settle gate before each timing-sensitive pass (see settle.sh).
. ./settle.sh

# Structural gate before the expensive passes. `--dry-run` resolves every step
# in every feature without touching the stack, so an undefined or misspelled
# step is reported in seconds instead of forty minutes into a run -- and an
# overlapping step pattern (behave resolves those by registration order rather
# than erroring) shows up as the wrong definition being bound.
echo "=== Resolving step definitions (dry run) ==="
if ! python -m behave --dry-run --format null --no-summary > /tmp/dryrun.out 2>&1; then
  echo "step definitions do not resolve — not starting the suite:"
  tail -30 /tmp/dryrun.out
  exit 1
fi
if grep -qiE "undefined|ambiguous" /tmp/dryrun.out; then
  echo "undefined or ambiguous steps — not starting the suite:"
  grep -iE "undefined|ambiguous" /tmp/dryrun.out | head -20
  exit 1
fi
echo "  OK — every step resolves"

wait_for_load_to_settle "main pass"

# Run main suite first, excluding timing-sensitive @isolated scenarios.
# Then run @isolated scenarios in a second pass so each gets a fresh VEN
# state and is not affected by Node1 resource contention from prior scenarios.
set +e
# Every pass also records its failed scenarios (behave's `rerun` formatter writes
# feature:line locations), so a failure gets exactly one retry on its own below.
RERUN_MAIN=/tmp/rerun_main.txt
RERUN_ISOLATED=/tmp/rerun_isolated.txt
python -m behave -f rerun -o "$RERUN_MAIN" -f pretty --tags=~@isolated --exclude "isolated" "$@"
MAIN_EXIT=$?

echo ""
wait_for_load_to_settle "@isolated pass"

echo "=== Running @isolated scenarios (fresh VEN state) ==="
# Point directly at the dedicated isolated/ subdirectory. Every scenario in
# that directory is @isolated, so nothing is loaded-but-skipped: zero structural
# skips in the summary.
python -m behave -f rerun -o "$RERUN_ISOLATED" -f pretty features/isolated/ "$@"
ISOLATED_EXIT=$?

# Flaky versus red: see retry_failed.sh.
. ./retry_failed.sh
retry_failed_scenarios main "$MAIN_EXIT" "$RERUN_MAIN" "$@" && MAIN_EXIT=0
retry_failed_scenarios isolated "$ISOLATED_EXIT" "$RERUN_ISOLATED" "$@" && ISOLATED_EXIT=0

exit $((MAIN_EXIT | ISOLATED_EXIT))
