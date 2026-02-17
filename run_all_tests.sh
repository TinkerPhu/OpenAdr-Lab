#!/usr/bin/env bash
#
# Run all tests in the OpenADR Lab project.
#
# Usage:
#   bash run_all_tests.sh              # run everything
#   bash run_all_tests.sh --local      # local tests only (UI unit tests)
#   bash run_all_tests.sh --e2e        # E2E behave tests only (on Pi4)
#   bash run_all_tests.sh --resilience # resilience tests only (on Pi4)
#   bash run_all_tests.sh --rust       # openleadr-rs cargo tests only (on Pi4)
#
# Prerequisites:
#   - Node.js + npm installed locally (for UI tests)
#   - SSH access to Pi4-Server configured
#   - Git repo cloned at /srv/docker/openadr_lab on Pi4-Server
#
set -euo pipefail

# ── Configuration ────────────────────────────────────────────────────────────

PI4_HOST="Pi4-Server"
PI4_DIR="/srv/docker/openadr_lab"

# Resolve real path for Windows subst drive workaround
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# On Windows subst D: -> C:\DriveD, vitest needs the real path
if [[ "$SCRIPT_DIR" == /d/* || "$SCRIPT_DIR" == D:* ]]; then
    REAL_ROOT="${SCRIPT_DIR//\/d\//\/c\/DriveD\/}"
    REAL_ROOT="${REAL_ROOT//D:\//C:\\DriveD\\}"
else
    REAL_ROOT="$SCRIPT_DIR"
fi

# ── Colors ───────────────────────────────────────────────────────────────────

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

PASS_COUNT=0
FAIL_COUNT=0
SKIP_COUNT=0
RESULTS=()

pass()  { PASS_COUNT=$((PASS_COUNT + 1)); RESULTS+=("${GREEN}PASS${NC}  $1"); echo -e "  ${GREEN}PASS${NC}  $1"; }
fail()  { FAIL_COUNT=$((FAIL_COUNT + 1)); RESULTS+=("${RED}FAIL${NC}  $1"); echo -e "  ${RED}FAIL${NC}  $1"; }
skip()  { SKIP_COUNT=$((SKIP_COUNT + 1)); RESULTS+=("${YELLOW}SKIP${NC}  $1"); echo -e "  ${YELLOW}SKIP${NC}  $1"; }
header(){ echo -e "\n${CYAN}${BOLD}═══ $1 ═══${NC}\n"; }

# ── Parse arguments ──────────────────────────────────────────────────────────

RUN_LOCAL=true
RUN_E2E=true
RUN_RESILIENCE=true
RUN_RUST=true

if [[ $# -gt 0 ]]; then
    RUN_LOCAL=false; RUN_E2E=false; RUN_RESILIENCE=false; RUN_RUST=false
    for arg in "$@"; do
        case "$arg" in
            --local)      RUN_LOCAL=true ;;
            --e2e)        RUN_E2E=true ;;
            --resilience) RUN_RESILIENCE=true ;;
            --rust)       RUN_RUST=true ;;
            --help|-h)
                echo "Usage: bash run_all_tests.sh [--local] [--e2e] [--resilience] [--rust]"
                echo "  No flags = run all. Flags can be combined."
                exit 0 ;;
            *) echo "Unknown flag: $arg"; exit 1 ;;
        esac
    done
fi

echo ""
echo -e "${BOLD}╔═══════════════════════════════════════════════════════════╗${NC}"
echo -e "${BOLD}║           OpenADR Lab — Full Test Suite                  ║${NC}"
echo -e "${BOLD}╚═══════════════════════════════════════════════════════════╝${NC}"

# ── 1. Local UI Unit Tests ───────────────────────────────────────────────────

if $RUN_LOCAL; then
    header "1. VEN UI Unit Tests (vitest)"
    if command -v npm &> /dev/null; then
        VEN_UI_DIR="$SCRIPT_DIR/VEN/ui"
        if [[ -d "$VEN_UI_DIR/node_modules" ]]; then
            if (cd "$VEN_UI_DIR" && npm test 2>&1); then
                pass "VEN UI unit tests"
            else
                fail "VEN UI unit tests"
            fi
        else
            echo "  node_modules not found — running npm install first..."
            (cd "$VEN_UI_DIR" && npm install && npm test 2>&1) && pass "VEN UI unit tests" || fail "VEN UI unit tests"
        fi
    else
        skip "VEN UI unit tests (npm not found)"
    fi

    header "2. VTN UI Unit Tests (vitest)"
    if command -v npm &> /dev/null; then
        VTN_UI_DIR="$SCRIPT_DIR/VTN/ui"
        if [[ -d "$VTN_UI_DIR/node_modules" ]]; then
            if (cd "$VTN_UI_DIR" && npm test 2>&1); then
                pass "VTN UI unit tests"
            else
                fail "VTN UI unit tests"
            fi
        else
            echo "  node_modules not found — running npm install first..."
            (cd "$VTN_UI_DIR" && npm install && npm test 2>&1) && pass "VTN UI unit tests" || fail "VTN UI unit tests"
        fi
    else
        skip "VTN UI unit tests (npm not found)"
    fi
fi

# ── 2. openleadr-rs Cargo Tests ─────────────────────────────────────────────

if $RUN_RUST; then
    header "3. openleadr-rs Cargo Tests (on Pi4-Server)"
    if ssh -o ConnectTimeout=5 "$PI4_HOST" true 2>/dev/null; then
        RUST_CMD="cd $PI4_DIR && docker compose -f tests/docker-compose.cargo-test.yml run --build --rm cargo-test 2>&1; RESULT=\$?; docker compose -f tests/docker-compose.cargo-test.yml down 2>&1; exit \$RESULT"
        if ssh "$PI4_HOST" "$RUST_CMD" 2>&1; then
            pass "openleadr-rs cargo tests"
        else
            fail "openleadr-rs cargo tests"
        fi
    else
        skip "openleadr-rs cargo tests (cannot reach Pi4-Server)"
    fi
fi

# ── 3. E2E Behave Tests ─────────────────────────────────────────────────────

if $RUN_E2E; then
    header "4. E2E Integration Tests (behave on Pi4-Server)"
    if ssh -o ConnectTimeout=5 "$PI4_HOST" true 2>/dev/null; then
        echo "  Syncing latest code to Pi4..."
        ssh "$PI4_HOST" "cd $PI4_DIR && git pull --recurse-submodules" 2>&1

        echo "  Building and running test stack (this may take a few minutes)..."
        E2E_CMD="cd $PI4_DIR && docker compose -f tests/docker-compose.test.yml run --build --rm test-runner 2>&1; RESULT=\$?; docker compose -f tests/docker-compose.test.yml down -v 2>&1; exit \$RESULT"
        if ssh "$PI4_HOST" "$E2E_CMD" 2>&1; then
            pass "E2E behave tests"
        else
            fail "E2E behave tests"
        fi
    else
        skip "E2E behave tests (cannot reach Pi4-Server)"
    fi
fi

# ── 4. Resilience Tests ─────────────────────────────────────────────────────

if $RUN_RESILIENCE; then
    header "5. Resilience / Failure Recovery Tests (behave on Pi4-Server)"
    if ssh -o ConnectTimeout=5 "$PI4_HOST" true 2>/dev/null; then
        echo "  Building and running resilience tests..."
        RES_CMD="cd $PI4_DIR && docker compose -f tests/docker-compose.test.yml run --build --rm test-runner --tags=@resilience 2>&1; RESULT=\$?; docker compose -f tests/docker-compose.test.yml down -v 2>&1; exit \$RESULT"
        if ssh "$PI4_HOST" "$RES_CMD" 2>&1; then
            pass "Resilience tests"
        else
            fail "Resilience tests"
        fi
    else
        skip "Resilience tests (cannot reach Pi4-Server)"
    fi
fi

# ── Summary ──────────────────────────────────────────────────────────────────

echo ""
echo -e "${BOLD}╔═══════════════════════════════════════════════════════════╗${NC}"
echo -e "${BOLD}║                    Test Summary                          ║${NC}"
echo -e "${BOLD}╠═══════════════════════════════════════════════════════════╣${NC}"
for r in "${RESULTS[@]}"; do
    echo -e "${BOLD}║${NC}  $r"
done
echo -e "${BOLD}╠═══════════════════════════════════════════════════════════╣${NC}"
echo -e "${BOLD}║${NC}  ${GREEN}$PASS_COUNT passed${NC}  ${RED}$FAIL_COUNT failed${NC}  ${YELLOW}$SKIP_COUNT skipped${NC}"
echo -e "${BOLD}╚═══════════════════════════════════════════════════════════╝${NC}"
echo ""

if [[ $FAIL_COUNT -gt 0 ]]; then
    exit 1
fi
