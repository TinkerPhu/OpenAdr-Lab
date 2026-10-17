#!/usr/bin/env bash
# runner.sh — the CI runner on Node3: works through the handoff board's queued tickets.
#
# Started every minute by the systemd timer (ci/systemd/), it takes the oldest queued
# ticket (ci/board.sh), holds the host's lock, and for the ticket's commit:
#   1. rebases it onto origin/main, so what is tested is exactly what would land;
#   2. quick tier: the python audits and, for code, fmt / clippy / cargo test / cargo audit
#      in the repo's unit-test harness (tests/docker-compose.ven-unit-test.yml);
#   3. full tier (TIERS contains "full"): run_all_tests.sh --rust --e2e --resilience here;
#   4. KIND=merge and everything green: fast-forwards origin/main to the tested commit.
# A change the suites cannot see (docs, .claude/, ci/) runs the audits and the CI mechanism tests only. The verdict goes into the ticket (status, result.md)
# Nothing here resumes a session: the user does that with ci/ci-next.sh.
#
# Installed by ci/install.sh into /srv/ci/bin, so a ticket that changes these very
# scripts is tested by the previous version of them.
set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CI_ROOT="${CI_ROOT:-/srv/ci}"
LOCK_SH="${CI_LOCK_SH:-$HERE/locks/docker_host_lock.sh}"   # the host lock, installed beside the runner
CI_REPO="${CI_REPO:-/srv/docker/openadr_lab}"
# The audits are stdlib python, but audit_step_definitions dry-runs behave: install.sh builds a venv.
PY="$CI_ROOT/venv/bin/python3"; [ -x "$PY" ] || PY=python3
export BOARD_DIR="${BOARD_DIR:-$CI_ROOT/board}" BOARD_NO_MAIN=1
# shellcheck source=board.sh
source "$HERE/board.sh"

UNIT_TEST="docker compose -f tests/docker-compose.ven-unit-test.yml"   # the harness's cargo caches are volumes
MAX_ATTEMPTS=3
export GIT_COMMITTER_NAME="Node3 CI runner" GIT_COMMITTER_EMAIL="ci@node3.invalid"

STEPS=()          # "name|exit|seconds"
FAILED_STEP=""
VERDICT=""        # merged | green | red | conflict | stale | error | requeue
NOTE=""           # one line for result.md

log() { echo "[$(date -u +%H:%M:%SZ)] $*"; }

# run_step <name> <command...> — timed; remembers the first failing step.
run_step() {
    local name="$1" t0 rc; shift
    t0=$(date +%s)
    log "=== $name"
    "$@"; rc=$?
    STEPS+=("$name|$rc|$(( $(date +%s) - t0 ))")
    [ "$rc" -eq 0 ] || { [ -n "$FAILED_STEP" ] || FAILED_STEP="$name"; log "--- $name FAILED (exit $rc)"; }
    return "$rc"
}

acquire_lock() {
    local i
    for i in 1 2 3; do
        LOCK_OWNER=ci-runner LOCK_HOST=localhost bash "$LOCK_SH" acquire -l 150 -m "ci: $1" && return 0
        [ $? -eq 2 ] || return 1
    done
    return 1
}
release_lock() { LOCK_OWNER=ci-runner LOCK_HOST=localhost bash "$LOCK_SH" release || true; }

# checkout_rebased <sha> — the checkout at <sha> rebased onto origin/main. 0 ok, 3 conflict, 1 other.
checkout_rebased() {
    cd "$CI_REPO" || return 1
    git rebase --abort >/dev/null 2>&1
    git reset -q --hard && git clean -fdq
    git fetch -q origin --prune || return 1
    git cat-file -e "$1^{commit}" 2>/dev/null || { NOTE="commit $1 is not on origin: push the branch first"; return 1; }
    git checkout -q --detach "$1" || return 1
    if ! git rebase -q origin/main; then
        git rebase --abort >/dev/null 2>&1
        git checkout -q --detach "$1"
        NOTE="rebase onto origin/main ($(git rev-parse --short origin/main)) conflicts"
        return 3
    fi
    git submodule update -q --init --recursive
}

