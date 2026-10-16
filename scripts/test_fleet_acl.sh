#!/usr/bin/env bash
# Does the broker's ACL actually stop a VEN speaking for another VEN?
#
# The ACL is the only thing standing between "every VEN has its own password"
# and "that fact means something". A config that authenticates everyone and
# authorises everyone looks identical from the outside until someone spoofs a
# reading, so this asks the broker directly.
#
# It asks by DELIVERY, not by exit code, because neither client tells the truth
# about authorisation:
#   - a denied publish is still PUBACKed and then silently dropped, so
#     `mosquitto_pub` exits 0 whether or not the message went anywhere;
#   - a wildcard subscription is never refused at SUBACK — mosquitto accepts it
#     and filters each message at delivery time, so `mosquitto_sub` on
#     `openadr-lab/fleet/#` "succeeds" for a VEN that can read only its own row.
# Measuring either exit code reads a working ACL as a broken one, and an absent
# password as a working ACL. The only honest question is whether the message
# arrived.
#
# Usage:  bash scripts/test_fleet_acl.sh [Node1]
#
# It needs no root secret. A VEN's password is read from the file that VEN
# itself starts with (VEN/.env.fleet.ven-N on the host, written by
# gen_fleet_mqtt_secrets.py --per-ven-dir), which is the password the broker
# holds for it. The root secret is deliberately kept off the hosts and is only
# needed to generate or rotate passwords, never to verify them.
set -uo pipefail
HOST="${1:-Node1}"
BROKER="${BROKER_CONTAINER:-vtn-lab-mqtt-1}"
VEN_DIR="${VEN_DIR:-/srv/docker/openadr_lab/VEN}"

# One VEN's broker password, from its own env file on $HOST. An absent file or
# an empty value is a setup problem, not an ACL result: say so and stop rather
# than let every check below fail on a blank password.
ven_password() { # ven-name
  local pw
  pw=$(ssh "$HOST" "sed -n 's/^FLEET_MQTT_PASSWORD=//p' '$VEN_DIR/.env.fleet.$1'" 2>/dev/null)
  if [[ -z "$pw" ]]; then
    echo "No FLEET_MQTT_PASSWORD for $1 in $VEN_DIR/.env.fleet.$1 on $HOST." >&2
    echo "Generate it with scripts/gen_fleet_mqtt_secrets.py --per-ven-dir (needs the root secret)." >&2
    exit 2
  fi
  printf '%s' "$pw"
}

PW1=$(ven_password ven-1) || exit 2
PW2=$(ven_password ven-2) || exit 2
BFF_U="${MQTT_BFF_USERNAME:-openadr-bff}"
BFF_P="${MQTT_BFF_PASSWORD:-openadr-bff}"
fail=0

# Subscribe as one client, publish as another, and report whether it landed.
# A distinct topic leaf per probe keeps the retained fleet state out of it.
delivered() { # sub_user sub_pw sub_topic pub_user pub_pw pub_topic
  ssh "$HOST" sh -s <<EOF >/dev/null 2>&1
docker exec "$BROKER" sh -c '
  mosquitto_sub -h 127.0.0.1 -p 1884 -u "$1" -P "$2" -t "$3" -C 1 -W 5 > /tmp/acl_probe 2>/dev/null &
  sleep 1
  mosquitto_pub -h 127.0.0.1 -p 1884 -u "$4" -P "$5" -t "$6" -m acl-probe -q 1 >/dev/null 2>&1
  wait
  [ -s /tmp/acl_probe ]
'
EOF
}

check() { # description expected(arrives|blocked) sub_user sub_pw sub_topic pub_user pub_pw pub_topic
  local what="$1" expect="$2"; shift 2
  if delivered "$@"; then got=arrives; else got=blocked; fi
  if [[ "$got" == "$expect" ]]; then
    echo "  OK   $what ($got)"
  else
    echo "  FAIL $what: expected $expect, got $got"; fail=1
  fi
}

echo "=== fleet ACL on $HOST ==="

# The control: without this passing, every 'blocked' below could just be a
# broken password or an unreachable broker rather than the ACL doing its job.
check "ven-1 publishes under its own name, BFF sees it" arrives \
  "$BFF_U" "$BFF_P" "openadr-lab/fleet/ven-1/acl-probe" \
  ven-1 "$PW1" "openadr-lab/fleet/ven-1/acl-probe"

# The point of the whole file: a VEN cannot invent a reading for its neighbour.
check "ven-1 publishes as ven-2" blocked \
  "$BFF_U" "$BFF_P" "openadr-lab/fleet/ven-2/acl-probe" \
  ven-1 "$PW1" "openadr-lab/fleet/ven-2/acl-probe"

# Nor read the fleet, which is the BFF's job. The subscription is accepted;
# what must not happen is the message arriving.
# Subscribing to the exact probe leaf rather than `openadr-lab/fleet/#` on
# purpose: under the wildcard the broker delivers ven-1's own RETAINED status
# first, `-C 1` returns on it, and a working ACL reads as a broken one.
check "ven-1 reads ven-2's topic" blocked \
  ven-1 "$PW1" "openadr-lab/fleet/ven-2/acl-probe" \
  ven-2 "$PW2" "openadr-lab/fleet/ven-2/acl-probe"

# The BFF is a consumer of what the VENs say; a bug there must not be able to
# invent a reading either.
check "BFF publishes into the fleet" blocked \
  ven-1 "$PW1" "openadr-lab/fleet/ven-1/acl-probe" \
  "$BFF_U" "$BFF_P" "openadr-lab/fleet/ven-1/acl-probe"

# The objective command is the BFF's one write (fleet objective control): it
# must reach the VEN it names, and nothing else under the fleet root.
check "BFF commands ven-1's objective" arrives   ven-1 "$PW1" "openadr-lab/fleet/ven-1/command/objective"   "$BFF_U" "$BFF_P" "openadr-lab/fleet/ven-1/command/objective"

check "ven-1 commands ven-2's objective" blocked   ven-2 "$PW2" "openadr-lab/fleet/ven-2/command/objective"   ven-1 "$PW1" "openadr-lab/fleet/ven-2/command/objective"

exit $fail
