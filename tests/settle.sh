# settle.sh — the load-settle gate before the timing-sensitive behave passes,
# sourced by tests/entrypoint.sh.
#
# Both timing-sensitive @isolated scenarios and the resilience suite flake under
# host contention from whatever ran just before (an image build, the main pass).
# Containers share the host kernel, so /proc/stat is the real host CPU. This waits
# until the previous section's load has drained, capped at 8 minutes.
#
# It samples CPU busy % over 5 s windows rather than reading /proc/loadavg: the
# 1-minute load average is a moving average that took ~4 minutes to decay after
# a build had already finished, and the suite waited for the average, not the load.
# Tests: tests/test_settle.sh.

SETTLE_IDLE_PCT=50       # an idle host: under half of all cores busy
SETTLE_FLAT_PCT=10       # a host's own floor: three samples within 10 points
SETTLE_SATURATED_PCT=90  # ...that are not a saturated host
SETTLE_SAMPLE_S=5
SETTLE_MAX_S=480

# busy_pct_between <cpu line before> <cpu line after> — non-idle share of the
# interval in whole percent. Fields after "cpu": user nice system idle iowait ...;
# idle and iowait count as idle.
busy_pct_between() {
    awk -v a="$1" -v b="$2" 'BEGIN {
        n = split(a, x, " "); split(b, y, " "); tot = 0
        for (i = 2; i <= n; i++) tot += y[i] - x[i]
        idle = (y[5] - x[5]) + (y[6] - x[6])
        print (tot > 0) ? int(100 * (tot - idle) / tot + 0.5) : 0 }'
}

# settle_decision <busy % samples, oldest first> — succeeds when the load has
# drained: the latest sample is idle, or the last three are flat and not rising
# (a standing fleet never goes idle, so its own floor is the target) below
# saturation (a build still running is flat at ~100% too). POSIX sh: the
# entrypoint runs under dash.
settle_decision() {
    s_a=""; s_b=""; s_c=""
    for s_v in "$@"; do s_a=$s_b; s_b=$s_c; s_c=$s_v; done
    if [ "$s_c" -lt "$SETTLE_IDLE_PCT" ]; then echo "idle at ${s_c}% busy"; return 0; fi
    [ -n "$s_a" ] || return 1
    s_lo=$s_a; s_hi=$s_a
    for s_v in "$s_b" "$s_c"; do
        if [ "$s_v" -lt "$s_lo" ]; then s_lo=$s_v; fi
        if [ "$s_v" -gt "$s_hi" ]; then s_hi=$s_v; fi
    done
    if [ $((s_hi - s_lo)) -lt "$SETTLE_FLAT_PCT" ] && [ "$s_c" -le "$s_a" ] && [ "$s_c" -lt "$SETTLE_SATURATED_PCT" ]; then
        echo "flat at ${s_c}% busy (this host's floor)"; return 0
    fi
    return 1
}

wait_for_load_to_settle() {
    echo "=== Waiting for host load to settle before $1 ==="
    w_deadline=$(( $(date +%s) + SETTLE_MAX_S )); w_p1=""; w_p2=""
    while :; do
        w_before=$(head -1 /proc/stat); sleep "$SETTLE_SAMPLE_S"; w_after=$(head -1 /proc/stat)
        w_now=$(busy_pct_between "$w_before" "$w_after")
        if w_verdict=$(settle_decision $w_p2 $w_p1 "$w_now"); then
            echo "Host load settled: $w_verdict."; return 0
        fi
        if [ "$(date +%s)" -ge "$w_deadline" ]; then
            echo "Host still ${w_now}% busy after $((SETTLE_MAX_S / 60)) min — proceeding anyway."; return 0
        fi
        echo "  ${w_now}% busy — sampling again"
        w_p2=$w_p1; w_p1=$w_now
    done
}
