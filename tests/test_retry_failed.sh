#!/usr/bin/env bash
# Does a failed scenario get exactly one retry, and does only a passing retry clear the pass?
# Usage:  bash tests/test_retry_failed.sh
set -uo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
# shellcheck source=retry_failed.sh
. ./retry_failed.sh

CALLS="$TMP/calls"
python() { echo "$*" >> "$CALLS"; ! grep -q "$FAIL_ON" <<<"$*"; }   # fake behave: fails for $FAIL_ON

FAILS=0
check() { if [ "$2" = "$3" ]; then echo "  ok    $1"; else echo "  FAIL  $1 (got '$3', want '$2')"; FAILS=$((FAILS + 1)); fi; }
run() { : > "$CALLS"; OUT="$(retry_failed_scenarios main "$1" "$TMP/rerun" --tags=@x 2>&1)"; RC=$?; }

printf 'features/a.feature:10
features/b.feature:20
' > "$TMP/rerun"

FAIL_ON=NEVER; run 1
check "all retries pass: the pass is cleared"       0 "$RC"
check "each scenario is retried once, on its own"  2 "$(wc -l < "$CALLS" | tr -d ' ')"
check "each is named as flaky"                      2 "$(grep -c '^FLAKY_SCENARIO: ' <<<"$OUT")"
check "the caller's behave args are kept"           1 "$(grep -c -- '--tags=@x' <<<"$(head -1 "$CALLS")")"

FAIL_ON=b.feature; run 1
check "one still failing: the pass stays red"       1 "$RC"
check "the one that passed is still named flaky"    1 "$(grep -c '^FLAKY_SCENARIO: features/a' <<<"$OUT")"

FAIL_ON=NEVER; run 0
check "a passing pass is not retried"               1 "$RC"
check "and nothing is run"                          0 "$(wc -l < "$CALLS" | tr -d ' ')"

: > "$TMP/rerun"; run 1
check "no rerun file content (crash): stays red"    1 "$RC"

echo; [ "$FAILS" -eq 0 ] && echo "all passed" || { echo "$FAILS failed"; exit 1; }