quick_tier() { # $1 = the selected suites (board_suites); the audits and self-tests always run
    local s
    for s in audit_file_sizes audit_duplication audit_debt_gate audit_ven_architecture audit_step_definitions; do
        run_step "$s" "$PY" "ci/audits/$s.py" >>"$TICKET_DIR/quick.log" 2>&1 || return 1
    done
    # The CI mechanism has no other test: a change to it skips the suites, so these always run.
    for s in test_board test_runner test_handoff test_deploy test_web; do
        run_step "ci/$s" bash "ci/$s.sh" >>"$TICKET_DIR/quick.log" 2>&1 || return 1
    done
    [[ " $1 " == *" cargo "* ]] || return 0
    # The harness copies VEN/ and lab-core/ into its image, so the image is rebuilt from this commit;
    # the cargo registry and target are volumes, so a run compiles only what changed.
    run_step "unit-test image" $UNIT_TEST build ven-unit-test >>"$TICKET_DIR/quick.log" 2>&1 || return 1
    run_step "cargo fmt" $UNIT_TEST run --rm ven-unit-test cargo fmt --check >>"$TICKET_DIR/quick.log" 2>&1 || return 1
    run_step "cargo clippy" $UNIT_TEST run --rm ven-unit-test cargo clippy --locked --all-targets --all-features -- -D warnings >>"$TICKET_DIR/quick.log" 2>&1 || return 1
    run_step "cargo test" $UNIT_TEST run --rm ven-unit-test cargo test --locked --jobs 2 >>"$TICKET_DIR/quick.log" 2>&1 || return 1
    run_step "cargo audit (VEN)" $UNIT_TEST run --rm ven-unit-test cargo audit >>"$TICKET_DIR/quick.log" 2>&1 || return 1
    run_step "cargo audit (BFF)" $UNIT_TEST run --rm -v "$CI_REPO/VTN/bff:/bff:ro" -w /bff ven-unit-test cargo audit >>"$TICKET_DIR/quick.log" 2>&1 || return 1
}

# suite_flags <suites> - the run_all_tests.sh flags of the full-tier suites among them (cargo is the quick tier's)
suite_flags() {
    local s
    for s in $1; do
        case "$s" in rust-vtn) echo "--rust" ;; e2e|resilience) echo "--$s" ;; esac
    done
}

full_tier() { # $1 = the selected suites
    local flags; flags="$(suite_flags "$1" | tr '\n' ' ')"
    [ -n "$flags" ] || return 0
    run_step "run_all_tests.sh ${flags% }" \
        env -u DOCKER_HOST OPENADR_LAB_HOST=localhost LOCK_HELD_BY_CALLER=1 \
        bash run_all_tests.sh $flags >"$TICKET_DIR/full.log" 2>&1
}

# After a run, leave no test containers behind and the checkout on main, as the lock rules ask.
tidy_host() {
    cd "$CI_REPO" || return 0
    docker ps -aq --filter name=openadr-test | xargs -r docker rm -f >/dev/null 2>&1
    docker compose -f tests/docker-compose.test.yml down -v >/dev/null 2>&1
    git rebase --abort >/dev/null 2>&1
    git reset -q --hard; git clean -fdq
    git checkout -q main && git reset -q --hard origin/main
}

push_main() { # $1 = tested sha — fast-forward only
    cd "$CI_REPO" || return 1
    git fetch -q origin main || return 1
    git merge-base --is-ancestor origin/main "$1" || return 9
    git push -q origin "$1:refs/heads/main"
}

