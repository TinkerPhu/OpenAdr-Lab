#!/usr/bin/env bash
# install.sh — install or update the CI runner on this machine (Node3).
#
# Run from a checkout of main on the CI host:  bash scripts/ci/install.sh
# Copies the runner and what it needs into /srv/ci/bin (so a ticket that changes these scripts
# is tested by the previous version), installs the systemd timer that starts the runner every
# minute, warms the quick tier's unit-test image, and prepares the key the runner pushes to
# origin/main with. Safe to run again.
#
# Layout:  /srv/ci/board/     tickets (board.sh)        /srv/ci/archive/   old tickets
#          /srv/ci/bin/       ci/{runner,board}.sh, docker_host_lock.sh, lib/
#          /srv/ci/env        optional: NTFY_TOPIC=<topic> for phone pushes
#          /srv/ci/deploy_key push key (write access to this repository only)
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CI_ROOT="${CI_ROOT:-/srv/ci}"
CI_REPO="${CI_REPO:-/srv/docker/openadr_lab}"
GITHUB_REPO="${GITHUB_REPO:-git@github.com:TinkerPhu/OpenAdr-Lab.git}"
ME="$(id -un)"

sudo install -d -o "$ME" -g "$(id -gn)" "$CI_ROOT" "$CI_ROOT/board" "$CI_ROOT/archive" "$CI_ROOT/bin" "$CI_ROOT/bin/ci" "$CI_ROOT/bin/lib"
install -m 0755 "$ROOT/scripts/ci/runner.sh" "$ROOT/scripts/ci/board.sh" "$CI_ROOT/bin/ci/"
install -m 0755 "$ROOT/scripts/docker_host_lock.sh" "$CI_ROOT/bin/"
install -m 0644 "$ROOT/scripts/lib/lease_lock_core.sh" "$ROOT/scripts/lib/lease_lock_cli.sh" "$CI_ROOT/bin/lib/"

sed "s/@USER@/$ME/" "$ROOT/scripts/ci/systemd/ci-runner.service" | sudo tee /etc/systemd/system/ci-runner.service >/dev/null
sudo install -m 0644 "$ROOT/scripts/ci/systemd/ci-runner.timer" /etc/systemd/system/ci-runner.timer
sudo systemctl daemon-reload
sudo systemctl enable --now ci-runner.timer

# The runner pushes only to origin/main, with a key that has write access to this repository
# alone. Fetching stays on the public https URL.
if [ ! -f "$CI_ROOT/deploy_key" ]; then
    ssh-keygen -q -t ed25519 -N "" -C "node3-ci-runner" -f "$CI_ROOT/deploy_key"
fi
git -C "$CI_REPO" config remote.origin.pushurl "$GITHUB_REPO"
git -C "$CI_REPO" config core.sshCommand "ssh -i $CI_ROOT/deploy_key -o IdentitiesOnly=yes -o StrictHostKeyChecking=accept-new"
# The unit-test image carries cargo-audit, which compiles for a while on a Pi: pay for it here
# instead of inside the first ticket.
(cd "$CI_REPO" && docker compose -f tests/docker-compose.ven-unit-test.yml build ven-unit-test)

echo
echo "installed. Register this public key as a write-enabled deploy key of the repository"
echo "(GitHub: Settings > Deploy keys > Allow write access) if it is not registered yet:"
cat "$CI_ROOT/deploy_key.pub"
