#!/usr/bin/env bash
#
# pi4_lock.sh — cooperative lease lock for the shared Pi4-Server docker host.
#
# Multiple worktrees / AI sessions deploy and test on the same Pi4. Anything
# that builds or runs docker there must hold this lock first. The lock lives
# ON the Pi4 (not in any worktree), so it covers every session and machine
# that can reach the host.
#
# Usage:
#   bash scripts/pi4_lock.sh acquire -m "E2E run for fix/foo"        # 60-min lease
#   bash scripts/pi4_lock.sh acquire -m "full suite" -l 180          # 180-min lease
#   bash scripts/pi4_lock.sh release
#   bash scripts/pi4_lock.sh refresh [-l MIN]   # extend the lease (from now)
#   bash scripts/pi4_lock.sh status
#
# Semantics:
#   - The acquirer declares its own lease end (now + LEASE_MIN, stored as a UTC
#     epoch in the lock). Once that point is past, the lock counts as dead
#     (crashed session) and is stolen by the next acquirer, with a warning.
#   - Re-entrant per owner: acquiring while already holding renews the lease.
#   - acquire waits up to MAX_WAIT_SEC, then exits 2 ("rerun to keep waiting") —
#     kept under the 10-minute tool timeout of AI sessions on purpose.
#   - Owner identity = user@host:<worktree path>, so release/refresh only act
#     on a lock you own.
#
set -euo pipefail

PI4_HOST="${PI4_LOCK_HOST:-Pi4-Server}"
LEASE_MIN="${PI4_LOCK_LEASE_MIN:-60}"
POLL_SEC="${PI4_LOCK_POLL_SEC:-20}"
MAX_WAIT_SEC="${PI4_LOCK_MAX_WAIT_SEC:-540}"

repo_root="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
OWNER="$(whoami)@$(hostname):${repo_root}"

usage() { sed -n '2,25p' "$0"; exit 1; }

# One SSH round-trip; all check-and-act logic runs server-side so it is atomic
# (mkdir is the mutex; the owner file is metadata). The lock path is defined
# inside the remote script, not passed as an argument — MSYS (Git Bash on
# Windows) rewrites arguments that look like absolute POSIX paths.
remote_op() { # $1=op  $2=description
    # printf %q: ssh joins remote-command args with spaces, so multi-word
    # descriptions must be shell-escaped to survive the remote word split.
    ssh "$PI4_HOST" bash -s -- $(printf '%q %q %q %q' "$1" "$OWNER" "$LEASE_MIN" "${2:-}") <<'REMOTE'
op="$1"; owner="$2"; lease_min="$3"; desc="$4"
lock="/tmp/openadr_pi4.lock"
now=$(date +%s)
expiry=$(( now + lease_min * 60 ))
write_owner() { printf '%s\n%s\n%s\n' "$owner" "$expiry" "$desc" > "$lock/owner"; }
read_owner()  { cur_owner=$(sed -n 1p "$lock/owner" 2>/dev/null)
                cur_expiry=$(sed -n 2p "$lock/owner" 2>/dev/null)
                cur_desc=$(sed -n 3p "$lock/owner" 2>/dev/null)
                left_min=$(( (${cur_expiry:-0} - now) / 60 ))
                expiry_utc=$(date -u -d "@${cur_expiry:-0}" +'%Y-%m-%d %H:%MZ' 2>/dev/null || echo '?'); }
held_msg() { echo "$1 by $cur_owner: ${cur_desc:-no description} (lease ends $expiry_utc, in ${left_min}min)"; }
case "$op" in
  try_acquire)
    if mkdir "$lock" 2>/dev/null; then write_owner; echo "ACQUIRED (lease ${lease_min}min)"; exit 0; fi
    read_owner
    if [ "$cur_owner" = "$owner" ]; then write_owner; echo "ACQUIRED (re-entrant, lease renewed ${lease_min}min)"; exit 0; fi
    if [ -z "$cur_expiry" ] || [ "$now" -ge "$cur_expiry" ]; then
        write_owner
        echo "ACQUIRED (stole dead lock from ${cur_owner:-unknown}, lease expired $(( -left_min ))min ago: ${cur_desc:-?})"
        exit 0
    fi
    held_msg "HELD"; exit 1 ;;
  release)
    [ -d "$lock" ] || { echo "NOT LOCKED"; exit 0; }
    read_owner
    if [ "$cur_owner" = "$owner" ]; then rm -rf "$lock"; echo "RELEASED"; exit 0; fi
    held_msg "NOT OWNER — held"; exit 1 ;;
  refresh)
    [ -d "$lock" ] || { echo "NOT LOCKED — nothing to refresh"; exit 1; }
    read_owner
    if [ "$cur_owner" = "$owner" ]; then desc="$cur_desc"; write_owner; echo "REFRESHED (lease ${lease_min}min from now)"; exit 0; fi
    held_msg "NOT OWNER — held"; exit 1 ;;
  status)
    [ -d "$lock" ] || { echo "FREE"; exit 0; }
    read_owner
    if [ "$now" -ge "${cur_expiry:-0}" ]; then
        echo "DEAD (stealable) — was $cur_owner: ${cur_desc:-no description} (lease expired $(( -left_min ))min ago)"
    else
        held_msg "HELD"
    fi
    exit 0 ;;
esac
REMOTE
}

cmd="${1:-}"; shift 2>/dev/null || true
desc=""
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
        waited=0
        while true; do
            if out=$(remote_op try_acquire "$desc"); then echo "$out"; exit 0; fi
            echo "$out"
            if [ "$waited" -ge "$MAX_WAIT_SEC" ]; then
                echo "Still held after ${waited}s — rerun 'pi4_lock.sh acquire' to keep waiting."
                exit 2
            fi
            echo "  waiting ${POLL_SEC}s... (${waited}s so far; the lock is stealable once its lease end passes)"
            sleep "$POLL_SEC"; waited=$((waited + POLL_SEC))
        done ;;
    release|refresh|status)
        remote_op "$cmd" "" ;;
    *) usage ;;
esac
