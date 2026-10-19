#!/usr/bin/env bash
# Does the runner spread a ticket's suites over lock-guarded lanes and report it?
#
# Sources the runner as a library (RUNNER_NO_MAIN=1) and drives lane planning, lane acquisition and
# the lane jobs with the host-touching functions (ssh, the lock script, run_all_tests.sh, docker)
# replaced by stubs that log what they were asked and behave by a table. Pins: the median per suite
# and host, longest-first planning, a busy host is skipped without waiting, a failing suite cancels
# the other lanes and they are tidied before their lock is released, an infrastructure failure
# moves the suite to the home lane, and the Lanes block and per-lane logs reach result.md.
#
# Usage:  bash ci/test_lanes.sh
set -uo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
export CI_ROOT="$TMP/ci" CI_REPO="$TMP/repo" BOARD_DIR="$TMP/board" RUNNER_NO_MAIN=1
export CI_HOME_NAME=Node3 CI_LANE_HOSTS="Node2 Node1" CI_LANE_POLL_S=1 CI_LANE_HEARTBEAT_S=3600
mkdir -p "$BOARD_DIR" "$CI_REPO"
# shellcheck source=runner.sh
source "$SCRIPT_DIR/runner.sh"
set +e

FAILS=0
check() { # $1=name  $2=expected  $3=actual
    if [ "$2" = "$3" ]; then echo "  ok    $1"
    else echo "  FAIL  $1 (got '$3', want '$2')"; FAILS=$((FAILS + 1)); fi
}

echo "board_suite_seconds"
ticket() { # <name> <status> <result rows...>
    local n="$1" st="$2"; shift 2
    mkdir -p "$BOARD_DIR/$n"; echo "$st" > "$BOARD_DIR/$n/status"
    { echo "# CI result: $n"; echo; echo "| step | exit | seconds |"; echo "|---|---|---|"; printf '%s\n' "$@"; } > "$BOARD_DIR/$n/result.md"
}
check "no history: the fallback"            3300 "$(board_suite_seconds e2e Node3)"
check "no history, resilience fallback"     350 "$(board_suite_seconds resilience Node3)"
check "no history, rust-vtn fallback"        100 "$(board_suite_seconds rust-vtn Node3)"
ticket 20261001T0000Z_a_x merged "| run_all_tests.sh --e2e [Node3] | 0 | 3000 |"
ticket 20261002T0000Z_a_x merged "| run_all_tests.sh --e2e [Node3] | 0 | 3200 |"
ticket 20261003T0000Z_a_x green  "| run_all_tests.sh --e2e [Node3] | 0 | 3400 |"
ticket 20261004T0000Z_a_x red    "| run_all_tests.sh --e2e [Node3] | 1 | 100 |"
ticket 20261005T0000Z_a_x queued "| run_all_tests.sh --e2e [Node3] | 0 | 9999 |"
ticket 20261006T0000Z_a_x merged "| run_all_tests.sh --e2e [Node2] | 0 | 5000 |"
check "median of a host with 3 passing rows" 3200 "$(board_suite_seconds e2e Node3)"
check "a failed or unfinished row is ignored" 3200 "$(board_suite_seconds e2e Node3)"
check "a host with fewer than 3 rows uses the median over all hosts" 3200 "$(board_suite_seconds e2e Node2)"
check "a suite never timed on a host uses its fallback" 350 "$(board_suite_seconds resilience Node2)"

echo "lane_plan"
lane_suite_seconds() { # <suite> <host> - a table, as board_suite_seconds would answer
    case "$1" in e2e) echo 3300 ;; resilience) echo 900 ;; rust-vtn) echo 600 ;; esac
}
check "three suites, three lanes" "Node3: e2e|Node2: resilience|Node1: rust-vtn" \
    "$(lane_plan 'Node3 Node2 Node1' 'rust-vtn resilience e2e' | tr '\n' '|' | sed 's/|$//')"
