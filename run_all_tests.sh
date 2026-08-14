#!/usr/bin/env bash
#
# Run all tests in the OpenADR Lab project.
#
# Usage:
#   bash run_all_tests.sh              # run everything (does NOT include --coverage)
#   bash run_all_tests.sh --local      # local tests only (UI unit tests)
#   bash run_all_tests.sh --e2e        # E2E behave tests only
#   bash run_all_tests.sh --resilience # resilience tests only
#   bash run_all_tests.sh --rust       # openleadr-rs cargo tests only
#   bash run_all_tests.sh --coverage   # VEN cargo-tarpaulin coverage report only (opt-in,
#                                       # GB-30 in docs/BACKLOG.md — never runs as part of
#                                       # the bare/no-flag "everything" invocation: it's a
#                                       # separate instrumented build, ~2-4x slower than a
#                                       # plain cargo test, that doesn't share cargo's
#                                       # incremental-compilation cache with the normal
#                                       # test build)
#
# Prerequisites:
#   - Node.js + npm installed locally (for UI tests)
#   - SSH access to DOCKER_HOST (or the shared OPENADR_LAB_HOST) configured
#     (when running remotely)
#   - Git repo cloned at DOCKER_DIR on the docker host
#
set -euo pipefail

# ── Configuration ────────────────────────────────────────────────────────────

# SSH hostname of the machine running Docker. Falls back to the shared
# OPENADR_LAB_HOST env var (also used by scripts/docker_host_lock.sh) before the
# hardcoded default, so the host only needs to be set in one place.
# Set to empty string "" to run docker commands directly on this machine (no SSH).
# Example remote value: "Node1"
DOCKER_HOST="${DOCKER_HOST:-${OPENADR_LAB_HOST:-Node1}}"  # override with DOCKER_HOST="" for local docker
DOCKER_DIR="/srv/docker/openadr_lab"  # repo path on the docker host

# Auto-detect: if DOCKER_HOST is empty, "localhost", or matches this machine's
# hostname, docker commands run locally without SSH.
_THIS_HOST="$(hostname -s 2>/dev/null || true)"
if [[ -z "$DOCKER_HOST" || "$DOCKER_HOST" == "localhost" || "$_THIS_HOST" == "$DOCKER_HOST" ]]; then
    _DOCKER_IS_LOCAL=true
else
    _DOCKER_IS_LOCAL=false
fi

# run_docker_cmd <cmd>  — runs a shell command on the docker host (local or remote)
run_docker_cmd() {
    if $_DOCKER_IS_LOCAL; then
        bash -c "$1"
    else
        ssh "$DOCKER_HOST" "$1"
    fi
}

# can_reach_docker  — returns 0 if the docker host is reachable
can_reach_docker() {
    if $_DOCKER_IS_LOCAL; then
        return 0
    else
        ssh -o ConnectTimeout=5 "$DOCKER_HOST" true 2>/dev/null
    fi
}

_DOCKER_LABEL="$( $_DOCKER_IS_LOCAL && echo "local" || echo "$DOCKER_HOST" )"

