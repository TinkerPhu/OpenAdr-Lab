#!/usr/bin/env bash
# Does the CI handoff board (scripts/ci/board.sh) queue, order and report tickets correctly?
#
# The board is plain files on Node3: one directory per handoff, written by sessions and by
# the runner. These cases run the real library against a throwaway directory.
#
# Usage:  bash scripts/ci/test_board.sh
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

echo "docs-only classification"
check "markdown and docs are docs-only"     0 "$(printf 'docs/a.md\nwiki/b.md\nREADME.md\nopenspec/changes/x/design.md\n' | board_docs_only; echo $?)"
check "a source file is not docs-only"      1 "$(printf 'docs/a.md\nVEN/src/main.rs\n' | board_docs_only; echo $?)"
check "a script is not docs-only"           1 "$(printf 'scripts/ci/runner.sh\n' | board_docs_only; echo $?)"
check "a feature file is not docs-only"     1 "$(printf 'tests/features/a.feature\n' | board_docs_only; echo $?)"
check ".md inside code is still code"       1 "$(printf 'VEN/profiles/a.yaml\nVEN/notes.md\n' | board_docs_only; echo $?)"
check "no changed file is docs-only"        0 "$(printf '' | board_docs_only; echo $?)"

[ "$FAILS" -eq 0 ] && echo "all passed" || { echo "$FAILS failed"; exit 1; }