check "three suites, two lanes" "Node3: e2e|Node2: resilience rust-vtn" \
    "$(lane_plan 'Node3 Node2' 'resilience rust-vtn e2e' | tr '\n' '|' | sed 's/|$//')"
check "one suite, one lane" "Node3: e2e" "$(lane_plan 'Node3 Node2 Node1' 'e2e')"
check "one lane takes everything, longest first" "Node3: e2e resilience rust-vtn" \
    "$(lane_plan 'Node3' 'rust-vtn resilience e2e')"

echo "lane_unit_cost: a host's own median decides"
lane_suite_seconds() { case "$1 $2" in "e2e "*) echo 3300 ;; "resilience Node2") echo 5000 ;; "resilience "*) echo 900 ;; "rust-vtn "*) echo 600 ;; esac; }
check "a slow host gets the short suite instead" "Node3: e2e resilience|Node2: rust-vtn" \
    "$(lane_plan 'Node3 Node2' 'e2e resilience rust-vtn' | tr '\n' '|' | sed 's/|$//')"

# ── lane acquisition ─────────────────────────────────────────────────────────

CALLS="$TMP/calls"; : > "$CALLS"
BUSY=""        # hosts whose lock a session holds
DIRTY=""       # hosts whose checkout is dirty
lane_host_ok()   { echo "ok $1" >> "$CALLS"; return 0; }
lane_acquire()   { echo "acquire $1" >> "$CALLS"; case " $BUSY " in *" $1 "*) LANE_WHY="held by a session"; return 2 ;; esac; return 0; }
lane_publish()   { echo "publish" >> "$CALLS"; return 0; }
lane_unpublish() { echo "unpublish" >> "$CALLS"; }
lane_prepare()   { echo "prepare $1" >> "$CALLS"; case " $DIRTY " in *" $1 "*) LANE_WHY="checkout has uncommitted changes"; return 1 ;; esac; return 0; }
lane_lock()       { echo "lock $1 $2" >> "$CALLS"; }
TICKET_NAME=t; TICKET_DIR="$TMP/t"; mkdir -p "$TICKET_DIR"

echo "lanes_acquire"
: > "$CALLS"; BUSY=""; DIRTY=""
lanes_acquire 3
check "three suites: both extra hosts, in order" "Node3 Node2 Node1" "${LANE_HOSTS[*]}"
check "the extras were tried once each" "2" "$(grep -c '^acquire' "$CALLS")"
: > "$CALLS"; BUSY="Node2"
lanes_acquire 3
check "a busy host is skipped, the next one is used" "Node3 Node1" "${LANE_HOSTS[*]}"
check "the skip is recorded with its reason" "Node2|held by a session" "${LANE_SKIPPED[*]}"
check "the busy host was not prepared" "0" "$(grep -c '^prepare Node2' "$CALLS")"
: > "$CALLS"; BUSY="Node2 Node1"
lanes_acquire 3
check "both busy: only the home lane" "Node3" "${LANE_HOSTS[*]}"
check "both busy: nothing published" "0" "$(grep -c '^publish' "$CALLS")"
: > "$CALLS"; BUSY=""
lanes_acquire 1
check "one suite: no extra host is even tried" "0" "$(grep -c '^acquire' "$CALLS")"
check "one suite: only the home lane" "Node3" "${LANE_HOSTS[*]}"
: > "$CALLS"; BUSY=""; DIRTY="Node2"
lanes_acquire 2
check "a dirty checkout is skipped and its lock released" "Node3 Node1|1" "${LANE_HOSTS[*]}|$(grep -c '^lock release Node2' "$CALLS")"
check "the dirty skip says why" "Node2|checkout has uncommitted changes" "${LANE_SKIPPED[*]}"
DIRTY=""
CI_LANE_HOSTS_SAVE="$CI_LANE_HOSTS"; LANE_HOSTS_CONF=""
: > "$CALLS"; lanes_acquire 3
check "no lane hosts configured: nothing else touched" "Node3|0" "${LANE_HOSTS[*]}|$(grep -c '^acquire' "$CALLS")"
LANE_HOSTS_CONF="$CI_LANE_HOSTS_SAVE"

