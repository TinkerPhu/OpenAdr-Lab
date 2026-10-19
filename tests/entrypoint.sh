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

# Every VEN of the stack has its own identity (R-152): the CLIENT_IDs of
# docker-compose.test.yml, one VTN user and VEN object each.
for ven in ven-1 ven-2 ven-no-pv ven-multi ven-penalty ven-usage-sim ven-usage-forecast; do
  echo "Provisioning $ven via API..."
  python provision_ven.py "$ven"
done
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
: > "$RERUN_MAIN"
: > "$RERUN_ISOLATED"

# Feature paths pick what each pass runs, so `test-runner features/x.feature` runs x alone;
# every other argument (tags, formats) stays in "$@" and applies to every pass and retry.
MAIN_PATHS='' ISOLATED_PATHS='' PATHS_GIVEN=''
for arg; do
  shift
  case "$arg" in
    features/isolated/*) ISOLATED_PATHS="$ISOLATED_PATHS $arg"; PATHS_GIVEN=1 ;;
    features/*) MAIN_PATHS="$MAIN_PATHS $arg"; PATHS_GIVEN=1 ;;
    *) set -- "$@" "$arg" ;;
  esac
done
[ -n "$PATHS_GIVEN" ] || ISOLATED_PATHS=features/isolated/

MAIN_EXIT=0
if [ -z "$PATHS_GIVEN" ] || [ -n "$MAIN_PATHS" ]; then
  # shellcheck disable=SC2086  # feature paths carry no spaces; empty runs every feature
  python -m behave -f rerun -o "$RERUN_MAIN" -f pretty --tags=~@isolated --exclude "isolated" "$@" $MAIN_PATHS
  MAIN_EXIT=$?
fi

ISOLATED_EXIT=0
if [ -n "$ISOLATED_PATHS" ]; then
  echo ""
  wait_for_load_to_settle "@isolated pass"

  echo "=== Running @isolated scenarios (fresh VEN state) ==="
  # Point directly at the dedicated isolated/ subdirectory. Every scenario in
  # that directory is @isolated, so nothing is loaded-but-skipped: zero structural
  # skips in the summary.
  # shellcheck disable=SC2086
  python -m behave -f rerun -o "$RERUN_ISOLATED" -f pretty $ISOLATED_PATHS "$@"
  ISOLATED_EXIT=$?
fi

# Flaky versus red: see retry_failed.sh.
. ./retry_failed.sh
retry_failed_scenarios main "$MAIN_EXIT" "$RERUN_MAIN" "$@" && MAIN_EXIT=0
retry_failed_scenarios isolated "$ISOLATED_EXIT" "$RERUN_ISOLATED" "$@" && ISOLATED_EXIT=0

exit $((MAIN_EXIT | ISOLATED_EXIT))
