#!/usr/bin/env bash
#
# wsl_lock.sh — cooperative lease lock for the shared WSL instance on this
# laptop.
#
# This machine has only 8 GB RAM (see CLAUDE.md memory-budget rule): running
# more than one `wsl cargo build/check/test/clippy` at a time can exhaust the
# pagefile and crash WSL. Multiple worktrees / AI sessions on this same
# Windows host share the one WSL instance, so anything that runs a
# large-memory WSL command must hold this lock first. The lock lives INSIDE
# WSL (not any worktree), so it covers every session that reaches this WSL
# instance, the same way pi4_lock.sh's lock lives on the Pi4 for every
# session that reaches that host.
#
# Usage:
#   bash scripts/wsl_lock.sh acquire -m "cargo check on 033-task-status"   # 20-min lease
#   bash scripts/wsl_lock.sh acquire -m "full cargo test" -l 60            # 60-min lease
#   bash scripts/wsl_lock.sh release
#   bash scripts/wsl_lock.sh refresh [-l MIN]   # extend the lease (from now)
#   bash scripts/wsl_lock.sh status
#
# Semantics: identical to pi4_lock.sh — self-declared lease, re-entrant per
# owner, dead locks are stolen with a warning, acquire waits up to
# MAX_WAIT_SEC then exits 2 ("rerun to keep waiting"). Owner identity =
# user@host:<worktree path>, so release/refresh only act on a lock you own.
#
set -euo pipefail

LEASE_MIN="${WSL_LOCK_LEASE_MIN:-20}"
POLL_SEC="${WSL_LOCK_POLL_SEC:-10}"
MAX_WAIT_SEC="${WSL_LOCK_MAX_WAIT_SEC:-540}"

repo_root="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
OWNER="$(whoami)@$(hostname):${repo_root}"

usage() { sed -n '2,22p' "$0"; exit 1; }

# All check-and-act logic runs inside WSL so it is atomic (mkdir is the
# mutex; the owner file is metadata) — same design as pi4_lock.sh's remote_op,
# just via `wsl` instead of `ssh`.
wsl_op() { # $1=op  $2=description
    wsl bash -s -- "$1" "$OWNER" "$LEASE_MIN" "${2:-}" <<'REMOTE'
op="$1"; owner="$2"; lease_min="$3"; desc="$4"
lock="/tmp/openadr_wsl.lock"
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
            if out=$(wsl_op try_acquire "$desc"); then echo "$out"; exit 0; fi
            echo "$out"
            if [ "$waited" -ge "$MAX_WAIT_SEC" ]; then
                echo "Still held after ${waited}s — rerun 'wsl_lock.sh acquire' to keep waiting."
                exit 2
            fi
            echo "  waiting ${POLL_SEC}s... (${waited}s so far; the lock is stealable once its lease end passes)"
            sleep "$POLL_SEC"; waited=$((waited + POLL_SEC))
        done ;;
    release|refresh|status)
        wsl_op "$cmd" "" ;;
    *) usage ;;
esac
