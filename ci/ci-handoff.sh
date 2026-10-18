#!/usr/bin/env bash
# ci-handoff.sh — hand the current branch to the CI runner on Node3, then stop working on it.
#
# Writes a ticket onto the handoff board (ci/board.sh); the runner rebases the commit
# onto origin/main, tests it and, if everything is green, fast-forwards main. The session can
# close: `ci/ci-next.sh` lists finished tickets and resumes the session that wrote one.
#
#   bash ci/ci-handoff.sh                    # test everything, merge if green
#   bash ci/ci-handoff.sh --kind check       # test only, never merge
#   bash ci/ci-handoff.sh --tiers quick      # audits/fmt/clippy/cargo test only (never merges code)
#   bash ci/ci-handoff.sh --deploy Node1 VEN "ven-2 ven-3"     # ONLY after the user said so:
#       the runner builds release images of origin/main, ships them to Node1 and switches the
#       services over (--rollback in addition: back to the previous images). No branch is involved.
#
# Before handing off: journal and key-learnings entries committed (they are tested and merged
# with the code), the branch pushed (`git push -f origin HEAD:<branch>`), the tree clean.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=ci-lib.sh
source "$HERE/ci-lib.sh"
TIERS=quick,full
KIND=merge
RESUME_DIR=""
DEPLOY=()            # target stack services
ROLLBACK=no

die() { echo "ci-handoff: $*" >&2; exit 1; }

while [ $# -gt 0 ]; do
    case "$1" in
        --tiers)      TIERS="${2:?--tiers needs quick or quick,full}"; shift 2 ;;
        --kind)       KIND="${2:?--kind needs merge or check}"; shift 2 ;;
        --deploy)     DEPLOY=("${2:?--deploy needs target, stack and services}" "${3:?--deploy needs a stack}" "${4:?--deploy needs the services, quoted}"); KIND=deploy; shift 4 ;;
        --rollback)   ROLLBACK=yes; shift ;;
        --resume-dir) RESUME_DIR="${2:?--resume-dir needs a directory}"; shift 2 ;;
        *) sed -n '2,17p' "$0"; exit 1 ;;
    esac
done
case "$KIND" in merge|check|deploy) ;; *) die "--kind is merge or check" ;; esac
[ "$ROLLBACK" = no ] || [ "$KIND" = deploy ] || die "--rollback goes with --deploy"
case "$TIERS" in quick|quick,full) ;; *) die "--tiers is quick or quick,full" ;; esac

if [ "$KIND" = deploy ]; then
    # A deploy ships what origin/main holds; the branch check below does not apply.
    BRANCH=main
    SHA="$(git ls-remote origin refs/heads/main | cut -f1)"
    [ -n "$SHA" ] || die "cannot read origin/main"
else
    BRANCH="$(git rev-parse --abbrev-ref HEAD)"
    case "$BRANCH" in main|HEAD) die "hand off a branch, not '$BRANCH'" ;; esac
    [ -z "$(git status --porcelain --untracked-files=no)" ] || die "uncommitted changes: commit them first"
    SHA="$(git rev-parse HEAD)"
    REMOTE_SHA="$(git ls-remote origin "refs/heads/$BRANCH" | cut -f1)"
    [ "$REMOTE_SHA" = "$SHA" ] || die "origin has ${REMOTE_SHA:-no such branch}, this is $SHA: git push -f origin HEAD:$BRANCH first"
fi
DEPLOY_FIELDS=()
[ "$KIND" != deploy ] || DEPLOY_FIELDS=("TARGET=${DEPLOY[0]}" "STACK=${DEPLOY[1]}" "SERVICES=${DEPLOY[2]}" "ROLLBACK=$ROLLBACK")

# The session lives under the directory Claude Code was STARTED in, which is where it must be
# resumed from. For a worktree session that is the primary checkout.
if [ -z "$RESUME_DIR" ]; then
    RESUME_DIR="$(git rev-parse --path-format=absolute --git-common-dir)"
    RESUME_DIR="${RESUME_DIR%/.git}"
fi
SESSION_ID="${CLAUDE_CODE_SESSION_ID:-manual-$(whoami)}"
WORKTREE="$(git rev-parse --show-toplevel)"

# ssh joins its arguments with spaces, so they travel as one %q-escaped command line; the
# board script itself goes over stdin, which needs no copy of the repository on the host.
# shellcheck disable=SC2029
TICKET="$(ssh "$CI_HOST" "bash -s -- $(printf '%q ' create \
    "SESSION_ID=$SESSION_ID" "BRANCH=$BRANCH" "SHA=$SHA" "TIERS=$TIERS" "KIND=$KIND" \
    "RESUME_DIR=$RESUME_DIR" "WORKTREE=$WORKTREE" "${DEPLOY_FIELDS[@]}")" < "$HERE/board.sh")"

echo "handed off: $TICKET"
echo "  $BRANCH @ ${SHA:0:8}, tiers $TIERS, kind $KIND"
echo "  The runner picks it up within a minute. To be woken with the verdict, run in the background:"
echo "    bash ci/ci-wait.sh $TICKET --ack"
echo "  Or close this session: 'bash ci/ci-next.sh' resumes it with the result (--all shows the board)."
