#!/usr/bin/env bash
# lanes.sh — the full tier on several docker hosts at once. Sourced by runner.sh, never run.
#
# A lane is one docker host running its share of the ticket's suites one after the other. The
# runner's own host is always lane 1 (its lock is already held for the ticket); the hosts in
# CI_LANE_HOSTS, in that priority order, are extra lanes, taken with the same lease lock the
# sessions use (ci/locks/docker_host_lock.sh), once and without waiting: a host a session holds is
# simply not used. The tested commit reaches an extra host as refs/ci/<ticket> on origin.
#
#   lanes_acquire N        take up to N-1 extra hosts (LANE_HOSTS, LANE_SKIPPED)
#   lane_plan HOSTS SUITES longest suite first onto the lane that would finish earliest
#   run_lane               one lane's job (background): its suites, then tidy and unlock
#   full_tier SUITES       the whole thing; returns 0 when every suite passed
#
# Needs from runner.sh: TICKET_DIR, TICKET_NAME, TESTED_SHA, STEPS, FAILED_STEP, CI_REPO, LOCK_SH, log.
# Needs from board.sh: suite_flags, board_suite_seconds.

CI_HOME_NAME="${CI_HOME_NAME:-$(hostname -s 2>/dev/null || echo localhost)}"
LANE_HOSTS_CONF="${CI_LANE_HOSTS:-}"          # extra hosts, priority order; empty = one lane
LANE_LEASE_MIN=45                              # an extra lane's lock; the heartbeat renews it
LANE_HEARTBEAT_S="${CI_LANE_HEARTBEAT_S:-900}"
LANE_POLL_S="${CI_LANE_POLL_S:-5}"
LANE_GUARD_HOSTS="${CI_LANE_GUARD_HOSTS-Node1}"  # hosts that also serve something else
LANE_MAX_LOAD="${CI_LANE_MAX_LOAD:-2.0}"
LANE_MIN_MEM_MB="${CI_LANE_MIN_MEM_MB:-1500}"

# ── Reaching a host ──────────────────────────────────────────────────────────

lane_target() { if [ "$1" = "$CI_HOME_NAME" ]; then echo localhost; else echo "$1"; fi; }

# lane_sh <host> <command> - a shell command on the host (here for the home lane)
lane_sh() {
    if [ "$(lane_target "$1")" = localhost ]; then bash -c "$2"
    else ssh -n -o BatchMode=yes -o ConnectTimeout=10 "$1" "$2"; fi
}

# lane_lock <op> <host> [lock args] - the sessions' lock script, one owner per ticket
lane_lock() {
    local op="$1" host="$2" owner=ci-runner; shift 2
    [ "$(lane_target "$host")" = localhost ] || owner="ci-runner@$TICKET_NAME"
    LOCK_OWNER="$owner" LOCK_HOST="$(lane_target "$host")" LOCK_MAX_WAIT_SEC="${LANE_LOCK_WAIT:-540}" \
        bash "$LOCK_SH" "$op" "$@"
}

# lane_acquire <host> - once, no waiting. 0 got it, 2 somebody else holds it (LANE_WHY), 1 failed
lane_acquire() {
    local out rc LANE_LOCK_WAIT=0
    out="$(lane_lock acquire "$1" -l "$LANE_LEASE_MIN" -m "ci: $TICKET_NAME" 2>&1)"; rc=$?
    [ "$rc" -eq 0 ] && return 0
    LANE_WHY="$(printf '%s\n' "$out" | sed -n '1p' | cut -c1-120)"
    [ "$rc" -eq 2 ] && return 2
    return 1
}
lane_release() { lane_lock release "$1" >/dev/null 2>&1 || true; }

# lane_host_ok <host> - reachable, and for a host that serves something else, not busy (LANE_WHY)
lane_host_ok() {
    local h="$1" load mem
    lane_sh "$h" true >/dev/null 2>&1 || { LANE_WHY="unreachable over ssh"; return 1; }
    case " $LANE_GUARD_HOSTS " in *" $h "*) ;; *) return 0 ;; esac
    load="$(lane_sh "$h" "cut -d' ' -f1 /proc/loadavg" 2>/dev/null)"
    mem="$(lane_sh "$h" "awk '/MemAvailable/{print int(\$2/1024)}' /proc/meminfo" 2>/dev/null)"
    awk -v l="${load:-99}" -v m="${mem:-0}" -v ml="$LANE_MAX_LOAD" -v mm="$LANE_MIN_MEM_MB" \
        'BEGIN { exit !(l <= ml && m >= mm) }' ||
        { LANE_WHY="load ${load:-?}, ${mem:-?} MB free (limits $LANE_MAX_LOAD and $LANE_MIN_MEM_MB MB)"; return 1; }
}

