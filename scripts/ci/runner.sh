#!/usr/bin/env bash
# runner.sh — the CI runner on Node3: works through the handoff board's queued tickets.
#
# Started every minute by the systemd timer (scripts/ci/systemd/), it takes the oldest queued
# ticket (scripts/ci/board.sh), holds the host's lock, and for the ticket's commit:
#   1. rebases it onto origin/main, so what is tested is exactly what would land;
#   2. quick tier: the python audits and, for code, fmt / clippy / cargo test / cargo audit
#      in the repo's unit-test harness (tests/docker-compose.ven-unit-test.yml);
#   3. full tier (TIERS contains "full"): run_all_tests.sh --rust --e2e --resilience here;
#   4. KIND=merge and everything green: fast-forwards origin/main to the tested commit.
# A docs-only change runs the audits only. The verdict goes into the ticket (status, result.md)
# and, if NTFY_TOPIC is set in /srv/ci/env, to a phone push. Nothing here resumes a session:
# the user does that with scripts/ci/ci-next.sh.
#
# Installed by scripts/ci/install.sh into /srv/ci/bin, so a ticket that changes these very
# scripts is tested by the previous version of them.
set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CI_ROOT="${CI_ROOT:-/srv/ci}"
CI_BIN="${CI_BIN:-$(dirname "$HERE")}"                 # holds docker_host_lock.sh and lib/
CI_REPO="${CI_REPO:-/srv/docker/openadr_lab}"
# The audits are stdlib python, but audit_step_definitions dry-runs behave: install.sh builds a venv.
PY="$CI_ROOT/venv/bin/python3"; [ -x "$PY" ] || PY=python3
export BOARD_DIR="${BOARD_DIR:-$CI_ROOT/board}" BOARD_NO_MAIN=1
# shellcheck source=board.sh
source "$HERE/board.sh"
[ -f "$CI_ROOT/env" ] && source "$CI_ROOT/env"           # NTFY_TOPIC=...

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
        LOCK_OWNER=ci-runner LOCK_HOST=localhost bash "$CI_BIN/docker_host_lock.sh" acquire -l 150 -m "ci: $1" && return 0
        [ $? -eq 2 ] || return 1
    done
    return 1
}
release_lock() { LOCK_OWNER=ci-runner LOCK_HOST=localhost bash "$CI_BIN/docker_host_lock.sh" release || true; }

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

quick_tier() { # $1 = docs-only (yes|no)
    local s
    for s in audit_file_sizes audit_duplication audit_debt_gate audit_ven_architecture audit_step_definitions; do
        run_step "$s" "$PY" "scripts/$s.py" >>"$TICKET_DIR/quick.log" 2>&1 || return 1
    done
    [ "$1" = yes ] && return 0
    # The harness copies VEN/ and lab-core/ into its image, so the image is rebuilt from this commit;
    # the cargo registry and target are volumes, so a run compiles only what changed.
    run_step "unit-test image" $UNIT_TEST build ven-unit-test >>"$TICKET_DIR/quick.log" 2>&1 || return 1
    run_step "cargo fmt" $UNIT_TEST run --rm ven-unit-test cargo fmt --check >>"$TICKET_DIR/quick.log" 2>&1 || return 1
    run_step "cargo clippy" $UNIT_TEST run --rm ven-unit-test cargo clippy --locked --all-targets --all-features -- -D warnings >>"$TICKET_DIR/quick.log" 2>&1 || return 1
    run_step "cargo test" $UNIT_TEST run --rm ven-unit-test cargo test --locked --jobs 2 >>"$TICKET_DIR/quick.log" 2>&1 || return 1
    run_step "cargo audit (VEN)" $UNIT_TEST run --rm ven-unit-test cargo audit >>"$TICKET_DIR/quick.log" 2>&1 || return 1
    run_step "cargo audit (BFF)" $UNIT_TEST run --rm -v "$CI_REPO/VTN/bff/Cargo.lock:/bff/Cargo.lock:ro" ven-unit-test cargo audit -f /bff/Cargo.lock >>"$TICKET_DIR/quick.log" 2>&1 || return 1
}