write_result() { # $1 = ticket
    local s name rc secs
    {
        echo "# CI result: $1"
        echo
        echo "- verdict: **$VERDICT**${NOTE:+ — $NOTE}"
        echo "- branch: $BRANCH   requested: $SHA   tested: ${TESTED_SHA:-(not reached)}"
        echo "- tiers: $TIERS   kind: $KIND   attempt: $(cat "$TICKET_DIR/attempt" 2>/dev/null)"
        echo "- finished: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
        [ "$VERDICT" = merged ] && echo "- origin/main is now at: $TESTED_SHA (the rebased commit; the branch's own sha differs if main moved)"
        echo
        if [ -n "${CHANGED_WHY+x}" ]; then
            echo "## Suites"; echo
            echo "- run: ${SUITES_RUN:-(none)}"
            echo "- skipped: ${SUITES_SKIPPED:-(none)}"
            echo "- decided by:"; printf '%s\n' "$CHANGED_WHY" | head -n 5 | sed 's/^/  - /'
            echo
        fi
        echo "| step | exit | seconds |"
        echo "|---|---|---|"
        for s in "${STEPS[@]:-}"; do
            [ -n "$s" ] || continue
            IFS='|' read -r name rc secs <<<"$s"; echo "| $name | $rc | $secs |"
        done
        if [ -s "$TICKET_DIR/full.log" ] && grep -q '^FLAKY_SCENARIO:' "$TICKET_DIR/full.log"; then
            echo; echo "## Flaky: failed once, passed on its own retry (look at these)"; echo; echo '```'
            grep -h '^FLAKY_SCENARIO:' "$TICKET_DIR/full.log" | sort -u
            echo '```'
        fi
        if [ -n "$FAILED_STEP" ]; then
            echo; echo "## Failed: $FAILED_STEP"; echo
            if [ -s "$TICKET_DIR/full.log" ] && grep -q "Failing scenarios" "$TICKET_DIR/full.log" 2>/dev/null; then
                echo '```'; grep -A40 "Failing scenarios" "$TICKET_DIR/full.log" | head -50; echo '```'
            else
                echo '```'
                for f in quick.log full.log; do
                    [ -s "$TICKET_DIR/$f" ] && { echo "--- $f (last lines)"; tail -n 40 "$TICKET_DIR/$f" | cut -c1-240; }
                done
                echo '```'
            fi
        fi
        echo; echo "Logs: $TICKET_DIR/{run,quick,full}.log on Node3"
    } > "$TICKET_DIR/result.md.tmp" && mv "$TICKET_DIR/result.md.tmp" "$TICKET_DIR/result.md"
}

# deploy_ticket — KIND=deploy: ship what origin/main holds (it was tested when it merged); the
# ticket's own SHA only names what the session believed it was deploying.
deploy_ticket() { # [--rollback] <target> <stack> <service>...
    cd "$CI_REPO" || return 1
    git reset -q --hard && git clean -fdq && git fetch -q origin --prune || return 1
    git checkout -q --detach origin/main && git submodule update -q --init --recursive || return 1
    TESTED_SHA="$(git rev-parse HEAD)"
    run_step "deploy ${*}" bash ci/deploy.sh "$@" >>"$TICKET_DIR/full.log" 2>&1
}

# test_and_merge — rebase, quick tier, full tier, merge: sets VERDICT (and NOTE).
test_and_merge() {
    local rc
    checkout_rebased "$SHA"; rc=$?
    if [ $rc -eq 3 ]; then VERDICT=conflict
    elif [ $rc -ne 0 ]; then VERDICT=error
    else
        TESTED_SHA="$(git -C "$CI_REPO" rev-parse HEAD)"
        select_suites
        if ! quick_tier "$SUITES_RUN"; then VERDICT=red
        elif [[ ",$TIERS," == *,full,* ]] && ! full_tier "$SUITES_RUN"; then VERDICT=red
        elif [ "$KIND" != merge ] || [ -n "$SUITES_UNRUN" ]; then
            VERDICT=green; NOTE="tested, not merged (kind $KIND, tiers $TIERS${SUITES_UNRUN:+, not run: $SUITES_UNRUN})"
        else
            push_main "$TESTED_SHA"; rc=$?
            if [ $rc -eq 0 ]; then VERDICT=merged
            elif [ $rc -eq 9 ]; then VERDICT=requeue; NOTE="origin/main moved while testing"
            else VERDICT=error; NOTE="push to origin/main failed (exit $rc)"; fi
        fi
    fi
}