# ── lane jobs ────────────────────────────────────────────────────────────────

# lane_run_suite <host> <suite> <log>: behaves by $BEHAVE entries "host:suite=rc[,sleep]"
BEHAVE=""
lane_run_suite() {
    local h="$1" s="$2" log="$3" spec rc=0 nap=0
    for spec in $BEHAVE; do
        if [ "${spec%%=*}" = "$h:$s" ]; then v="${spec#*=}"; rc="${v%%,*}"; [ "$v" = "$rc" ] || nap="${v#*,}"; fi
    done
    echo "start $h $s" >> "$CALLS"
    [ "$nap" -eq 0 ] || sleep "$nap"
    echo "=== $s on $h" >> "$log"
    [ "$rc" -ne 0 ] || printf '  [0;32mPASS[0m  %s
' "$s" >> "$log"
    echo "end $h $s $rc" >> "$CALLS"
    return "$rc"
}
tidy_containers() { echo "tidy_containers $1" >> "$CALLS"; }
tidy_tree()       { echo "tidy_tree $1" >> "$CALLS"; }
lane_suite_seconds() { case "$1" in e2e) echo 3300 ;; resilience) echo 900 ;; rust-vtn) echo 600 ;; esac; }

run_full() { # <suites> -> sets RC; fresh ticket dir and STEPS
    TICKET_DIR="$TMP/t$RANDOM"; mkdir -p "$TICKET_DIR"; TICKET_NAME=t; STEPS=(); FAILED_STEP=""
    : > "$CALLS"; SECONDS=0
    full_tier "$1"; RC=$?
}

echo "full_tier, two lanes, all green"
BUSY=""; DIRTY=""; BEHAVE=""
run_full "e2e resilience"
check "returns success" 0 "$RC"
check "e2e on Node3 and resilience on Node2, as named rows" \
    "run_all_tests.sh --e2e [Node3]|0|run_all_tests.sh --resilience [Node2]|0" \
    "$(printf '%s|' "${STEPS[@]}" | awk -F'|' '{print $1"|"$2"|"$4"|"$5}')"
check "the extra lane was tidied (tree too) before its lock was released" "1" \
    "$(awk '/^tidy_tree Node2/{t=NR} /^lock release Node2/{r=NR} END{print (t&&r&&t<r)?1:0}' "$CALLS")"
check "the home lane got no tree tidy from its job (the ticket does that last)" "0" "$(grep -c '^tidy_tree Node3' "$CALLS")"
check "each lane has its own log" "1|1" "$([ -s "$TICKET_DIR/full.log" ] && echo 1)|$([ -s "$TICKET_DIR/full.Node2.log" ] && echo 1)"
check "the ref is published once and removed" "1|1" "$(grep -c '^publish' "$CALLS")|$(grep -c '^unpublish' "$CALLS")"
check "the lanes file shows both lanes ok" "Node3|e2e|ok Node2|resilience|ok" \
    "$(awk -F'|' '$1!="skipped"{printf "%s|%s|%s ", $1,$2,$3}' "$TICKET_DIR/lanes" | sed 's/ $//')"

echo "full_tier, a failing suite cancels the other lane"
BEHAVE="Node3:e2e=0,30 Node2:resilience=1"
run_full "e2e resilience"
check "returns red" 1 "$RC"
check "it did not wait for the long suite" "1" "$([ "$SECONDS" -lt 20 ] && echo 1 || echo 0)"
check "the failed step is named" "run_all_tests.sh --resilience [Node2]" "$FAILED_STEP"
check "the cancelled suite never reports an end" "0" "$(grep -c '^end Node3 e2e' "$CALLS")"
check "the cancelled lane is marked cancelled" "cancelled" "$(awk -F'|' '$1=="Node3"{print $3}' "$TICKET_DIR/lanes")"
check "the failed lane is marked red" "red" "$(awk -F'|' '$1=="Node2"{print $3}' "$TICKET_DIR/lanes")"
check "the cancelled home lane's containers were tidied" "1" "$(grep -c '^tidy_containers Node3' "$CALLS")"
check "the failed extra lane is released" "1" "$(grep -c '^lock release Node2' "$CALLS")"