# ── The tested commit on an extra host ───────────────────────────────────────

lane_publish()   { git -C "$CI_REPO" push -q origin "$TESTED_SHA:refs/ci/$TICKET_NAME"; }
lane_unpublish() { git -C "$CI_REPO" push -q origin ":refs/ci/$TICKET_NAME" >/dev/null 2>&1 || true; }

# lane_prepare <host> - the host's checkout at the tested commit, detached. A dirty tree is
# refused before anything is touched: it is somebody's work.
lane_prepare() {
    local h="$1" dirty repo; repo="$(printf '%q' "$CI_REPO")"
    dirty="$(lane_sh "$h" "cd $repo && git status --porcelain | head -3" 2>&1)" ||
        { LANE_WHY="cannot read the checkout: ${dirty:0:80}"; return 1; }
    [ -z "$dirty" ] || { LANE_WHY="checkout has uncommitted changes"; return 1; }
    if ! lane_sh "$h" "cd $repo && git fetch -q origin refs/ci/$TICKET_NAME && git checkout -q --detach FETCH_HEAD && git submodule update -q --init --recursive" >>"$TICKET_DIR/run.log" 2>&1; then
        tidy_tree "$h"; LANE_WHY="fetching refs/ci/$TICKET_NAME failed"; return 1
    fi
}

# ── Leaving a host clean ─────────────────────────────────────────────────────

# Only containers of the test compose projects, found by their project label: a host also runs
# production or a resident fleet, and those are never touched.
lane_tidy_script() { cat <<'EOS'
for p in openadr-test openadr-cargo-test; do
    docker ps -aq --filter "label=com.docker.compose.project=$p" | xargs -r docker rm -f >/dev/null 2>&1
done
docker compose -f tests/docker-compose.test.yml down -v >/dev/null 2>&1
docker compose -f tests/docker-compose.openleadr-test.yml down -v >/dev/null 2>&1
left=0
for p in openadr-test openadr-cargo-test; do
    left=$((left + $(docker ps -aq --filter "label=com.docker.compose.project=$p" | wc -l)))
done
[ "$left" -eq 0 ]
EOS
}
tidy_containers() { # <host>
    lane_sh "$1" "cd $(printf '%q' "$CI_REPO") 2>/dev/null || exit 0; $(lane_tidy_script)" >/dev/null 2>&1 ||
        log "WARNING: containers of the test projects are left on $1"
}
tidy_tree() { # <host> - the checkout back on main, as the lock rules ask
    lane_sh "$1" "cd $(printf '%q' "$CI_REPO") 2>/dev/null || exit 0
git rebase --abort >/dev/null 2>&1; git reset -q --hard; git clean -fdq
git fetch -q origin; git checkout -q main && git reset -q --hard origin/main" >/dev/null 2>&1
}
tidy_host() { tidy_containers "${1:-$CI_HOME_NAME}"; tidy_tree "${1:-$CI_HOME_NAME}"; }

# ── Acquiring lanes ──────────────────────────────────────────────────────────

LANE_HOSTS=(); LANE_SKIPPED=(); LANE_PUBLISHED=no

