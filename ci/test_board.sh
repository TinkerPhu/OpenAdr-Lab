#!/usr/bin/env bash
# Does the CI handoff board (ci/board.sh) queue, order and report tickets correctly?
#
# The board is plain files on Node3: one directory per handoff, written by sessions and by
# the runner. These cases run the real library against a throwaway directory.
#
# Usage:  bash ci/test_board.sh
set -uo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
export BOARD_DIR="$TMP/board" BOARD_NO_MAIN=1
mkdir -p "$BOARD_DIR"
# shellcheck source=board.sh
source "$SCRIPT_DIR/board.sh"

FAILS=0
check() { # $1=name  $2=expected  $3=actual
    if [ "$2" = "$3" ]; then echo "  ok    $1"
    else echo "  FAIL  $1 (got '$3', want '$2')"; FAILS=$((FAILS + 1)); fi
}
SHA_A=0123456789abcdef0123456789abcdef01234567
SHA_B=fedcba9876543210fedcba9876543210fedcba98

echo "ticket names"
check "name = utc, session prefix, branch slug" \
    "20261016T0955Z_2e1cc119_fix-capacity-event-trigger" \
    "$(board_ticket_name 20261016T0955Z 2e1cc119-0311-412f-8f25-d0f49ff0deb2 fix/capacity-event-trigger)"

echo "creating tickets"
T1=$(board_ticket_name 20261016T0955Z aaaaaaaa-1 fix/one)
T2=$(board_ticket_name 20261016T1005Z bbbbbbbb-2 fix/two)
board_create "$T2" SESSION_ID=bbbbbbbb-2 RESUME_DIR=/r BRANCH=fix/two SHA=$SHA_B TIERS=quick,full KIND=test >/dev/null
board_create "$T1" SESSION_ID=aaaaaaaa-1 RESUME_DIR=/r BRANCH=fix/one SHA=$SHA_A TIERS=quick,full KIND=test >/dev/null
check "a new ticket is queued"               queued   "$(board_status "$T1")"
check "request fields can be read back"      fix/one "$(board_get "$T1" BRANCH)"
check "the sha is stored"                    $SHA_A   "$(board_get "$T1" SHA)"
check "no temp file is left behind"          0        "$(find "$BOARD_DIR" -name '*.tmp*' | wc -l | tr -d ' ')"
check "the oldest queued ticket comes first" "$T1"    "$(board_next_queued)"
check "a bad sha is refused"                 1        "$(board_create bad SESSION_ID=x BRANCH=b SHA='zz; rm -rf /' >/dev/null 2>&1; echo $?)"
check "a branch with a space is refused"     1        "$(board_create bad2 SESSION_ID=x BRANCH='a b' SHA=$SHA_A >/dev/null 2>&1; echo $?)"
check "a refused ticket leaves no directory" 0        "$(ls "$BOARD_DIR" | grep -c '^bad')"

echo "running and finishing"
board_set_status "$T1" running
check "running tickets are not next"        "$T2"    "$(board_next_queued)"
check "running is not finished"             ""       "$(board_finished_unacked)"
board_set_status "$T1" red
board_set_status "$T2" merged
check "finished tickets are listed oldest first" "$T1
$T2" "$(board_finished_unacked)"
board_ack "$T1"
check "an acknowledged ticket drops off"    "$T2"    "$(board_finished_unacked)"
check "nothing queued, nothing next"        ""       "$(board_next_queued)"

echo "attempts"
check "first bump is 1"                     1        "$(board_bump_attempt "$T1")"
check "second bump is 2"                    2        "$(board_bump_attempt "$T1")"

echo "suite selection"
suites() { printf '%b' "$1" | board_suites | tr '
' ' ' | sed 's/ $//'; }
ALL="cargo e2e resilience rust-vtn"
check "docs and markdown select nothing"          ""  "$(suites 'docs/a.md
wiki/b.md
README.md
openspec/changes/x/design.md
')"
check "CI scripts select nothing"                 ""  "$(suites 'ci/runner.sh
ci/test_deploy.sh
ci/web/ci_web.py
')"
check "agent config selects nothing"              ""  "$(suites '.claude/CLAUDE.md
.claude/skills/ci-handoff/SKILL.md
')"
check "VEN source: cargo, e2e, resilience"        "cargo e2e resilience" "$(suites 'VEN/src/main.rs
')"
check "UI only: e2e"                              "e2e" "$(suites 'VEN/ui/src/a.tsx
VTN/ui/src/b.tsx
lab-ui/src/c.ts
')"
check "a feature file: e2e only"                  "e2e" "$(suites 'tests/features/a.feature
tests/features/steps/x.py
')"
check "the resilience script: resilience only"    "resilience" "$(suites 'tests/failure_recovery_test.sh
')"
check "the openleadr test harness: rust-vtn only" "rust-vtn" "$(suites 'tests/docker-compose.openleadr-test.yml
tests/Dockerfile.openleadr-test
')"
check "the VEN unit harness: cargo only"          "cargo" "$(suites 'tests/Dockerfile.ven-unit-test
')"
check "the BFF: cargo, e2e, resilience"           "cargo e2e resilience" "$(suites 'VTN/bff/src/main.rs
')"
check "other VTN files: e2e, resilience"          "e2e resilience" "$(suites 'VTN/docker-compose.yml
')"
check "mixed paths select the union"              "cargo e2e resilience" "$(suites 'docs/a.md
VEN/ui/src/a.tsx
VEN/src/main.rs
')"
check "an audit script runs everything"           "$ALL" "$(suites 'ci/audits/audit_duplication.py
')"
check "the host lock runs everything"             "$ALL" "$(suites 'ci/locks/docker_host_lock.sh
')"
check "the test entrypoint runs everything"       "$ALL" "$(suites 'tests/entrypoint.sh
')"
check "a path no rule names runs everything"      "$ALL" "$(suites 'brand_new_dir/x
')"
check "openleadr-rs runs everything"              "$ALL" "$(suites 'openleadr-rs/src/lib.rs
')"
check ".md inside code is code"                   "cargo e2e resilience" "$(suites 'VEN/notes.md
')"
check "a top-level non-md file runs everything"   "$ALL" "$(suites 'jobs.json
')"
check "no changed file selects nothing"           ""  "$(suites '')"
check "the reason lists path and rule"            "VEN/ui/a.tsx -> e2e" "$(printf 'VEN/ui/a.tsx
' | board_suites_why)"

echo "suite map"
map_suites() { grep -v '^#' "$SCRIPT_DIR/suites.map" | awk 'NF{print $2}' | tr ',' '\n' | sort -u; }
flagless=""
for s in $(map_suites | grep -v -x -e '-' -e all -e cargo); do
    grep -q -e "--${s/rust-vtn/rust})" "$SCRIPT_DIR/../run_all_tests.sh" || flagless="$flagless $s"
done
check "every suite in the map is a run_all_tests.sh flag" "" "$flagless"
check "the last rule is the catch-all"            "* all" "$(grep -v '^#' "$SCRIPT_DIR/suites.map" | awk 'NF{print $1, $2}' | tail -1)"

[ "$FAILS" -eq 0 ] && echo "all passed" || { echo "$FAILS failed"; exit 1; }