# select_suites - from the paths this ticket changes, set SUITES_RUN (to run), SUITES_SKIPPED (the map
# says the change cannot affect them), SUITES_UNRUN (selected but outside the ticket's tiers, so
# the commit may not merge) and CHANGED_WHY (path -> suites, for result.md).
select_suites() {
    local changed s
    changed="$(git -C "$CI_REPO" diff --name-only "origin/main..$TESTED_SHA")"
    CHANGED_WHY="$(board_suites_why <<<"$changed")"
    SUITES_RUN="$(board_suites <<<"$changed" | tr '\n' ' ' | sed 's/ $//')"
    SUITES_SKIPPED=""; SUITES_UNRUN=""
    for s in $BOARD_ALL_SUITES; do
        [[ " $SUITES_RUN " == *" $s "* ]] || SUITES_SKIPPED="${SUITES_SKIPPED:+$SUITES_SKIPPED }$s"
    done
    if [[ ",$TIERS," != *,full,* ]]; then
        for s in $SUITES_RUN; do
            case "$s" in cargo) ;; *) SUITES_UNRUN="${SUITES_UNRUN:+$SUITES_UNRUN }$s" ;; esac
        done
    fi
}

deploy_flow() { # $1 = ticket
    local rollback=()
    [ "$(board_get "$1" ROLLBACK)" = yes ] && rollback=(--rollback)
    # SERVICES is a space-separated list of names; deploy.sh checks every one against its allowlist.
    # shellcheck disable=SC2046
    if deploy_ticket "${rollback[@]}" "$(board_get "$1" TARGET)" "$(board_get "$1" STACK)" $(board_get "$1" SERVICES)
    then VERDICT=deployed; else VERDICT=red; fi
}

process_ticket() {
    local t="$1" attempt rc
    TICKET_DIR="$BOARD_DIR/$t"; STEPS=(); FAILED_STEP=""; VERDICT=""; NOTE=""; TESTED_SHA=""; unset CHANGED_WHY; SUITES_RUN=""; SUITES_SKIPPED=""; SUITES_UNRUN=""
    BRANCH="$(board_get "$t" BRANCH)"; SHA="$(board_get "$t" SHA)"
    TIERS="$(board_get "$t" TIERS)"; KIND="$(board_get "$t" KIND)"
    TIERS="${TIERS:-quick,full}"; KIND="${KIND:-merge}"
    attempt=$(board_bump_attempt "$t")
    board_set_status "$t" running
    exec 8>>"$TICKET_DIR/run.log"
    {
        log "ticket $t (attempt $attempt): $BRANCH @ $SHA, tiers $TIERS, kind $KIND"
        if ! acquire_lock "$t"; then
            VERDICT=requeue; NOTE="host lock busy"
        else
            if [ "$KIND" = deploy ]; then deploy_flow "$t"; else test_and_merge; fi
            tidy_host
            release_lock
        fi
        if [ "$VERDICT" = requeue ]; then
            if [ "$NOTE" = "host lock busy" ]; then
                board_write "$TICKET_DIR/attempt" $((attempt - 1))   # waiting is not an attempt
            elif [ "$attempt" -ge "$MAX_ATTEMPTS" ]; then
                VERDICT=stale; NOTE="${NOTE}; gave up after $attempt attempts"
            fi
        fi
        log "verdict: $VERDICT ${NOTE:+($NOTE)}"
    } >&8 2>&1
    exec 8>&-
    write_result "$t"
    if [ "$VERDICT" = requeue ]; then board_set_status "$t" queued; else board_set_status "$t" "$VERDICT"; fi
}

main() {
    local t
    mkdir -p "$BOARD_DIR"
    exec 9>"$CI_ROOT/runner.lock"; flock -n 9 || exit 0     # one runner at a time
    # A ticket left running by a crashed runner: nobody else can hold the flock, so it is dead.
    for t in $(board_running); do
        if [ "$(cat "$BOARD_DIR/$t/attempt" 2>/dev/null || echo 0)" -ge "$MAX_ATTEMPTS" ]; then
            board_set_status "$t" error
        else
            board_set_status "$t" queued
        fi
    done
    while t="$(board_next_queued)"; [ -n "$t" ]; do
        process_ticket "$t"
        # A requeued ticket waits for the next timer tick instead of spinning on a busy lock.
        [ "$(board_status "$t")" = queued ] && break
    done
}

if [ "${RUNNER_NO_MAIN:-}" != 1 ]; then main "$@"; fi
