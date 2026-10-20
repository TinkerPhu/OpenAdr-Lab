#!/usr/bin/env bash
# deploy.sh — build release images of origin/main on Node3 and switch a production host to them.
#
#   deploy.sh <Node1|Node2> <stack dir> <service>...     build, ship, switch over
#   deploy.sh --rollback <Node1|Node2> <stack dir> <service>...   back to the :previous images
#
# Runs on Node3 inside the checkout the runner has put on the commit to deploy (ci/runner.sh,
# ticket KIND=deploy). Node1, Node2 and Node3 share a CPU architecture, so the image built once
# here is shipped (docker save | ssh docker load) and never rebuilt on a host that runs the lab.
# The target keeps the image it ran before as <image>:previous. Compose project names follow the
# directory name (VEN -> ven) on every host, so a shipped image carries the name the target's own
# compose file expects and `up --no-build` finds it.
#
# Nothing but names from the allowlists below ever reaches a shell on the target.
set -euo pipefail

TARGETS="Node1 Node2"
STACKS="VEN VTN VEN/scale_out/node2"
REMOTE_DIR="${REMOTE_DIR:-/srv/docker/openadr_lab}"
DRY="${DEPLOY_DRY_RUN:-}"      # non-empty: print the remote and docker commands, run nothing

die() { echo "deploy: $*" >&2; exit 1; }
run() { if [ -n "$DRY" ]; then echo "+ $*"; else "$@"; fi; }

ROLLBACK=no
[ "${1:-}" = "--rollback" ] && { ROLLBACK=yes; shift; }
[ $# -ge 3 ] || die "usage: deploy.sh [--rollback] <$TARGETS> <stack dir> <service>..."
TARGET="$1"; STACK="$2"; shift 2
SERVICES=("$@")

case " $TARGETS " in *" $TARGET "*) ;; *) die "target '$TARGET' is not one of: $TARGETS" ;; esac
case " $STACKS " in *" $STACK "*) ;; *) die "stack '$STACK' is not one of: $STACKS" ;; esac
for s in "${SERVICES[@]}"; do
    [[ "$s" =~ ^[a-z0-9][a-z0-9-]*$ ]] || die "bad service name '$s'"
done

COMPOSE="docker compose -f $STACK/docker-compose.yml"
LOCK="bash ci/locks/docker_host_lock.sh"

# Production hosts are shared: take the target's lock for the whole switch-over.
export LOCK_HOST="$TARGET" LOCK_OWNER=ci-runner
run $LOCK acquire -l 30 -m "ci deploy: $STACK ${SERVICES[*]}"
trap 'run $LOCK release' EXIT

# The image each named service builds. Compose names it <project>-<service>, the project being the
# stack directory's own name (VEN -> ven, VEN/scale_out/node2 -> node2). `compose config --images`
# is not used: it also lists the images of the services these depend on (bff -> vtn), which are
# neither built here nor meant to change on the target.
PROJECT="$(basename "$STACK" | tr '[:upper:]' '[:lower:]')"
IMAGES=()
for s in "${SERVICES[@]}"; do IMAGES+=("$PROJECT-$s"); done

if [ "$ROLLBACK" = yes ]; then
    for img in "${IMAGES[@]}"; do
        run ssh "$TARGET" "docker image inspect '${img}:previous' >/dev/null && docker tag '${img}:previous' '${img}:latest'"
    done
else
    run $COMPOSE build "${SERVICES[@]}"
    for img in "${IMAGES[@]}"; do
        # Keep what runs now as :previous (a host that never had the image has nothing to keep).
        run ssh "$TARGET" "docker image inspect '${img}:latest' >/dev/null 2>&1 && docker tag '${img}:latest' '${img}:previous' || true"
    done
    if [ -n "$DRY" ]; then echo "+ docker save ${IMAGES[*]} | ssh $TARGET docker load"
    else docker save "${IMAGES[@]}" | ssh "$TARGET" docker load; fi
fi

# Switch over on the target with the compose file it already has: the images are in place.
SWITCH="cd $REMOTE_DIR/$STACK && git -C $REMOTE_DIR fetch -q origin && git -C $REMOTE_DIR checkout -q main && git -C $REMOTE_DIR reset -q --hard origin/main && docker compose up -d --no-build ${SERVICES[*]}"
# nginx caches upstream addresses at start: the VEN UI must re-resolve after a backend is recreated.
[ "$STACK" = VEN ] && [[ " ${SERVICES[*]} " == *" ven-"* ]] && SWITCH="$SWITCH && docker compose restart ui"
run ssh "$TARGET" "$SWITCH"

for s in "${SERVICES[@]}"; do
    run ssh "$TARGET" "cd $REMOTE_DIR/$STACK && docker compose ps --status running --services | grep -qx '$s'" \
        || die "$s is not running on $TARGET after the switch-over: roll back with --rollback"
done
echo "deploy: ${SERVICES[*]} ($STACK) now run on $TARGET"
