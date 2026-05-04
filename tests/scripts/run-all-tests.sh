#!/bin/bash
# Master test runner: Rust unit tests → Vitest → BDD integration tests
# Prints a combined timing summary at the end.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
OVERALL_START=$(date +%s)
FAILURES=""

# ---------------------------------------------------------------------------
# Tier 1: Rust unit tests
# ---------------------------------------------------------------------------
echo ""
echo "========================================================================"
echo "TIER 1: Rust unit tests"
echo "========================================================================"
RUST_BUILD_TIME_S=0
RUST_TEST_TOTAL_MS=0
RUST_TEST_CONTAINER_TIME_S=0
RUST_EXIT=0

bash "$SCRIPT_DIR/run-rust-tests.sh" 2>&1 | tee /tmp/rust-tests.log || RUST_EXIT=$?

# Extract timing vars printed by run-rust-tests.sh
eval "$(grep -E '^RUST_(BUILD_TIME_S|TEST_TOTAL_MS|TEST_CONTAINER_TIME_S)=' /tmp/rust-tests.log || true)"

if [ $RUST_EXIT -ne 0 ]; then
  FAILURES="$FAILURES Rust"
fi

# ---------------------------------------------------------------------------
# Tier 2: Vitest (VEN/ui + VTN/ui)
# ---------------------------------------------------------------------------
echo ""
echo "========================================================================"
echo "TIER 2: Vitest (VEN/ui + VTN/ui)"
echo "========================================================================"
VITEST_EXIT=0

bash "$SCRIPT_DIR/run-vitest.sh" 2>&1 | tee /tmp/vitest.log || VITEST_EXIT=$?

# Extract timing vars printed by run-vitest.sh
eval "$(grep -E '^VITEST_(BUILD|CONTAINER|EXIT)_' /tmp/vitest.log || true)"
VITEST_BUILD_ven_ui_S="${VITEST_BUILD_ven_ui_S:-0}"
VITEST_CONTAINER_ven_ui_S="${VITEST_CONTAINER_ven_ui_S:-0}"
VITEST_BUILD_vtn_ui_S="${VITEST_BUILD_vtn_ui_S:-0}"
VITEST_CONTAINER_vtn_ui_S="${VITEST_CONTAINER_vtn_ui_S:-0}"

if [ $VITEST_EXIT -ne 0 ]; then
  FAILURES="$FAILURES Vitest"
fi

# ---------------------------------------------------------------------------
# Tier 3: BDD integration tests (behave)
# ---------------------------------------------------------------------------
echo ""
echo "========================================================================"
echo "TIER 3: BDD integration tests"
echo "========================================================================"
BDD_START=$(date +%s)
BDD_EXIT=0

cd "$REPO_ROOT"
docker compose -f tests/docker-compose.test.yml run --build --rm test-runner 2>&1 \
  | tee /tmp/bdd.log || BDD_EXIT=$?

BDD_END=$(date +%s)
BDD_WALL_S=$((BDD_END - BDD_START))

if [ $BDD_EXIT -ne 0 ]; then
  FAILURES="$FAILURES BDD"
fi

# Extract BDD scenario counts from behave summary line
BDD_SUMMARY=$(grep -E '^[0-9]+ feature' /tmp/bdd.log | tail -1 || true)

# Extract BDD total accounted time from timing summary printed by environment.py
BDD_ACCOUNTED_S=$(grep 'TOTAL ACCOUNTED:' /tmp/bdd.log | grep -oE '[0-9]+\.[0-9]+' | head -1 || true)
BDD_SCENARIO_COUNT=$(grep 'Scenarios:' /tmp/bdd.log | grep -oE 'Scenarios: [0-9]+' | grep -oE '[0-9]+' | head -1 || true)

# ---------------------------------------------------------------------------
# Combined timing summary
# ---------------------------------------------------------------------------
OVERALL_END=$(date +%s)
OVERALL_WALL_S=$((OVERALL_END - OVERALL_START))

RUST_TEST_S=$(( RUST_TEST_TOTAL_MS / 1000 ))

echo ""
echo "========================================================================"
echo "TEST TIMING SUMMARY"
echo "========================================================================"
printf "%-28s  %s\n" "Rust unit tests:" \
  "build=${RUST_BUILD_TIME_S}s  tests=${RUST_TEST_S}s  container=${RUST_TEST_CONTAINER_TIME_S}s"
printf "%-28s  %s\n" "VEN/ui vitest:" \
  "build=${VITEST_BUILD_ven_ui_S}s  tests=${VITEST_CONTAINER_ven_ui_S}s"
printf "%-28s  %s\n" "VTN/ui vitest:" \
  "build=${VITEST_BUILD_vtn_ui_S}s  tests=${VITEST_CONTAINER_vtn_ui_S}s"
printf "%-28s  %s\n" "BDD integration (wall):" \
  "${BDD_WALL_S}s$([ -n "$BDD_ACCOUNTED_S" ] && echo "  accounted=${BDD_ACCOUNTED_S}s")$([ -n "$BDD_SCENARIO_COUNT" ] && echo "  scenarios=${BDD_SCENARIO_COUNT}")"
[ -n "$BDD_SUMMARY" ] && echo "  Behave: $BDD_SUMMARY"
echo "------------------------------------------------------------------------"
printf "%-28s  %s\n" "TOTAL WALL CLOCK:" "${OVERALL_WALL_S}s  ($(( OVERALL_WALL_S / 60 ))m $(( OVERALL_WALL_S % 60 ))s)"

if [ -n "$FAILURES" ]; then
  echo ""
  echo "FAILED TIERS:$FAILURES"
  exit 1
else
  echo ""
  echo "All tiers passed."
fi
