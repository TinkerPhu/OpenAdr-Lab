#!/usr/bin/env bash
# board.sh — the CI handoff board: one directory per ticket, plain files, on Node3.
#
# A session hands a pushed branch to the runner by creating a ticket; the runner works
# through queued tickets oldest first and writes its verdict into them; `ci-next` lists
# finished tickets and resumes the session that wrote one. See docs/guidelines/TESTING.md
# ("CI on Node3") for the whole flow.
#
#   <BOARD_DIR>/<utc yyyymmddTHHMMZ>_<session 8>_<branch slug>/
#       request.env   KEY=value lines written by the session (never sourced, only read)
#       status        queued | running | merged | deployed | green | red | conflict | stale | error
#                     (green = tested but not merged: a check-only ticket)
#       result.md     the runner's summary
#       run.log       the runner's full log
#       attempt       how many times the runner started this ticket
#       ack           present once ci-next has handed the result to a session
#
# Two ways to use this file:
#   source it (BOARD_NO_MAIN=1) for the functions — the runner and the tests do;
#   or send it to the board's machine on stdin, as a session does without needing any
#   copy of the repository on Node3:
#       ssh Node3 'bash -s -- create SESSION_ID=... BRANCH=... SHA=...' < scripts/ci/board.sh
set -uo pipefail
export LC_ALL=C

BOARD_DIR="${BOARD_DIR:-/srv/ci/board}"
BOARD_FINISHED="merged deployed green red conflict stale error"

# ── Naming ───────────────────────────────────────────────────────────────────

board_slug() { printf '%s' "$1" | tr '/ ' '--' | tr -cd 'A-Za-z0-9._-' | cut -c1-48; }

# board_ticket_name <utc> <session id> <branch>
board_ticket_name() { printf '%s_%s_%s' "$1" "${2:0:8}" "$(board_slug "$3")"; }

# ── Reading ──────────────────────────────────────────────────────────────────