# lanes_acquire <number of suites> - LANE_HOSTS = the home lane plus up to N-1 extra hosts, in
# priority order; LANE_SKIPPED = "host|reason" for the ones not used.
lanes_acquire() {
    local want=$(( $1 - 1 )) h rc LANE_WHY=""
    LANE_HOSTS=("$CI_HOME_NAME"); LANE_SKIPPED=(); LANE_PUBLISHED=no
    for h in $LANE_HOSTS_CONF; do
        [ "$h" != "$CI_HOME_NAME" ] || continue
        [ $(( ${#LANE_HOSTS[@]} - 1 )) -lt "$want" ] || break
        lane_host_ok "$h" || { LANE_SKIPPED+=("$h|$LANE_WHY"); continue; }
        lane_acquire "$h"; rc=$?
        [ "$rc" -eq 0 ] || { LANE_SKIPPED+=("$h|${LANE_WHY:-lock failed}"); continue; }
        if [ "$LANE_PUBLISHED" = no ]; then
            if lane_publish; then LANE_PUBLISHED=yes
            else lane_release "$h"; LANE_SKIPPED+=("$h|cannot publish refs/ci/$TICKET_NAME to origin"); break; fi
        fi
        if ! lane_prepare "$h"; then lane_release "$h"; LANE_SKIPPED+=("$h|$LANE_WHY"); continue; fi
        LANE_HOSTS+=("$h")
    done
}

# ── Planning ─────────────────────────────────────────────────────────────────

lane_suite_seconds() { board_suite_seconds "$@"; }

# lane_plan "<hosts>" "<suites>" - "host: suites" per lane that got any. Longest suite first (by
# its time on the first lane), each onto the lane whose projected end, by that host's own median, is earliest.
lane_plan() {
    local -a hosts=($1) load=() plan=()
    local i s bi best end secs
    for i in "${!hosts[@]}"; do load[i]=0; plan[i]=""; done
    for s in $(for s in $2; do echo "$(lane_suite_seconds "$s" "${hosts[0]}") $s"; done | sort -rn -s -k1,1 | awk '{print $2}'); do
        bi=-1; best=0
        for i in "${!hosts[@]}"; do
            secs="$(lane_suite_seconds "$s" "${hosts[i]}")"; end=$(( load[i] + secs ))
            if [ "$bi" -lt 0 ] || [ "$end" -lt "$best" ]; then bi=$i; best=$end; fi
        done
        plan[bi]="${plan[bi]} $s"; load[bi]=$best
    done
    for i in "${!hosts[@]}"; do [ -z "${plan[i]}" ] || echo "${hosts[i]}: ${plan[i]# }"; done
}

# ── One lane ─────────────────────────────────────────────────────────────────

# lane_run_suite <host> <suite> <logfile> - run_all_tests.sh for one suite; its exit code
lane_run_suite() {
    local h="$1" flag; flag="$(suite_flags "$2")"
    cd "$CI_REPO" || return 1
    if [ "$(lane_target "$h")" = localhost ]; then
        env -u DOCKER_HOST OPENADR_LAB_HOST=localhost LOCK_HELD_BY_CALLER=1 bash run_all_tests.sh $flag >>"$3" 2>&1
    else
        env DOCKER_HOST="$h" LOCK_HELD_BY_CALLER=1 TREE_PREPARED_BY_CALLER=1 bash run_all_tests.sh $flag >>"$3" 2>&1
    fi
}

# lane_close <host> <rows file> - on every way out of a lane job: no test container left, an extra
# host back on main and unlocked (the home lane's tree and lock belong to the ticket), then "done".
lane_close() {
    [ -z "${LANE_HB:-}" ] || kill "$LANE_HB" 2>/dev/null
    tidy_containers "$1"
    if [ "$1" != "$CI_HOME_NAME" ]; then tidy_tree "$1"; lane_release "$1"; fi
    : > "$2.done"
}

# run_lane <host> <rows file> <suites...> - rows: "suite|exit|seconds|kind|why", kind ok, test or
# infra. A suite that reports no PASS (skipped, host unreachable) is not a pass.
# run_all_tests.sh colours its verdict lines: ESC[0;32mPASS ESC[0m then two spaces.
LANE_PASS_RE=$'PASS(\[[0-9;]*m)?  '
run_lane() {
    local h="$1" rows="$2" s t0 rc kind why before log; shift 2
    : > "$rows"
    log="$TICKET_DIR/full.$h.log"; [ "$h" != "$CI_HOME_NAME" ] || log="$TICKET_DIR/full.log"
    trap "lane_close '$h' '$rows'" EXIT
    trap 'exit 143' TERM
    if [ "$h" != "$CI_HOME_NAME" ]; then
        ( trap 'kill "$nap" 2>/dev/null; exit 0' TERM   # no orphaned sleep when the lane ends
          while :; do sleep "$LANE_HEARTBEAT_S" & nap=$!; wait "$nap"; lane_lock refresh "$h" -l "$LANE_LEASE_MIN" >/dev/null 2>&1; done ) &
        LANE_HB=$!
    fi
    for s in "$@"; do
        before="$(grep -cE "$LANE_PASS_RE" "$log" 2>/dev/null)"; before="${before:-0}"
        t0=$(date +%s); why=""
        lane_run_suite "$h" "$s" "$log"; rc=$?
        kind=ok
        if [ "$rc" -ne 0 ]; then
            case "$rc" in 3) kind=infra; why="pre-flight: capacity" ;; 4) kind=infra; why="pre-flight: checkout incomplete" ;;
                          5) kind=infra; why="pre-flight: compose" ;; 255) kind=infra; why="ssh failed" ;; *) kind=test ;; esac
        elif [ "$(grep -cE "$LANE_PASS_RE" "$log" 2>/dev/null)" -le "$before" ]; then
            kind=infra; why="no result (skipped or unreachable)"
        fi
        echo "$s|$rc|$(( $(date +%s) - t0 ))|$kind|$why" >> "$rows"
        [ "$kind" = ok ] || break
    done
}

