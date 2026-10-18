#!/usr/bin/env bash
# ci-next.sh — pick up the next finished CI result and resume the session that asked for it.
#
#   bash ci/ci-next.sh             # list finished tickets; resume the session of the chosen one
#   bash ci/ci-next.sh 2           # the second one in the list
#   bash ci/ci-next.sh --list      # only list
#   bash ci/ci-next.sh --all       # the whole board, with every status
#   bash ci/ci-next.sh --new       # start a FRESH session in the ticket's worktree instead
#                                          # (cheaper than waking a long session for a plain "merged")
#   bash ci/ci-next.sh --dry-run   # print the command, resume nothing, acknowledge nothing
#
# Resuming happens from the directory the session was started in (stored in the ticket); the
# resumed session gets the runner's result as its first message. A ticket is acknowledged when
# it is handed over, so it is listed once.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=ci-lib.sh
source "$HERE/ci-lib.sh"
MODE=resume
DRY=no
PICK=""

die() { echo "ci-next: $*" >&2; exit 1; }
board() { # board <op> [args...] — the board script runs on the CI host, sent over stdin
    # shellcheck disable=SC2029
    ssh "$CI_HOST" "bash -s -- $(printf '%q ' "$@")" < "$HERE/board.sh"
}

while [ $# -gt 0 ]; do
    case "$1" in
        --list)    MODE=list; shift ;;
        --all)     MODE=all; shift ;;
        --new)     MODE=new; shift ;;
        --dry-run) DRY=yes; shift ;;
        [0-9]*)    PICK="$1"; shift ;;
        *) sed -n '2,14p' "$0"; exit 1 ;;
    esac
done

if [ "$MODE" = all ]; then board all; exit 0; fi

mapfile -t ROWS < <(board list)
[ "${#ROWS[@]}" -gt 0 ] || { echo "no finished CI results waiting"; exit 0; }

i=0
for row in "${ROWS[@]}"; do
    i=$((i + 1))
    IFS=$'\t' read -r name status branch sha <<<"$row"
    printf '%2d. %-9s %-40s %s  %s\n' "$i" "$status" "$branch" "${sha:0:8}" "$name"
done
[ "$MODE" = list ] && exit 0

if [ -z "$PICK" ]; then
    if [ "${#ROWS[@]}" -eq 1 ]; then PICK=1
    else read -r -p "which one? " PICK; fi
fi
[[ "$PICK" =~ ^[0-9]+$ ]] && [ "$PICK" -ge 1 ] && [ "$PICK" -le "${#ROWS[@]}" ] || die "no such ticket: $PICK"
IFS=$'\t' read -r NAME STATUS BRANCH SHA <<<"${ROWS[$((PICK - 1))]}"

SHOWN="$(board show "$NAME")"
field() { printf '%s\n' "$SHOWN" | sed -n "s/^$1=//p" | head -1; }
SESSION_ID="$(field SESSION_ID)"; RESUME_DIR="$(field RESUME_DIR)"; WORKTREE="$(field WORKTREE)"
RESULT="$(printf '%s\n' "$SHOWN" | sed -n '/^--- result.md/,$p' | tail -n +2)"

PROMPT="CI result for your hand-off $NAME: $STATUS on branch $BRANCH.
$(case "$STATUS" in
    merged)  echo "origin/main now holds the tested commit. Finish the task: delete this branch (local and origin), remove your worktree, return any docker host checkout you used to main." ;;
    deployed) echo "The deploy ran and the services are up on the target host. Report it to the user; if something looks wrong, hand off a rollback (ci-handoff.sh --deploy ... --rollback)." ;;
    green)   echo "Tested but not merged (check-only hand-off). Report the result to the user." ;;
    *)       echo "It did not merge. Find the cause in the result below (full logs are on the CI host), fix it, push, and run ci/ci-handoff.sh again." ;;
esac)

$RESULT"

if [ "$MODE" = new ]; then
    DIR="$WORKTREE"; [ -d "$DIR" ] || DIR="$RESUME_DIR"
    CMD=(claude "$PROMPT")
else
    DIR="$RESUME_DIR"
    CMD=(claude --resume "$SESSION_ID" "$PROMPT")
fi

if [ "$DRY" = yes ]; then
    echo "would run in $DIR:"; printf ' %q' "${CMD[@]}"; echo
    exit 0
fi
command -v claude >/dev/null || die "the 'claude' command is not on PATH"
[ -d "$DIR" ] || die "directory $DIR does not exist (try --new)"
board ack "$NAME"
cd "$DIR"
exec "${CMD[@]}"
