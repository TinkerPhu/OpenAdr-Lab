#!/usr/bin/env bash
#
# Standalone failure-recovery test script.
# Run from the repo root on Pi4-Server:
#   bash tests/failure_recovery_test.sh
#
# Tests VTN restart, VEN restart, and DB outage recovery.
#
set -euo pipefail

COMPOSE="docker compose -p openadr-test -f tests/docker-compose.test.yml"
VTN_URL="http://localhost:3000"
VEN1_URL="http://localhost:8080"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

pass() { echo -e "${GREEN}✓ $1${NC}"; }
fail() { echo -e "${RED}✗ $1${NC}"; exit 1; }
info() { echo -e "${YELLOW}▸ $1${NC}"; }

wait_for_url() {
    local url=$1 timeout=${2:-60}
    local deadline=$((SECONDS + timeout))
    while [ $SECONDS -lt $deadline ]; do
        if curl -sf "$url" > /dev/null 2>&1; then return 0; fi
        sleep 2
    done
    return 1
}

get_token() {
    curl -sf -X POST "$VTN_URL/auth/token" \
        -d "grant_type=client_credentials&client_id=any-business&client_secret=any-business" \
        | python3 -c "import sys,json; print(json.load(sys.stdin)['access_token'])"
}

vtn_post() {
    local path=$1 token=$2; shift 2
    curl -sf -X POST "$VTN_URL$path" \
        -H "Authorization: Bearer $token" \
        -H "Content-Type: application/json" \
        "$@"
}

vtn_get() {
    local path=$1 token=$2
    curl -sf "$VTN_URL$path" -H "Authorization: Bearer $token"
}

ven_has_event() {
    local ven_url=$1 event_name=$2 timeout=${3:-30}
    local deadline=$((SECONDS + timeout))
    while [ $SECONDS -lt $deadline ]; do
        if curl -sf "$ven_url/events" 2>/dev/null | grep -q "\"eventName\":\"$event_name\""; then
            return 0
        fi
        sleep 3
    done
    return 1
}

# ──────────────────────────────────────────────────────────────────────────────
# Note: This script assumes the test stack is already running.
# If not, bring it up first with:
#   docker compose -p openadr-test -f tests/docker-compose.test.yml up -d --build
# ──────────────────────────────────────────────────────────────────────────────

echo ""
echo "═══════════════════════════════════════════════════════════════"
echo "  OpenADR Failure Recovery Tests"
echo "═══════════════════════════════════════════════════════════════"
echo ""

# Verify services are up
info "Checking services are healthy..."
wait_for_url "$VTN_URL/health" 10 || fail "VTN not running — start stack first"
pass "All services healthy"

TOKEN=$(get_token) || fail "Cannot get VTN token"

# ── Test 1: VEN retains cache during VTN outage ────────────────────────────
info "Test 1: VEN retains cached events when VTN goes down"

PROG_ID=$(vtn_post "/programs" "$TOKEN" -d '{"programName":"fr-cache-test"}' \
    | python3 -c "import sys,json; print(json.load(sys.stdin)['id'])")

vtn_post "/events" "$TOKEN" \
    -d "{\"programID\":\"$PROG_ID\",\"eventName\":\"fr-cache-evt\",\"intervals\":[{\"id\":0,\"payloads\":[{\"type\":\"SIMPLE\",\"values\":[1.0]}]}]}" \
    > /dev/null

ven_has_event "$VEN1_URL" "fr-cache-evt" 30 || fail "VEN-1 never synced event"

$COMPOSE stop -t 5 test-vtn > /dev/null 2>&1

# VEN should still serve cached data
if curl -sf "$VEN1_URL/events" | grep -q "fr-cache-evt"; then
    pass "VEN-1 serves cached events while VTN is down"
else
    $COMPOSE start test-vtn > /dev/null 2>&1
    fail "VEN-1 lost cached events when VTN went down"
fi

$COMPOSE start test-vtn > /dev/null 2>&1
wait_for_url "$VTN_URL/health" 60 || fail "VTN did not recover"
pass "Test 1 passed"

# ── Test 2: VEN re-syncs after VTN restart ─────────────────────────────────
info "Test 2: VEN re-syncs after VTN restart"

TOKEN=$(get_token)
PROG_ID2=$(vtn_post "/programs" "$TOKEN" -d '{"programName":"fr-resync-test"}' \
    | python3 -c "import sys,json; print(json.load(sys.stdin)['id'])")

$COMPOSE restart -t 5 test-vtn > /dev/null 2>&1
wait_for_url "$VTN_URL/health" 60 || fail "VTN did not restart"
TOKEN=$(get_token)

vtn_post "/events" "$TOKEN" \
    -d "{\"programID\":\"$PROG_ID2\",\"eventName\":\"fr-resync-evt\",\"intervals\":[{\"id\":0,\"payloads\":[{\"type\":\"SIMPLE\",\"values\":[1.0]}]}]}" \
    > /dev/null

ven_has_event "$VEN1_URL" "fr-resync-evt" 30 || fail "VEN-1 did not re-sync after VTN restart"
pass "Test 2 passed"

# ── Test 3: VEN recovers after its own restart ─────────────────────────────
info "Test 3: VEN recovers after its own restart"

$COMPOSE restart -t 5 test-ven-1 > /dev/null 2>&1
wait_for_url "$VEN1_URL/health" 60 || fail "VEN-1 did not restart"

ven_has_event "$VEN1_URL" "fr-resync-evt" 30 || fail "VEN-1 did not recover events after restart"
pass "Test 3 passed"

# ── Test 4: VTN recovers after DB restart ──────────────────────────────────
info "Test 4: VTN recovers after DB restart"

$COMPOSE restart -t 5 test-db > /dev/null 2>&1
sleep 5  # Give DB time to accept connections

wait_for_url "$VTN_URL/health" 60 || fail "VTN did not recover after DB restart"
TOKEN=$(get_token)

PROGRAMS=$(vtn_get "/programs" "$TOKEN")
if echo "$PROGRAMS" | grep -q "fr-resync-test"; then
    pass "VTN serves data after DB restart"
else
    fail "VTN lost data after DB restart"
fi
pass "Test 4 passed"

echo ""
echo "═══════════════════════════════════════════════════════════════"
echo -e "  ${GREEN}All failure recovery tests passed${NC}"
echo "═══════════════════════════════════════════════════════════════"
echo ""
