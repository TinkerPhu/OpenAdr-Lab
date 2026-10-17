#!/usr/bin/env bash
# Does the runner (ci/runner.sh) turn a change into the right suites, flags and result.md block?
#
# Sources the runner as a library (RUNNER_NO_MAIN=1) and drives select_suites, suite_flags and
# write_result against a throwaway git repository standing in for the CI checkout.
#
# Usage:  bash ci/test_runner.sh
set -uo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
export CI_ROOT="$TMP/ci" CI_REPO="$TMP/repo" BOARD_DIR="$TMP/board" RUNNER_NO_MAIN=1
mkdir -p "$BOARD_DIR"
# shellcheck source=runner.sh
source "$SCRIPT_DIR/runner.sh"
set +e

FAILS=0
check() { # $1=name  $2=expected  $3=actual
    if [ "$2" = "$3" ]; then echo "  ok    $1"
    else echo "  FAIL  $1 (got '$3', want '$2')"; FAILS=$((FAILS + 1)); fi
}

git init -q "$CI_REPO" && cd "$CI_REPO" || exit 1
git config user.email t@t.invalid && git config user.name t
mkdir -p VEN/src VEN/ui docs
echo a > VEN/src/a.rs && echo a > VEN/ui/a.tsx && echo a > docs/a.md
git add -A && git commit -qm base && git update-ref refs/remotes/origin/main HEAD

# change_on <path> - a commit on top of origin/main touching one path; sets TESTED_SHA
change_on() {
    git checkout -q --detach origin/main
    mkdir -p "$(dirname "$1")" && echo "$RANDOM" >> "$1"
    git add -A && git commit -qm change
    TESTED_SHA="$(git rev-parse HEAD)"
}

echo "select_suites"
TIERS=quick,full
change_on VEN/ui/a.tsx; select_suites
check "UI only runs e2e"                  "e2e" "$SUITES_RUN"
check "UI only skips the rest"            "cargo resilience rust-vtn" "$SUITES_SKIPPED"
check "UI only leaves nothing unrun"      "" "$SUITES_UNRUN"
change_on docs/a.md; select_suites
check "docs run no suite"                 "" "$SUITES_RUN"
check "docs skip every suite"             "cargo e2e resilience rust-vtn" "$SUITES_SKIPPED"
change_on VEN/src/a.rs; select_suites
check "VEN source skips the openleadr-rs tests" "rust-vtn" "$SUITES_SKIPPED"
TIERS=quick
change_on VEN/src/a.rs; select_suites
check "tiers quick: e2e and resilience stay unrun" "e2e resilience" "$SUITES_UNRUN"
check "the deciding path is listed"       "VEN/src/a.rs -> cargo e2e resilience" "$CHANGED_WHY"

echo "suite_flags"
check "rust-vtn is --rust"                "--rust" "$(suite_flags 'rust-vtn')"
check "e2e and resilience keep their name" "--e2e --resilience" "$(suite_flags 'cargo e2e resilience' | tr '\n' ' ' | sed 's/ $//')"
check "cargo alone has no full-tier flag" "" "$(suite_flags 'cargo')"

echo "result.md"
TICKET_DIR="$BOARD_DIR/t"; mkdir -p "$TICKET_DIR"
BRANCH=fix/x SHA=aaaa KIND=merge TIERS=quick,full VERDICT=merged NOTE="" TESTED_SHA=bbbb STEPS=("cargo test|0|5")
TIERS=quick,full; change_on VEN/ui/a.tsx; select_suites
write_result t
check "result.md lists the suites run"    "1" "$(grep -c '^- run: e2e$' "$TICKET_DIR/result.md")"
check "result.md lists the suites skipped" "1" "$(grep -c '^- skipped: cargo resilience rust-vtn$' "$TICKET_DIR/result.md")"
check "result.md names the deciding path" "1" "$(grep -c 'VEN/ui/a.tsx -> e2e' "$TICKET_DIR/result.md")"

[ "$FAILS" -eq 0 ] && echo "all passed" || { echo "$FAILS failed"; exit 1; }