echo "full_tier, an infrastructure failure moves the suite home"
BEHAVE="Node2:resilience=255"
run_full "e2e resilience"
check "returns success when the rerun passes" 0 "$RC"
check "the suite ran again on the home lane" "1" "$(grep -c '^end Node3 resilience 0' "$CALLS")"
check "only the home rows are steps" "run_all_tests.sh --e2e [Node3]|run_all_tests.sh --resilience [Node3]" \
    "$(printf '%s\n' "${STEPS[@]}" | cut -d'|' -f1 | tr '\n' '|' | sed 's/|$//')"
check "the move is recorded" "1" "$(grep -c 'moved' "$TICKET_DIR/lanes")"
BEHAVE="Node2:resilience=255 Node3:resilience=1"
run_full "e2e resilience"
check "the rerun's own failure is red" 1 "$RC"

echo "full_tier, a skip counts as infrastructure, not as a pass"
lane_run_suite() { echo "=== $2 SKIP" >> "$3"; return 0; }
run_full "e2e resilience"
check "a suite that reports no PASS is not green" 1 "$RC"

echo "full_tier, one lane (nothing configured)"
lane_run_suite() { echo "  PASS  $2" >> "$3"; return 0; }
LANE_HOSTS_CONF=""
run_full "e2e resilience rust-vtn"
check "returns success" 0 "$RC"
check "three suites one after the other, longest first" \
    "run_all_tests.sh --e2e [Node3]|run_all_tests.sh --resilience [Node3]|run_all_tests.sh --rust [Node3]" \
    "$(printf '%s\n' "${STEPS[@]}" | cut -d'|' -f1 | tr '\n' '|' | sed 's/|$//')"
LANE_HOSTS_CONF="$CI_LANE_HOSTS_SAVE"

echo "result.md"
TICKET_DIR="$TMP/res"; mkdir -p "$TICKET_DIR"
BRANCH=fix/x SHA=aaaa KIND=merge TIERS=quick,full VERDICT=red NOTE="" TESTED_SHA=bbbb FAILED_STEP="run_all_tests.sh --resilience [Node2]"
STEPS=("run_all_tests.sh --e2e [Node3]|0|3300" "run_all_tests.sh --resilience [Node2]|1|400")
printf 'Node3|e2e|cancelled|120|stopped: resilience failed on Node2\nNode2|resilience|red|400|\nskipped|Node1|load 3.1, 900 MB free\n' > "$TICKET_DIR/lanes"
printf 'FLAKY_SCENARIO: features/x.feature:3\n' > "$TICKET_DIR/full.Node2.log"
write_result res
check "a Lanes section" 1 "$(grep -c '^## Lanes' "$TICKET_DIR/result.md")"
check "a lane row" 1 "$(grep -c '^| Node2 | resilience | red | 400 |' "$TICKET_DIR/result.md")"
check "a cancelled lane row" 1 "$(grep -c '^| Node3 | e2e | cancelled |' "$TICKET_DIR/result.md")"
check "the skipped host and why" 1 "$(grep -c 'Node1.*load 3.1' "$TICKET_DIR/result.md")"
check "flaky scenarios are read from every lane log" 1 "$(grep -c 'FLAKY_SCENARIO: features/x.feature:3' "$TICKET_DIR/result.md")"

[ "$FAILS" -eq 0 ] && echo "all passed" || { echo "$FAILS failed"; exit 1; }
