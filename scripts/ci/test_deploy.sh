#!/usr/bin/env bash
# Does deploy.sh refuse anything outside its allowlists and issue the right steps in order?
# Runs it in dry-run mode: commands are printed, nothing is built, shipped or switched.
# Usage:  bash scripts/ci/test_deploy.sh
set -uo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$HERE/../.." || exit 1
FAILS=0
check() { if [ "$2" = "$3" ]; then echo "  ok    $1"; else echo "  FAIL  $1 (got '$3', want '$2')"; FAILS=$((FAILS + 1)); fi; }
deploy() { DEPLOY_DRY_RUN=1 bash scripts/ci/deploy.sh "$@" 2>&1; }
rc() { deploy "$@" >/dev/null; echo $?; }

check "an unknown target is refused"          1 "$(rc Node9 VEN ven-1)"
check "a target that is Node3 itself is refused" 1 "$(rc Node3 VEN ven-1)"
check "an unknown stack is refused"           1 "$(rc Node1 ../etc ven-1)"
check "a service with shell syntax is refused" 1 "$(rc Node1 VEN 'ven-1;rm -rf /')"
check "too few arguments are refused"         1 "$(rc Node1 VEN)"
check "a valid deploy is accepted"            0 "$(rc Node1 VEN ven-2)"

OUT="$(deploy Node1 VEN ven-2)"
order() { grep -n -F "$1" <<<"$OUT" | head -1 | cut -d: -f1; }
check "the lock is taken first"               1 "$(order 'docker_host_lock.sh acquire')"
[ "$(order 'compose -f VEN/docker-compose.yml build ven-2')" -lt "$(order 'docker load')" ]; check "build comes before shipping" 0 $?
[ "$(order ':previous')" -lt "$(order 'docker load')" ]; check "the old image is kept before the new one lands" 0 $?
[ "$(order 'docker load')" -lt "$(order 'up -d --no-build ven-2')" ]; check "shipping comes before switch-over" 0 $?
check "the VEN ui restarts after a ven backend" 1 "$(grep -c 'restart ui' <<<"$OUT")"
check "the lock is released last"             1 "$(tail -1 <<<"$OUT" | grep -c 'release')"
check "a non-VEN stack does not restart ui"   0 "$(deploy Node1 VTN vtn | grep -c 'restart ui')"
check "rollback tags :previous back and builds nothing" 0 "$(deploy --rollback Node2 VEN/scale_out/node2 ven-7 | grep -c ' build ')"

echo; [ "$FAILS" -eq 0 ] && echo "all passed" || { echo "$FAILS failed"; exit 1; }