full_tier() {
    run_step "run_all_tests.sh --rust --e2e --resilience" \
        env -u DOCKER_HOST OPENADR_LAB_HOST=localhost LOCK_HELD_BY_CALLER=1 \
        bash run_all_tests.sh --rust --e2e --resilience >"$TICKET_DIR/full.log" 2>&1
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
        echo "- tiers: $TIERS   kind: $KIND   docs-only: ${DOCS_ONLY:-?}   attempt: $(cat "$TICKET_DIR/attempt" 2>/dev/null)"
        echo "- finished: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
        [ "$VERDICT" = merged ] && echo "- origin/main is now at: $TESTED_SHA (the rebased commit; the branch's own sha differs if main moved)"
        echo
        echo "| step | exit | seconds |"
        echo "|---|---|---|"
        for s in "${STEPS[@]:-}"; do
            [ -n "$s" ] || continue
            IFS='|' read -r name rc secs <<<"$s"; echo "| $name | $rc | $secs |"
        done
        if [ -n "$FAILED_STEP" ]; then
            echo; echo "## Failed: $FAILED_STEP"; echo
            if [ -s "$TICKET_DIR/full.log" ] && grep -q "Failing scenarios" "$TICKET_DIR/full.log" 2>/dev/null; then
                echo '```'; grep -A40 "Failing scenarios" "$TICKET_DIR/full.log" | head -50; echo '```'
            else
                echo '```'; tail -40 "$TICKET_DIR/quick.log" "$TICKET_DIR/full.log" 2>/dev/null | cut -c1-240; echo '```'
            fi
        fi
        echo; echo "Logs: $TICKET_DIR/{run,quick,full}.log on Node3"
    } > "$TICKET_DIR/result.md.tmp" && mv "$TICKET_DIR/result.md.tmp" "$TICKET_DIR/result.md"
}

notify() { # $1 = ticket
    [ -n "${NTFY_TOPIC:-}" ] || return 0
    curl -fsS -m 10 -H "Title: CI $VERDICT: $BRANCH" -d "$1 — $VERDICT${NOTE:+ ($NOTE)}. Resume with: ci-next" \
        "https://ntfy.sh/$NTFY_TOPIC" >/dev/null 2>&1 || true
}

process_ticket() {
    local t="$1" attempt rc
    TICKET_DIR="$BOARD_DIR/$t"; STEPS=(); FAILED_STEP=""; VERDICT=""; NOTE=""; TESTED_SHA=""; DOCS_ONLY=""
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
            checkout_rebased "$SHA"; rc=$?
            if [ $rc -eq 3 ]; then VERDICT=conflict
            elif [ $rc -ne 0 ]; then VERDICT=error
            else
                TESTED_SHA="$(git -C "$CI_REPO" rev-parse HEAD)"
                if git -C "$CI_REPO" diff --name-only "origin/main..$TESTED_SHA" | board_docs_only; then DOCS_ONLY=yes; else DOCS_ONLY=no; fi
                if ! quick_tier "$DOCS_ONLY"; then VERDICT=red
                elif [ "$DOCS_ONLY" = no ] && [[ ",$TIERS," == *,full,* ]] && ! full_tier; then VERDICT=red
                elif [ "$KIND" != merge ] || { [ "$DOCS_ONLY" = no ] && [[ ",$TIERS," != *,full,* ]]; }; then
                    VERDICT=green; NOTE="tested, not merged (kind $KIND, tiers $TIERS)"
                else
                    push_main "$TESTED_SHA"; rc=$?
                    if [ $rc -eq 0 ]; then VERDICT=merged
                    elif [ $rc -eq 9 ]; then VERDICT=requeue; NOTE="origin/main moved while testing"
                    else VERDICT=error; NOTE="push to origin/main failed (exit $rc)"; fi
                fi
            fi
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
    if [ "$VERDICT" = requeue ]; then board_set_status "$t" queued; else board_set_status "$t" "$VERDICT"; notify "$t"; fi
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