# Resolve real path for Windows subst drive workaround.
# Only applied on Windows (MINGW/CYGWIN/MSYS); Linux/macOS use SCRIPT_DIR as-is.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
case "$(uname -s 2>/dev/null || true)" in
    MINGW*|CYGWIN*|MSYS*)
        # On Windows subst D: -> C:\DriveD, vitest needs the real path
        if [[ "$SCRIPT_DIR" == /d/* || "$SCRIPT_DIR" == D:* ]]; then
            REAL_ROOT="${SCRIPT_DIR//\/d\//\/c\/DriveD\/}"
            REAL_ROOT="${REAL_ROOT//D:\//C:\\DriveD\\}"
        else
            REAL_ROOT="$SCRIPT_DIR"
        fi
        ;;
    *)
        REAL_ROOT="$SCRIPT_DIR"
        ;;
esac

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
RUN_COVERAGE=false   # opt-in only — never enabled by the bare/no-flag "everything" run

if [[ $# -gt 0 ]]; then
    RUN_LOCAL=false; RUN_E2E=false; RUN_RESILIENCE=false; RUN_RUST=false
    for arg in "$@"; do
        case "$arg" in
            --local)      RUN_LOCAL=true ;;
            --e2e)        RUN_E2E=true ;;
            --resilience) RUN_RESILIENCE=true ;;
            --rust)       RUN_RUST=true ;;
            --coverage)   RUN_COVERAGE=true ;;
            --help|-h)
                echo "Usage: bash run_all_tests.sh [--local] [--e2e] [--resilience] [--rust] [--coverage]"
                echo "  No flags = run all except --coverage (opt-in only). Flags can be combined."
                exit 0 ;;
            *) echo "Unknown flag: $arg"; exit 1 ;;
        esac
    done
fi

echo ""
echo -e "${BOLD}╔═══════════════════════════════════════════════════════════╗${NC}"
echo -e "${BOLD}║           OpenADR Lab — Full Test Suite                  ║${NC}"
echo -e "${BOLD}╚═══════════════════════════════════════════════════════════╝${NC}"

# ── Node1 lease lock ───────────────────────────────────────────────────────────
# The docker host is shared between worktrees/sessions; serialize access.
# Aborts (exit 2) if the lock stays held past the wait limit — rerun to retry.

if ! $_DOCKER_IS_LOCAL && { $RUN_E2E || $RUN_RESILIENCE || $RUN_RUST || $RUN_COVERAGE; }; then
    header "Acquiring $_DOCKER_LABEL lock"
    LOCK_HOST="$DOCKER_HOST" bash "$SCRIPT_DIR/scripts/docker_host_lock.sh" acquire -l 180 -m "run_all_tests.sh ${*:-all}"
    trap 'LOCK_HOST="$DOCKER_HOST" bash "$SCRIPT_DIR/scripts/docker_host_lock.sh" release' EXIT

    # GB-24: fails fast if Node2's resident fleet experiment (VEN/scale_out/node2/,
    # node2-ven-*) is running alongside genuinely tight memory — this combination
    # degraded two full E2E runs (195/269, 40/270 spurious scenario failures, both
    # environmental) instead of failing fast. A host that's merely busy for another
    # reason (single active session, no fleet containers) is NOT blocked — that case
    # is already serialized by the lock above.
    MIN_AVAILABLE_MEM_MB=800  # heuristic floor, see docs/reference/TECHNICAL_DEBTS.md
    header "Pre-flight: $_DOCKER_LABEL capacity check"
    FLEET_COUNT=$(run_docker_cmd 'docker ps --filter "label=com.docker.compose.project=node2" -q | wc -l' 2>/dev/null || echo 0)
    AVAIL_MB=$(run_docker_cmd 'free -m | awk "/^Mem:/{print \$7}"' 2>/dev/null || echo 99999)
    if [[ "$FLEET_COUNT" -gt 0 && "$AVAIL_MB" -lt "$MIN_AVAILABLE_MEM_MB" ]]; then
        echo -e "${RED}${BOLD}ABORT${NC}: $_DOCKER_LABEL has $FLEET_COUNT resident fleet VEN"
        echo "  container(s) (VEN/scale_out/node2/) running alongside only ${AVAIL_MB} MB"
        echo "  available (floor: ${MIN_AVAILABLE_MEM_MB} MB) — GB-24: this combination"
        echo "  degrades the E2E/resilience/rust suite unpredictably instead of failing"
        echo "  fast (documented: 195/269 and 40/270 spurious scenario failures, both"
        echo "  environmental)."
        echo ""
        echo "  Options: stop the fleet experiment first, run against DOCKER_HOST=Node1"
        echo "  instead, or re-run once more memory frees up."
        exit 3
    fi
    echo "  OK — ${FLEET_COUNT} fleet VEN(s) resident, ${AVAIL_MB} MB available (floor ${MIN_AVAILABLE_MEM_MB} MB)"
fi

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
    header "3. openleadr-rs Cargo Tests (docker: $_DOCKER_LABEL)"
    if can_reach_docker; then
        RUST_CMD="cd $DOCKER_DIR && docker compose -f tests/docker-compose.openleadr-test.yml run --build --rm cargo-test 2>&1; RESULT=\$?; docker compose -f tests/docker-compose.openleadr-test.yml down 2>&1; exit \$RESULT"
        if run_docker_cmd "$RUST_CMD"; then
            pass "openleadr-rs cargo tests"
        else
            fail "openleadr-rs cargo tests"
        fi
    else
        skip "openleadr-rs cargo tests (cannot reach $_DOCKER_LABEL)"
    fi
fi

# ── VEN Coverage (opt-in, GB-30) ────────────────────────────────────────────
# Never runs as part of the bare/no-flag "everything" invocation — see the
# --coverage usage note at the top of this file and docs/BACKLOG.md GB-30.

if $RUN_COVERAGE; then
    header "VEN Coverage (cargo-tarpaulin, docker: $_DOCKER_LABEL)"
    if can_reach_docker; then
        if ! $_DOCKER_IS_LOCAL; then
            echo "  Syncing latest code to $_DOCKER_LABEL..."
            run_docker_cmd "cd $DOCKER_DIR && git pull --recurse-submodules" 2>&1
        fi
        echo "  Building and running instrumented coverage build (first run ~20 min; separate cache from the normal unit-test build)..."
        COV_CMD="cd $DOCKER_DIR && docker compose -f tests/docker-compose.ven-unit-test.yml run --build --rm ven-coverage 2>&1; RESULT=\$?; docker compose -f tests/docker-compose.ven-unit-test.yml down 2>&1; exit \$RESULT"
        if run_docker_cmd "$COV_CMD"; then
            pass "VEN coverage report generated"
            echo "  Report: $DOCKER_DIR/coverage/ven/tarpaulin-report.html (+ .json) on $_DOCKER_LABEL"
        else
            fail "VEN coverage report generation"
        fi
    else
        skip "VEN coverage (cannot reach $_DOCKER_LABEL)"
    fi
fi

# ── 3. E2E Behave Tests ─────────────────────────────────────────────────────

if $RUN_E2E; then
    header "4. E2E Integration Tests (behave, docker: $_DOCKER_LABEL)"
    if can_reach_docker; then
        if ! $_DOCKER_IS_LOCAL; then
            echo "  Syncing latest code to $_DOCKER_LABEL..."
            run_docker_cmd "cd $DOCKER_DIR && git pull --recurse-submodules" 2>&1
        fi

        echo "  Building and running test stack (this may take a few minutes)..."
        # Tear down before *and* after: an interrupted prior run (e.g. a dropped SSH
        # session mid-build) can leave test-ven-1/mosquitto still running, and `docker
        # compose run` reuses already-running dependencies rather than recreating them —
        # so a stale retained MQTT reading or cached measurement state can silently carry
        # into this run otherwise (see real_measurement_mqtt.feature flakiness).
        E2E_CMD="cd $DOCKER_DIR && docker compose -f tests/docker-compose.test.yml down -v 2>&1; docker compose -f tests/docker-compose.test.yml run --build --rm test-runner 2>&1; RESULT=\$?; docker compose -f tests/docker-compose.test.yml down -v 2>&1; exit \$RESULT"
        if run_docker_cmd "$E2E_CMD"; then
            pass "E2E behave tests"
        else
            fail "E2E behave tests"
        fi
    else
        skip "E2E behave tests (cannot reach $_DOCKER_LABEL)"
    fi
fi

# ── 4. Resilience Tests ─────────────────────────────────────────────────────

if $RUN_RESILIENCE; then
    header "5. Resilience / Failure Recovery Tests (docker: $_DOCKER_LABEL)"
    if can_reach_docker; then
        echo "  Building and running resilience tests..."
        # Pre-run teardown: same interrupted-prior-run rationale as the E2E suite above.
        RES_CMD="cd $DOCKER_DIR && docker compose -f tests/docker-compose.test.yml down -v 2>&1; docker compose -f tests/docker-compose.test.yml run --build --rm test-runner --tags=@resilience 2>&1; RESULT=\$?; docker compose -f tests/docker-compose.test.yml down -v 2>&1; exit \$RESULT"
        if run_docker_cmd "$RES_CMD"; then
            pass "Resilience tests"
        else
            fail "Resilience tests"
        fi
    else
        skip "Resilience tests (cannot reach $_DOCKER_LABEL)"
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
