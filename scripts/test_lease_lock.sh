#!/usr/bin/env bash
# Do the lease locks (docker_host_lock.sh, wsl_lock.sh) actually exclude a second holder?
#
# Runs both scripts for real, with `ssh` and `wsl` replaced by stubs that execute the
# lock core locally, against a throwaway lock directory. Two failures it pins:
#   - two SSH aliases of one machine (Node2 / its home-lab name) must be ONE lock —
#     the lock was once named after the alias, so each alias was a lock of its own;
#   - two AI sessions in one checkout must be two owners — owner identity was once
#     the checkout path alone, so the second session's acquire was "re-entrant" and
#     its release deleted the first session's lock.
#
# Usage:  bash scripts/test_lease_lock.sh
set -uo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

mkdir -p "$TMP/bin"
# ssh <host> <remote command...>: drop the host, run the command locally. The remote
# side re-splits the joined command line, exactly like sshd does.
printf '#!/usr/bin/env bash\nshift\neval "$*"\n' > "$TMP/bin/ssh"
# wsl <command...>: run it locally.
printf '#!/usr/bin/env bash\nexec "$@"\n' > "$TMP/bin/wsl"
chmod +x "$TMP/bin/ssh" "$TMP/bin/wsl"
export PATH="$TMP/bin:$PATH"
export DOCKER_HOST_LOCK_PATH="$TMP/docker_host.lock" WSL_LOCK_PATH="$TMP/wsl.lock"
export LOCK_MAX_WAIT_SEC=0 WSL_LOCK_MAX_WAIT_SEC=0 LOCK_OWNER=

FAILS=0
check() { # $1=name  $2=expected exit  $3...=command
    local name="$1" want="$2"; shift 2
    out=$("$@" 2>&1); got=$?
    if [ "$got" = "$want" ]; then echo "  ok    $name"
    else echo "  FAIL  $name (exit $got, want $want): $out"; FAILS=$((FAILS + 1)); fi
}

echo "docker_host_lock.sh"
check "session A acquires via alias Node2"         0 env CLAUDE_CODE_SESSION_ID=aaaaaaaa-1 LOCK_HOST=Node2 bash "$SCRIPT_DIR/docker_host_lock.sh" acquire -m A
check "session B is refused via another alias"     2 env CLAUDE_CODE_SESSION_ID=bbbbbbbb-2 LOCK_HOST=OtherAlias bash "$SCRIPT_DIR/docker_host_lock.sh" acquire -m B
check "session B is refused via the same alias"    2 env CLAUDE_CODE_SESSION_ID=bbbbbbbb-2 LOCK_HOST=Node2 bash "$SCRIPT_DIR/docker_host_lock.sh" acquire -m B
check "session B cannot release A's lock"          1 env CLAUDE_CODE_SESSION_ID=bbbbbbbb-2 LOCK_HOST=Node2 bash "$SCRIPT_DIR/docker_host_lock.sh" release
check "session A re-acquires (re-entrant)"         0 env CLAUDE_CODE_SESSION_ID=aaaaaaaa-1 LOCK_HOST=OtherAlias bash "$SCRIPT_DIR/docker_host_lock.sh" acquire -m A2
check "status via the other alias shows A"         0 bash -c "LOCK_HOST=OtherAlias bash '$SCRIPT_DIR/docker_host_lock.sh' status | grep -q 'HELD.*A2'"
check "session A releases"                         0 env CLAUDE_CODE_SESSION_ID=aaaaaaaa-1 LOCK_HOST=Node2 bash "$SCRIPT_DIR/docker_host_lock.sh" release
check "status is FREE afterwards"                  0 bash -c "bash '$SCRIPT_DIR/docker_host_lock.sh' status | grep -qx FREE"
check "an expired lease is stolen"                 0 bash -c "CLAUDE_CODE_SESSION_ID=aaaaaaaa-1 bash '$SCRIPT_DIR/docker_host_lock.sh' acquire -l 0 -m old >/dev/null && CLAUDE_CODE_SESSION_ID=bbbbbbbb-2 bash '$SCRIPT_DIR/docker_host_lock.sh' acquire -m new | grep -q 'stole dead lock'"

echo "docker_host_lock.sh on the host itself (LOCK_HOST=localhost, no ssh)"
check "session B releases the stolen lock"        0 env CLAUDE_CODE_SESSION_ID=bbbbbbbb-2 bash "$SCRIPT_DIR/docker_host_lock.sh" release
printf '#!/usr/bin/env bash
echo "ssh must not be used for localhost" >&2; exit 99
' > "$TMP/bin/ssh"
check "session A acquires without ssh"             0 env CLAUDE_CODE_SESSION_ID=aaaaaaaa-1 LOCK_HOST=localhost bash "$SCRIPT_DIR/docker_host_lock.sh" acquire -m A
check "session B is refused without ssh"           2 env CLAUDE_CODE_SESSION_ID=bbbbbbbb-2 LOCK_HOST=localhost bash "$SCRIPT_DIR/docker_host_lock.sh" acquire -m B
check "session A releases without ssh"             0 env CLAUDE_CODE_SESSION_ID=aaaaaaaa-1 LOCK_HOST=localhost bash "$SCRIPT_DIR/docker_host_lock.sh" release

echo "wsl_lock.sh"
check "session A acquires"                         0 env CLAUDE_CODE_SESSION_ID=aaaaaaaa-1 bash "$SCRIPT_DIR/wsl_lock.sh" acquire -m A
check "session B in the same checkout is refused"  2 env CLAUDE_CODE_SESSION_ID=bbbbbbbb-2 bash "$SCRIPT_DIR/wsl_lock.sh" acquire -m B
check "session B cannot release A's lock"          1 env CLAUDE_CODE_SESSION_ID=bbbbbbbb-2 bash "$SCRIPT_DIR/wsl_lock.sh" release
check "session A releases"                         0 env CLAUDE_CODE_SESSION_ID=aaaaaaaa-1 bash "$SCRIPT_DIR/wsl_lock.sh" release

[ "$FAILS" -eq 0 ] && echo "all passed" || { echo "$FAILS failed"; exit 1; }
