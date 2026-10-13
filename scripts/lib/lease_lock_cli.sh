# lease_lock_cli.sh — the caller-side half of the lease lock, sourced by
# docker_host_lock.sh and wsl_lock.sh.
#
# Owns who the caller is (owner identity), argument parsing and the acquire wait
# loop. It knows nothing about where the lock lives: the sourcing script defines
#   lock_op <op> <description>   run scripts/lib/lease_lock_core.sh on the lock's
#                                machine with: op "$LOCK_OWNER_ID" "$LEASE_MIN" desc <path>
# and sets LEASE_MIN, POLL_SEC, MAX_WAIT_SEC and LOCK_SCRIPT_NAME before calling
# lease_lock_main "$@".

LEASE_LOCK_CORE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lease_lock_core.sh"

# One owner per AI session, not per checkout: several sessions share the primary
# checkout, and with the path alone the second session's acquire counted as
# re-entrant and its release deleted the first session's lock. A Claude Code
# session exports CLAUDE_CODE_SESSION_ID to every command it runs, run_all_tests.sh
# included, so a session that holds the lock still re-enters it from the runner.
# LOCK_OWNER overrides (e.g. a human terminal holding a lock across shells).
lease_lock_owner() {
    local repo_root
    repo_root="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
    echo "${LOCK_OWNER:-$(whoami)@$(hostname):${repo_root}${CLAUDE_CODE_SESSION_ID:+#session-${CLAUDE_CODE_SESSION_ID:0:8}}}"
}
LOCK_OWNER_ID="$(lease_lock_owner)"

lease_lock_main() {
    local cmd="${1:-}" desc="" waited=0 out
    shift 2>/dev/null || true
    while [ $# -gt 0 ]; do
        case "$1" in
            -m) desc="${2:?-m needs a description}"; shift 2 ;;
            -l) LEASE_MIN="${2:?-l needs minutes}"
                case "$LEASE_MIN" in *[!0-9]*|'') echo "-l needs a whole number of minutes"; exit 1 ;; esac
                shift 2 ;;
            *)  echo "Unknown argument: $1"; usage ;;
        esac
    done

    case "$cmd" in
        acquire)
            [ -n "$desc" ] || desc="(no description) started $(date -u +%H:%MZ)"
            while true; do
                if out=$(lock_op try_acquire "$desc"); then echo "$out"; exit 0; fi
                echo "$out"
                if [ "$waited" -ge "$MAX_WAIT_SEC" ]; then
                    echo "Still held after ${waited}s — rerun '$LOCK_SCRIPT_NAME acquire' to keep waiting."
                    exit 2
                fi
                echo "  waiting ${POLL_SEC}s... (${waited}s so far; the lock is stealable once its lease end passes)"
                sleep "$POLL_SEC"; waited=$((waited + POLL_SEC))
            done ;;
        release|refresh|status)
            lock_op "$cmd" "" ;;
        *) usage ;;
    esac
}
