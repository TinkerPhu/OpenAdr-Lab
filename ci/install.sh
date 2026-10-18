#!/usr/bin/env bash
# install.sh — install or update the CI runner on this machine (Node3).
#
# Run from a checkout of main on the CI host:  bash ci/install.sh
# Copies the runner and what it needs into /srv/ci/bin (so a ticket that changes these scripts
# is tested by the previous version), installs the systemd timer that starts the runner every
# minute, warms the quick tier's unit-test image, and prepares the key the runner pushes to
# origin/main with. Safe to run again.
#
# Layout:  /srv/ci/board/     tickets (board.sh)        /srv/ci/archive/   old tickets
#          /srv/ci/bin/ci/    a copy of the repository's ci/ (runner, board, locks/, audits/, web/)
#          /srv/ci/deploy_key push key (write access to this repository only)
#          ~/.ssh/ci_deploy_ed25519 + Host Node1/Node2 entries: the key deploy tickets ship with
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CI_ROOT="${CI_ROOT:-/srv/ci}"
CI_REPO="${CI_REPO:-/srv/docker/openadr_lab}"
GITHUB_REPO="${GITHUB_REPO:-git@github.com:TinkerPhu/OpenAdr-Lab.git}"
ME="$(id -un)"

sudo install -d -o "$ME" -g "$(id -gn)" "$CI_ROOT" "$CI_ROOT/board" "$CI_ROOT/archive" "$CI_ROOT/bin"
# The installed tree mirrors the repository's ci/ directory (runner.sh finds locks/ beside itself).
# Written to a sibling and swapped in, so a runner or board started meanwhile sees a whole tree.
rm -rf "$CI_ROOT/bin/ci.new" && mkdir -p "$CI_ROOT/bin/ci.new"
cp -r "$ROOT/ci/." "$CI_ROOT/bin/ci.new/"
find "$CI_ROOT/bin/ci.new" -name __pycache__ -prune -exec rm -rf {} +
# A leftover ci.old that cannot be removed (root-owned from an install run under sudo) would make the
# next mv put the new tree INSIDE the old one: stop instead.
rm -rf "$CI_ROOT/bin/ci.old"
[ ! -e "$CI_ROOT/bin/ci.old" ] || { echo "install: cannot remove $CI_ROOT/bin/ci.old (owned by root? run install.sh as $ME, not with sudo)" >&2; exit 1; }
[ ! -d "$CI_ROOT/bin/ci" ] || mv "$CI_ROOT/bin/ci" "$CI_ROOT/bin/ci.old"
mv "$CI_ROOT/bin/ci.new" "$CI_ROOT/bin/ci"

for unit in ci-runner.service ci-web.service; do
    sed "s/@USER@/$ME/" "$ROOT/ci/systemd/$unit" | sudo tee "/etc/systemd/system/$unit" >/dev/null
done
sudo install -m 0644 "$ROOT/ci/systemd/ci-runner.timer" /etc/systemd/system/ci-runner.timer
# The board's clear buttons need a token; generated once, kept in a root-readable-by-us file.
if [ ! -f "$CI_ROOT/web.env" ]; then
    ( umask 077; printf 'CI_WEB_TOKEN=%s
' "$(head -c 24 /dev/urandom | base64 | tr -d '/+=')" > "$CI_ROOT/web.env" )
    echo "board token written to $CI_ROOT/web.env (ssh in and read it once to use the clear buttons)"
fi
# Lanes (ci/lanes.sh): the full tier also runs on these hosts when no session holds them. Written once,
# never overwritten; with CI_LANE_HOSTS empty the runner uses this host alone.
if [ ! -f "$CI_ROOT/runner.env" ]; then
    printf '%s
' '# How this host is named in tickets and lanes (its ssh alias elsewhere).' '#CI_HOME_NAME=Node3'         '# Extra hosts for the full tier, in priority order; each needs passwordless ssh as this user.'         '#CI_LANE_HOSTS="Node2 Node1"' > "$CI_ROOT/runner.env"
fi
LANES="$(sed -n 's/^CI_LANE_HOSTS=//p' "$CI_ROOT/runner.env" | tr -d '"')"
for h in $LANES; do
    ssh -o BatchMode=yes -o ConnectTimeout=8 "$h" true 2>/dev/null || echo "WARNING: lane host $h does not answer over ssh as $ME: the runner will skip it"
done
sudo systemctl daemon-reload
sudo systemctl enable --now ci-runner.timer ci-web.service
sudo systemctl restart ci-web.service

# The step-definition audit dry-runs behave, which the host's python does not have.
[ -x "$CI_ROOT/venv/bin/python3" ] || python3 -m venv "$CI_ROOT/venv"
"$CI_ROOT/venv/bin/pip" install -q -r "$ROOT/tests/requirements.txt"

# The runner pushes only to origin/main, with a key that has write access to this repository
# alone. Fetching stays on the public https URL.
if [ ! -f "$CI_ROOT/deploy_key" ]; then
    ssh-keygen -q -t ed25519 -N "" -C "node3-ci-runner" -f "$CI_ROOT/deploy_key"
fi
# The checkout is sparse: a new top-level directory does not arrive on a pull unless listed.
git -C "$CI_REPO" sparse-checkout add ci 2>/dev/null || true
git -C "$CI_REPO" config remote.origin.pushurl "$GITHUB_REPO"
git -C "$CI_REPO" config core.sshCommand "ssh -i $CI_ROOT/deploy_key -o IdentitiesOnly=yes -o StrictHostKeyChecking=accept-new"
# Deploy tickets (ci/deploy.sh) ship images to the production hosts over ssh. The runner
# user gets its own key and an ssh alias per target; the key is never in the repository.
NODE1_ADDR="${NODE1_ADDR:-192.168.1.103}"; NODE2_ADDR="${NODE2_ADDR:-192.168.1.104}"
DEPLOY_SSH_KEY="$HOME/.ssh/ci_deploy_ed25519"
mkdir -p "$HOME/.ssh" && chmod 700 "$HOME/.ssh"
[ -f "$DEPLOY_SSH_KEY" ] || ssh-keygen -q -t ed25519 -N "" -C "node3-ci-deploy" -f "$DEPLOY_SSH_KEY"
for pair in "Node1 $NODE1_ADDR" "Node2 $NODE2_ADDR"; do
    set -- $pair
    grep -q "^Host $1\$" "$HOME/.ssh/config" 2>/dev/null || cat >> "$HOME/.ssh/config" <<EOCONF
Host $1
  HostName $2
  User $ME
  IdentityFile $DEPLOY_SSH_KEY
  IdentitiesOnly yes
  StrictHostKeyChecking accept-new
EOCONF
done
chmod 600 "$HOME/.ssh/config"

# The unit-test image carries cargo-audit, which compiles for a while on a Pi: pay for it here
# instead of inside the first ticket.
(cd "$CI_REPO" && docker compose -f tests/docker-compose.ven-unit-test.yml build ven-unit-test)

echo
echo "installed. Register this public key as a write-enabled deploy key of the repository"
echo "(GitHub: Settings > Deploy keys > Allow write access) if it is not registered yet:"
cat "$CI_ROOT/deploy_key.pub"
echo
echo "For deploy tickets, add this key to ~/.ssh/authorized_keys of $ME on Node1 and Node2"
echo "(ssh-copy-id -i $DEPLOY_SSH_KEY.pub Node1) if it is not there yet:"
cat "$DEPLOY_SSH_KEY.pub"