# ── The lanes file: what the board and result.md show ────────────────────────

# lanes_write - $TICKET_DIR/lanes from the LANE_*_LINE array ("host|suites|state|seconds|note")
lanes_write() {
    local l
    { for l in "${LANE_LINES[@]:-}"; do [ -z "$l" ] || echo "$l"; done
      for l in "${LANE_SKIPPED[@]:-}"; do [ -z "$l" ] || echo "skipped|$l"; done; } > "$TICKET_DIR/lanes.tmp"
    mv "$TICKET_DIR/lanes.tmp" "$TICKET_DIR/lanes"
}

# lanes_result_block - the "## Lanes" section of result.md, from the lanes file
lanes_result_block() {
    local h su st secs note
    [ -s "$TICKET_DIR/lanes" ] || return 0
    echo; echo "## Lanes"; echo
    echo "| host | suites | outcome | seconds | note |"; echo "|---|---|---|---|---|"
    while IFS='|' read -r h su st secs note; do
        [ "$h" != skipped ] || continue
        echo "| $h | $su | $st | $secs | $note |"
    done < "$TICKET_DIR/lanes"
    if grep -q '^skipped|' "$TICKET_DIR/lanes"; then
        echo; echo "Not used:"
        while IFS='|' read -r h su st; do [ "$h" = skipped ] && echo "- $su: $st"; done < "$TICKET_DIR/lanes"
    fi
}

# ── The full tier ────────────────────────────────────────────────────────────

# lane_step_name <suite> <host> - the step row: run_all_tests.sh --e2e [Node3]
lane_step_name() { echo "run_all_tests.sh $(suite_flags "$1") [$2]"; }

# full_tier <selected suites> - 0 when every full-tier suite passed
full_tier() {
    local suites="" s n line i h rows pid cancelled=no pending any_test d
    local -a plan_hosts=() plan_suites=() pids=() cancelled_lane=()
    for s in $1; do [ -z "$(suite_flags "$s")" ] || suites="$suites $s"; done
    suites="${suites# }"; [ -n "$suites" ] || return 0
    n=$(wc -w <<<"$suites")
    lanes_acquire "$n"
    while IFS= read -r line; do plan_hosts+=("${line%%: *}"); plan_suites+=("${line#*: }"); done < <(lane_plan "${LANE_HOSTS[*]}" "$suites")
    # An acquired host the plan gave nothing to is returned at once.
    for h in "${LANE_HOSTS[@]}"; do
        [[ " ${plan_hosts[*]} " == *" $h "* ]] || { tidy_tree "$h"; lane_release "$h"; }
    done
    LANE_LINES=()
    for i in "${!plan_hosts[@]}"; do LANE_LINES[i]="${plan_hosts[i]}|${plan_suites[i]}|running||"; done
    lanes_write
    rm -f "$TICKET_DIR"/lane.*
    set -m
    for i in "${!plan_hosts[@]}"; do
        ( run_lane "${plan_hosts[i]}" "$TICKET_DIR/lane.${plan_hosts[i]}" ${plan_suites[i]} ) &
        pids[i]=$!
    done
    set +m
    # Wait for every lane; the first suite that fails a test stops the others.
    while :; do
        pending=0; any_test=""
        for i in "${!plan_hosts[@]}"; do
            [ -e "$TICKET_DIR/lane.${plan_hosts[i]}.done" ] || pending=1
            grep -q '|test|' "$TICKET_DIR/lane.${plan_hosts[i]}" 2>/dev/null && any_test="${plan_hosts[i]}"
        done
        [ "$pending" -eq 1 ] || break
        if [ -n "$any_test" ] && [ "$cancelled" = no ]; then
            cancelled=yes
            log "a suite failed on $any_test: stopping the other lanes"
            for i in "${!plan_hosts[@]}"; do
                [ -e "$TICKET_DIR/lane.${plan_hosts[i]}.done" ] && continue
                grep -q '|\(test\|infra\)|' "$TICKET_DIR/lane.${plan_hosts[i]}" 2>/dev/null && continue   # already ending
                cancelled_lane[i]="stopped: a suite failed on $any_test"
                kill -TERM -- "-${pids[i]}" 2>/dev/null
            done
        fi
        sleep "$LANE_POLL_S"
    done
    wait
    fold_lanes "$cancelled"
    [ "$LANE_PUBLISHED" = no ] || { lane_unpublish; LANE_PUBLISHED=no; }
    [ -z "$FAILED_STEP" ]
}

