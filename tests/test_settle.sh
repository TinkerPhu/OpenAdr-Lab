#!/bin/sh
# Does the load-settle gate (tests/settle.sh) decide "drained" correctly?
#
# The gate holds the timing-sensitive behave passes until the previous section's
# CPU load is gone. It reads CPU busy % straight from /proc/stat samples, so these
# cases feed it sample sequences instead of a real host.
#
# Usage:  sh tests/test_settle.sh   (POSIX sh: the entrypoint runs under dash)
set -u
. "$(dirname "$0")/settle.sh"

FAILS=0
check() { # $1=name  $2=expected (settled|busy)  $3...=busy % samples, oldest first
    name="$1"; want="$2"; shift 2
    if settle_decision "$@" >/dev/null; then got=settled; else got=busy; fi
    if [ "$got" = "$want" ]; then echo "  ok    $name"
    else echo "  FAIL  $name (got $got, want $want)"; FAILS=$((FAILS + 1)); fi
}

check "an idle host settles on the first sample"            settled 12
check "a busy host does not settle on one sample"           busy    85
check "a build still draining is not settled"               busy    95 80 62
check "a standing fleet's flat floor settles"               settled 68 71 66
check "a flat floor needs three samples"                    busy    70 70
check "a rising load is not settled"                        busy    60 64 72
check "a saturated host is not a floor (a build still runs)" busy    100 100 99

busy_pct_between "cpu  100 0 100 800 0 0 0 0 0 0" "cpu  150 0 150 900 0 0 0 0 0 0" \
    | grep -qx 50 && echo "  ok    busy % is non-idle share of the interval" \
    || { echo "  FAIL  busy % from /proc/stat deltas"; FAILS=$((FAILS + 1)); }

[ "$FAILS" -eq 0 ] && echo "all passed" || { echo "$FAILS failed"; exit 1; }
