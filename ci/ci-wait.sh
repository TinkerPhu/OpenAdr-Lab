#!/usr/bin/env bash
# ci-wait.sh — block until a CI ticket is finished, then print its verdict and exit.
#
#   bash ci/ci-wait.sh <ticket>                  # the name ci-handoff.sh printed
#   bash ci/ci-wait.sh <ticket> --ack            # also take it off the ci-next list
#   bash ci/ci-wait.sh <ticket> --max-wait 7200  # give up after this many seconds (default 86400)
#
# Meant to run in the background of the session that handed the branch off: when it exits, the
# session is woken with the verdict and needs no ci-next. The polling happens in this shell, so
# waiting costs no model turns however long the queue and the run take.
#
# Exit code: 0 merged, deployed or green; 1 red, conflict, stale or error; 3 gave up waiting.
# A dropped ssh connection is retried, never taken for a verdict.
#
# Nothing is consumed: the ticket on the CI host keeps its status and result.md, and without
# --ack `ci-next.sh` still lists it, so the workflow continues by hand if this script, the session
# or the laptop is gone.
set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=ci-lib.sh
source "$HERE/ci-lib.sh"

NAME=""
ACK=no
MAX_WAIT=86400
INTERVAL="${CI_WAIT_INTERVAL:-30}"
RESULT_LINES=80

while [ $# -gt 0 ]; do
    case "$1" in
        --ack)      ACK=yes; shift ;;
        --max-wait) MAX_WAIT="${2:?--max-wait needs seconds}"; shift 2 ;;
        -*)         sed -n '2,10p' "$0"; exit 1 ;;
        *)          NAME="$1"; shift ;;
    esac
done
[ -n "$NAME" ] || { echo "ci-wait: give the ticket name (ci-handoff.sh printed it)" >&2; exit 1; }

BY_HAND="Continue by hand: bash ci/ci-next.sh (finished tickets), bash ci/ci-next.sh --all (the board)."

while :; do
    if SHOWN="$(ci_board show "$NAME" 2>/dev/null)"; then
        STATUS="$(printf '%s\n' "$SHOWN" | sed -n 's/^status: //p' | head -1)"
        case " $BOARD_FINISHED " in
            *" $STATUS "*)
                echo "CI ticket $NAME: $STATUS"
                printf '%s\n' "$SHOWN" | sed -n '/^--- result.md/,$p' | tail -n +2 | head -n "$RESULT_LINES"
                echo
                echo "The full result and logs stay in the ticket on $CI_HOST. $BY_HAND"
                [ "$ACK" = no ] || ci_board ack "$NAME" || true
                case "$STATUS" in merged|deployed|green) exit 0 ;; *) exit 1 ;; esac ;;
        esac
    fi
    if [ "$SECONDS" -ge "$MAX_WAIT" ]; then
        echo "CI ticket $NAME: not finished after ${MAX_WAIT}s (last status: ${STATUS:-unreachable})."
        echo "$BY_HAND"
        exit 3
    fi
    sleep "$INTERVAL"
done
