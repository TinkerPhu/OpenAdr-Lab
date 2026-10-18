#!/usr/bin/env bash
# ci-lib.sh — what the session-side scripts (ci-handoff, ci-next, ci-wait) share: the CI host and
# the one way to run board.sh on it. Sourced, never run.
#
# The board script lives in the repository and is sent over stdin, so the CI host needs no copy of
# the repository. BOARD_FINISHED (the final ticket statuses) comes from the same file.

CI_HOST="${CI_HOST:?set CI_HOST to the ssh name of the CI host (e.g. export CI_HOST=Node3); see ci/README.md}"
CI_LIB_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=board.sh
BOARD_NO_MAIN=1 source "$CI_LIB_DIR/board.sh"

# ci_board <op> [args...] — run board.sh on the CI host (ssh joins its arguments with spaces, so
# they travel as one %q-escaped command line)
ci_board() {
    # shellcheck disable=SC2029
    ssh "$CI_HOST" "bash -s -- $(printf '%q ' "$@")" < "$CI_LIB_DIR/board.sh"
}