# board_tickets — every ticket name, oldest first (names start with the UTC time)
board_tickets() {
    local d
    for d in "$BOARD_DIR"/*/; do
        [ -d "$d" ] && basename "$d"
    done
}

board_status() { cat "$BOARD_DIR/$1/status" 2>/dev/null || echo unknown; }

# board_get <ticket> <KEY> — a request field; the file is read, never sourced.
board_get() { sed -n "s/^$2=//p" "$BOARD_DIR/$1/request.env" 2>/dev/null | head -1; }

board_next_queued() {
    local t
    for t in $(board_tickets); do
        [ "$(board_status "$t")" = queued ] && { echo "$t"; return 0; }
    done
    return 0
}

board_running() {
    local t
    for t in $(board_tickets); do
        [ "$(board_status "$t")" = running ] && echo "$t"
    done
}

# board_finished_unacked — finished tickets nobody has been handed yet, oldest first
board_finished_unacked() {
    local t s
    for t in $(board_tickets); do
        [ -e "$BOARD_DIR/$t/ack" ] && continue
        s="$(board_status "$t")"
        case " $BOARD_FINISHED " in *" $s "*) echo "$t" ;; esac
    done
}

# ── Writing ──────────────────────────────────────────────────────────────────

# board_write <file> <text> — atomic: readers see the old content or the new, never half.
board_write() {
    local tmp="$1.tmp.$$"
    printf '%s\n' "$2" > "$tmp" && mv "$tmp" "$1"
}

board_set_status() { board_write "$BOARD_DIR/$1/status" "$2"; }

board_ack() { : > "$BOARD_DIR/$1/ack"; }

# board_bump_attempt <ticket> — prints the new count
board_bump_attempt() {
    local f="$BOARD_DIR/$1/attempt" n
    n=$(( $(cat "$f" 2>/dev/null || echo 0) + 1 ))
    board_write "$f" "$n"
    echo "$n"
}

# board_create <ticket> KEY=value... — the ticket appears whole or not at all.
# Required: SESSION_ID, BRANCH, SHA (40 hex digits). Values may not contain newlines.
board_create() {
    local name="$1" kv key val have_session="" have_branch="" have_sha="" tmpdir
    shift
    for kv in "$@"; do
        key="${kv%%=*}"; val="${kv#*=}"
        case "$key" in ''|*[!A-Z_]*) echo "board: bad field name '$key'" >&2; return 1 ;; esac
        case "$val" in *$'\n'*) echo "board: $key has a newline" >&2; return 1 ;; esac
        case "$key" in
            SESSION_ID) have_session=1 ;;
            BRANCH) have_branch=1
                case "$val" in ''|*[!A-Za-z0-9._/-]*) echo "board: bad branch '$val'" >&2; return 1 ;; esac ;;
            SHA) have_sha=1
                [[ "$val" =~ ^[0-9a-f]{40}$ ]] || { echo "board: bad sha '$val'" >&2; return 1; } ;;
        esac
    done
    [ -n "$have_session$have_branch$have_sha" ] && [ -n "$have_session" ] && [ -n "$have_branch" ] && [ -n "$have_sha" ] \
        || { echo "board: SESSION_ID, BRANCH and SHA are required" >&2; return 1; }
    [ -e "$BOARD_DIR/$name" ] && { echo "board: $name exists" >&2; return 1; }

    tmpdir="$BOARD_DIR/.new.$name.$$"
    mkdir -p "$tmpdir" || return 1
    printf '%s\n' "$@" > "$tmpdir/request.env"
    printf 'queued\n' > "$tmpdir/status"
    mv "$tmpdir" "$BOARD_DIR/$name" || { rm -rf "$tmpdir"; return 1; }
    echo "$name"
}

# ── Classifying a change ─────────────────────────────────────────────────────

# board_docs_only — reads changed paths on stdin; succeeds when none is code. Docs,
# wiki and openspec directories and top-level markdown files are not code; a .md file
# inside a code directory still is (it may be embedded or read by a build).
board_docs_only() {
    local p
    while IFS= read -r p; do
        [ -z "$p" ] && continue
        case "$p" in
            docs/*|wiki/*|openspec/*) ;;
            */*) return 1 ;;
            *.md) ;;
            *) return 1 ;;
        esac
    done
    return 0
}

# ── Command line (only when run, not when sourced) ───────────────────────────

board_main() {
    local op="${1:-}" name t
    shift 2>/dev/null || true
    mkdir -p "$BOARD_DIR"
    case "$op" in
        create)
            local sid="" branch="" kv
            for kv in "$@"; do
                case "$kv" in SESSION_ID=*) sid="${kv#*=}" ;; BRANCH=*) branch="${kv#*=}" ;; esac
            done
            name="$(board_ticket_name "$(date -u +%Y%m%dT%H%MZ)" "$sid" "$branch")"
            board_create "$name" "$@" ;;
        list)  # finished tickets nobody has resumed: name, status, branch, sha
            for t in $(board_finished_unacked); do
                printf '%s\t%s\t%s\t%s\n' "$t" "$(board_status "$t")" "$(board_get "$t" BRANCH)" "$(board_get "$t" SHA)"
            done ;;
        all)  # every ticket with its status
            for t in $(board_tickets); do printf '%s\t%s\n' "$t" "$(board_status "$t")"; done ;;
        show)
            t="${1:?ticket name}"
            echo "status: $(board_status "$t")"
            cat "$BOARD_DIR/$t/request.env" 2>/dev/null
            echo "--- result.md"
            cat "$BOARD_DIR/$t/result.md" 2>/dev/null ;;
        ack) board_ack "${1:?ticket name}" ;;
        *) echo "usage: board.sh create KEY=value... | list | all | show <ticket> | ack <ticket>" >&2; return 1 ;;
    esac
}

# Run as a script, or from stdin with arguments; sourced with BOARD_NO_MAIN=1 it only defines functions.
if [ -z "${BOARD_NO_MAIN:-}" ] && { [ "${BASH_SOURCE[0]:-}" = "$0" ] || [ -z "${BASH_SOURCE[0]:-}" ]; }; then
    board_main "$@"
fi
