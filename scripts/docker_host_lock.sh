#!/usr/bin/env bash
#
# docker_host_lock.sh — cooperative lease lock for a shared docker host
# (e.g. Node1, Node2 or Node3).
#
# Multiple worktrees / AI sessions deploy and test on the same host. Anything
# that builds or runs docker there must hold this lock first. The lock lives
# ON the target host (not in any worktree), so it covers every session and
# machine that can reach it.
#
# Usage:
#   bash scripts/docker_host_lock.sh acquire -m "E2E run for fix/foo"        # 60-min lease
#   bash scripts/docker_host_lock.sh acquire -m "full suite" -l 180          # 180-min lease
#   bash scripts/docker_host_lock.sh release
#   bash scripts/docker_host_lock.sh refresh [-l MIN]   # extend the lease (from now)
#   bash scripts/docker_host_lock.sh status
#
# Semantics (scripts/lib/lease_lock_core.sh, shared with wsl_lock.sh):
#   - The acquirer declares its own lease end (now + LEASE_MIN, stored as a UTC
#     epoch in the lock). Once that point is past, the lock counts as dead
#     (crashed session) and is stolen by the next acquirer, with a warning.
#   - Re-entrant per owner: acquiring while already holding renews the lease.
#   - acquire waits up to MAX_WAIT_SEC, then exits 2 ("rerun to keep waiting") —
#     kept under the 10-minute tool timeout of AI sessions on purpose.
#   - Owner identity = user@host:<worktree path>#session-<id> (scripts/lib/
#     lease_lock_cli.sh), so release/refresh only act on a lock you own.
#   - Host: LOCK_HOST overrides for this script only; OPENADR_LAB_HOST is
#     the shared default (also used by run_all_tests.sh); falls back to "Node1".
#     The lock file has one fixed path on the host, so every SSH alias of a
#     machine reaches the same lock; it was once named after the alias, which
#     made two aliases of one machine two independent locks.
#
set -euo pipefail

LOCK_TARGET_HOST="${LOCK_HOST:-${OPENADR_LAB_HOST:-Node1}}"
LEASE_MIN="${LOCK_LEASE_MIN:-60}"
POLL_SEC="${LOCK_POLL_SEC:-20}"
MAX_WAIT_SEC="${LOCK_MAX_WAIT_SEC:-540}"
LOCK_PATH="${DOCKER_HOST_LOCK_PATH:-/tmp/openadr_docker_host.lock}"
LOCK_SCRIPT_NAME="docker_host_lock.sh"

usage() { sed -n '2,31p' "$0"; exit 1; }

source "$(dirname "${BASH_SOURCE[0]}")/lib/lease_lock_cli.sh"

# ssh joins its command arguments with spaces and the remote shell splits them
# again, so the arguments travel as one %q-escaped command line. Being one string
# that starts with "bash", it also escapes MSYS's rewriting of arguments that look
# like absolute POSIX paths (Git Bash on Windows).
lock_op() { # $1=op  $2=description
    ssh "$LOCK_TARGET_HOST" "bash -s -- $(printf '%q ' "$1" "$LOCK_OWNER_ID" "$LEASE_MIN" "${2:-}" "$LOCK_PATH")" < "$LEASE_LOCK_CORE"
}

lease_lock_main "$@"