# fold_lanes - the lanes' rows into STEPS and the lanes file; an infrastructure failure on an
# extra lane is rerun on the home lane afterwards.
fold_lanes() {
    local cancelled="$1" i h su rows state secs note s rc sec kind why moved="" mwhy="" ran seen
    for i in "${!plan_hosts[@]}"; do
        h="${plan_hosts[i]}"; su="${plan_suites[i]}"; rows="$TICKET_DIR/lane.$h"; state=ok; secs=0; note="${cancelled_lane[i]:-}"
        seen=" "
        while IFS='|' read -r s rc sec kind why; do
            [ -n "$s" ] || continue
            seen="$seen$s "; secs=$((secs + sec))
            case "$kind" in
                ok)    STEPS+=("$(lane_step_name "$s" "$h")|$rc|$sec") ;;
                test)  STEPS+=("$(lane_step_name "$s" "$h")|$rc|$sec"); [ -n "$FAILED_STEP" ] || FAILED_STEP="$(lane_step_name "$s" "$h")"; state=red ;;
                infra) if [ "$h" = "$CI_HOME_NAME" ]; then
                           STEPS+=("$(lane_step_name "$s" "$h")|$rc|$sec"); [ -n "$FAILED_STEP" ] || FAILED_STEP="$(lane_step_name "$s" "$h")"; state=red; note="$why"
                       else moved="$moved $s"; mwhy="$why"; state=moved; note="infrastructure ($why)"; fi ;;
            esac
        done < "$rows"
        for s in $su; do   # suites that never produced a row
            [[ "$seen" == *" $s "* ]] && continue
            if [ -n "${cancelled_lane[i]:-}" ]; then state=cancelled
            elif [ "$state" = moved ]; then moved="$moved $s"
            elif [ "$state" = ok ]; then state=moved; moved="$moved $s"; mwhy="lane ended early"; note="infrastructure (lane ended early)"; fi
        done
        [ -z "${cancelled_lane[i]:-}" ] || [ "$state" = red ] || state=cancelled
        LANE_LINES[i]="$h|$su|$state|$secs|$note"
    done
    if [ -n "$moved" ] && [ -z "$FAILED_STEP" ]; then
        rows="$TICKET_DIR/lane.$CI_HOME_NAME.moved"
        ( run_lane "$CI_HOME_NAME" "$rows" $moved )
        secs=0
        while IFS='|' read -r s rc sec kind why; do
            [ -n "$s" ] || continue; secs=$((secs + sec))
            STEPS+=("$(lane_step_name "$s" "$CI_HOME_NAME")|$rc|$sec")
            [ "$kind" = ok ] || { [ -n "$FAILED_STEP" ] || FAILED_STEP="$(lane_step_name "$s" "$CI_HOME_NAME")"; }
        done < "$rows"
        for i in "${!plan_hosts[@]}"; do
            [[ "${LANE_LINES[i]}" == *"|moved|"* ]] && LANE_LINES[i]="${LANE_LINES[i]%|*}|moved to $CI_HOME_NAME, reran ${moved# } in ${secs}s"
        done
    fi
    lanes_write
}
