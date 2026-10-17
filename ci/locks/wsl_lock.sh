#!/usr/bin/env bash
#
# wsl_lock.sh — cooperative lease lock for the shared WSL instance on this
# laptop.
#
# This machine has only 8 GB RAM (see CLAUDE.md memory-budget rule): running
# more than one `wsl cargo build/check/test/clippy` at a time can exhaust the
# pagefile and crash WSL. Multiple worktrees / AI sessions on this same
# Windows host share the one WSL instance, so anything that runs a
# large-memory WSL command must hold this lock first. The lock lives on a
# Windows-mounted path reachable from WSL (not inside any worktree, and not
# inside the WSL VM's own filesystem — see the R-67 note below for why), so
# it covers every session that reaches this WSL instance, the same way
# docker_host_lock.sh's lock lives on Node1 for every session that reaches
# that host.
#
# Usage:
#   bash ci/locks/wsl_lock.sh acquire -m "cargo check on 033-task-status"   # 20-min lease
#   bash ci/locks/wsl_lock.sh acquire -m "full cargo test" -l 60            # 60-min lease
#   bash ci/locks/wsl_lock.sh release
#   bash ci/locks/wsl_lock.sh refresh [-l MIN]   # extend the lease (from now)
#   bash ci/locks/wsl_lock.sh status
#
# Semantics: ci/locks/lib/lease_lock_core.sh, shared with docker_host_lock.sh —
# self-declared lease, re-entrant per owner, dead locks are stolen with a
# warning, acquire waits up to MAX_WAIT_SEC then exits 2 ("rerun to keep
# waiting"). Owner identity = user@host:<worktree path>#session-<id>, so
# release/refresh only act on a lock you own.
#
# R-67 (fixed 2026-08-29): the lock used to live at /tmp/openadr_wsl.lock,
# INSIDE the WSL VM. WSL2 shuts the VM down when idle and wipes /tmp on
# restart, which silently erased the lock — a later `acquire` then saw no
# lock at all (not an expired one) and succeeded immediately, so two
# sessions could hold it "at once" with neither aware of the other. The
# lock now lives on a Windows-mounted path (/mnt/c/...) so it survives a
# VM restart, the same way docker_host_lock.sh's lock survives because it
# lives on the persistent remote host rather than inside a throwaway VM.
# Override the path with WSL_LOCK_PATH if this default ever collides.
#
set -euo pipefail

LEASE_MIN="${WSL_LOCK_LEASE_MIN:-20}"
POLL_SEC="${WSL_LOCK_POLL_SEC:-10}"
MAX_WAIT_SEC="${WSL_LOCK_MAX_WAIT_SEC:-540}"
# Windows-mounted (survives a WSL VM restart) — see the R-67 note above.
LOCK_PATH="${WSL_LOCK_PATH:-/mnt/c/Users/Public/.openadr_wsl_lock}"
LOCK_SCRIPT_NAME="wsl_lock.sh"

usage() { sed -n '2,22p' "$0"; exit 1; }

source "$(dirname "${BASH_SOURCE[0]}")/lib/lease_lock_cli.sh"

# MSYS_NO_PATHCONV: this script is normally invoked from Git Bash, whose MSYS
# layer rewrites bare /mnt/c/... arguments as if they were paths to convert
# (e.g. to "C:/Program Files/Git/mnt/c/..."), silently corrupting LOCK_PATH
# before wsl.exe ever sees it. Found while fixing R-67, the same day, one
# command after the fix it was verifying.
lock_op() { # $1=op  $2=description
    MSYS_NO_PATHCONV=1 wsl bash -s -- "$1" "$LOCK_OWNER_ID" "$LEASE_MIN" "${2:-}" "$LOCK_PATH" < "$LEASE_LOCK_CORE"
}

lease_lock_main "$@"
