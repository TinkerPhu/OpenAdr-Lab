#!/usr/bin/env bash
# Does a hand-off reach the board and does ci-next resume the right session?
#
# Runs ci-handoff.sh and ci-next.sh for real against a throwaway git origin and board, with
# `ssh` replaced by a stub that runs the command locally and `claude` by one that records its
# arguments. Pins: a branch that is not pushed, dirty, or main is refused; the ticket carries
# the session and the directory it must be resumed from; a finished ticket is handed over once.
#
# Usage:  bash scripts/ci/test_handoff.sh
set -uo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

mkdir -p "$TMP/bin"
# ssh <host> <command...>: drop the host, run the command locally (the remote shell re-splits it).
printf '#!/usr/bin/env bash\nshift\neval "$*"\n' > "$TMP/bin/ssh"
printf '#!/usr/bin/env bash\nprintf "%%s\\n" "$@" > "%s/claude.args"\npwd > "%s/claude.pwd"\n' "$TMP" "$TMP" > "$TMP/bin/claude"
chmod +x "$TMP/bin/ssh" "$TMP/bin/claude"
export PATH="$TMP/bin:$PATH"
export BOARD_DIR="$TMP/board" CLAUDE_CODE_SESSION_ID=2e1cc119-0311-412f-8f25-d0f49ff0deb2
mkdir -p "$BOARD_DIR"

git init -q --bare -b main "$TMP/origin.git"
git clone -q "$TMP/origin.git" "$TMP/work" 2>/dev/null
cd "$TMP/work" || exit 1
git config user.email t@t; git config user.name t; git config core.autocrlf false
echo a > a.txt; git add a.txt; git commit -qm base; git push -q origin main
git checkout -qb fix/example; echo b > b.txt; git add b.txt; git commit -qm change

FAILS=0
check() { # $1=name  $2=expected  $3=actual
    if [ "$2" = "$3" ]; then echo "  ok    $1"
    else echo "  FAIL  $1 (got '$3', want '$2')"; FAILS=$((FAILS + 1)); fi
}
rc() { "$@" >/dev/null 2>&1; echo $?; }

echo "ci-handoff.sh"
check "an unpushed branch is refused"       1 "$(rc bash "$HERE/ci-handoff.sh")"
git push -q origin fix/example
echo dirty >> b.txt
check "uncommitted changes are refused"     1 "$(rc bash "$HERE/ci-handoff.sh")"
git checkout -q -- b.txt
git checkout -q main
check "main is refused"                     1 "$(rc bash "$HERE/ci-handoff.sh")"
git checkout -q fix/example
check "a bad kind is refused"               1 "$(rc bash "$HERE/ci-handoff.sh" --kind deploy)"
OUT="$(bash "$HERE/ci-handoff.sh" --resume-dir /resume/here 2>&1)"; echo "$OUT" | head -1
TICKET="$(ls "$BOARD_DIR")"
check "one ticket was created"              1 "$(ls "$BOARD_DIR" | wc -l | tr -d ' ')"
check "it is queued"                        queued "$(cat "$BOARD_DIR/$TICKET/status")"
check "it names the session"                "SESSION_ID=$CLAUDE_CODE_SESSION_ID" "$(grep ^SESSION_ID= "$BOARD_DIR/$TICKET/request.env")"
check "it names the commit"                 "SHA=$(git rev-parse HEAD)" "$(grep ^SHA= "$BOARD_DIR/$TICKET/request.env")"
check "it names the branch"                 "BRANCH=fix/example" "$(grep ^BRANCH= "$BOARD_DIR/$TICKET/request.env")"
check "it names the resume directory"       "RESUME_DIR=/resume/here" "$(grep ^RESUME_DIR= "$BOARD_DIR/$TICKET/request.env")"
check "the ticket name starts with the session" 1 "$(echo "$TICKET" | grep -c '_2e1cc119_fix-example$')"

echo "ci-next.sh"
check "nothing finished yet"                "no finished CI results waiting" "$(bash "$HERE/ci-next.sh")"
printf 'merged\n' > "$BOARD_DIR/$TICKET/status"
printf '# CI result: x\n\n- verdict: **merged**\n' > "$BOARD_DIR/$TICKET/result.md"
check "the finished ticket is listed"       1 "$(bash "$HERE/ci-next.sh" --list | grep -c "merged.*fix/example")"
mkdir -p "$TMP/resume"; sed -i "s|^RESUME_DIR=.*|RESUME_DIR=$TMP/resume|" "$BOARD_DIR/$TICKET/request.env"
DRY="$(bash "$HERE/ci-next.sh" --dry-run 1)"
check "dry run resumes the right session"   1 "$(echo "$DRY" | grep -c -- "--resume $CLAUDE_CODE_SESSION_ID")"
check "dry run resumes from the start dir"  1 "$(echo "$DRY" | grep -c "would run in $TMP/resume")"
check "dry run acknowledges nothing"        0 "$(ls "$BOARD_DIR/$TICKET/ack" 2>/dev/null | wc -l | tr -d ' ')"
bash "$HERE/ci-next.sh" 1 >/dev/null 2>&1
check "claude was started from there"       "$TMP/resume" "$(cat "$TMP/claude.pwd" 2>/dev/null)"
check "claude got --resume"                 1 "$(grep -c '^--resume$' "$TMP/claude.args")"
check "claude got the session id"           1 "$(grep -c "^$CLAUDE_CODE_SESSION_ID$" "$TMP/claude.args")"
check "the prompt carries the verdict"      1 "$(grep -c 'verdict: \*\*merged\*\*' "$TMP/claude.args")"
check "the ticket is acknowledged"          1 "$(ls "$BOARD_DIR/$TICKET/ack" 2>/dev/null | wc -l | tr -d ' ')"
check "and is not listed again"             "no finished CI results waiting" "$(bash "$HERE/ci-next.sh")"

[ "$FAILS" -eq 0 ] && echo "all passed" || { echo "$FAILS failed"; exit 1; }
