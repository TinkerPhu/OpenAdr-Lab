#!/usr/bin/env bash
# Does ci-wait.sh block until a ticket is finished, hand over the verdict and leave the ticket usable?
#
# Runs ci-wait.sh for real against a throwaway board, with `ssh` replaced by a stub that runs the
# command locally (and can be told to fail a few times, like a dropped connection). Pins: the exit
# code follows the verdict, a transient ssh failure is retried, a wait that runs out says how to
# continue by hand, and the ticket (status, result.md) is untouched and still listed by ci-next
# unless --ack was given.
#
# Usage:  bash ci/test_wait.sh
set -uo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

mkdir -p "$TMP/bin"
# ssh <host> <command...>: fail while $TMP/ssh_failures is above 0, else run the command locally.
cat > "$TMP/bin/ssh" <<'EOF'
#!/usr/bin/env bash
f="$(dirname "$0")/../ssh_failures"
if [ -s "$f" ] && [ "$(cat "$f")" -gt 0 ]; then echo $(( $(cat "$f") - 1 )) > "$f"; echo "ssh: connect timed out" >&2; exit 255; fi
shift
eval "$*"
EOF
chmod +x "$TMP/bin/ssh"
export PATH="$TMP/bin:$PATH"
unset BOARD_NO_MAIN
export CI_HOST=Node3 BOARD_DIR="$TMP/board" CI_WAIT_INTERVAL=1
mkdir -p "$BOARD_DIR"

FAILS=0
check() { # $1=name  $2=expected  $3=actual
    if [ "$2" = "$3" ]; then echo "  ok    $1"
    else echo "  FAIL  $1 (got '$3', want '$2')"; FAILS=$((FAILS + 1)); fi
}
ticket() { # <name> <status> - a ticket with a result.md
    mkdir -p "$BOARD_DIR/$1"
    printf 'BRANCH=fix/x\nSHA=abc12345\n' > "$BOARD_DIR/$1/request.env"
    echo "$2" > "$BOARD_DIR/$1/status"
    printf '# CI result: %s\n\n- verdict: **%s**\n' "$1" "$2" > "$BOARD_DIR/$1/result.md"
}
run() { bash "$HERE/ci-wait.sh" "$@" > "$TMP/out" 2>&1; echo $?; }

echo "arguments"
check "no CI_HOST is refused with a hint" 1 "$(env -u CI_HOST bash "$HERE/ci-wait.sh" t 2>&1 | grep -c 'set CI_HOST')"
check "no ticket name is refused"         1 "$(bash "$HERE/ci-wait.sh" 2>&1 | grep -c 'ticket')"

echo "verdicts"
ticket t_merged merged
check "merged exits 0"                    0 "$(run t_merged)"
check "the verdict is printed"            1 "$(grep -c 'verdict: \*\*merged\*\*' "$TMP/out")"
check "it names the ticket and the way on" 1 "$(grep -c 'ci-next.sh' "$TMP/out")"
ticket t_red red
check "red exits 1"                       1 "$(run t_red)"
ticket t_conflict conflict
check "conflict exits 1"                  1 "$(run t_conflict)"
ticket t_green green
check "green (check only) exits 0"        0 "$(run t_green)"
check "the ticket is left as it was"     "red" "$(cat "$BOARD_DIR/t_red/status")"
check "the result stays in the ticket"    1 "$(grep -c 'verdict' "$BOARD_DIR/t_red/result.md")"
check "an unacked ticket is still listed" 1 "$(BOARD_NO_MAIN= bash "$HERE/board.sh" list | grep -c t_red)"

echo "ack"
run t_red --ack >/dev/null
check "--ack hides it from ci-next"       0 "$(bash "$HERE/board.sh" list | grep -c t_red)"
check "--ack keeps the result"            1 "$(grep -c 'verdict' "$BOARD_DIR/t_red/result.md")"

echo "waiting"
ticket t_run running
( sleep 3; echo merged > "$BOARD_DIR/t_run/status" ) &
check "waits for a running ticket"        0 "$(run t_run)"
wait
check "it saw the final status"           1 "$(grep -c 'merged' "$TMP/out")"

echo "ssh drops"
ticket t_flaky merged
echo 2 > "$TMP/ssh_failures"
check "transient ssh failures are retried" 0 "$(run t_flaky)"
check "the failures were consumed"        0 "$(cat "$TMP/ssh_failures")"

echo "giving up"
ticket t_stuck queued
check "--max-wait ends with exit 3"       3 "$(run t_stuck --max-wait 2)"
check "it says how to continue by hand"   1 "$(grep -c 'ci-next.sh --all' "$TMP/out")"
check "a missing ticket gives up too"     3 "$(run no_such_ticket --max-wait 2)"

[ "$FAILS" -eq 0 ] && echo "all passed" || { echo "$FAILS failed"; exit 1; }
