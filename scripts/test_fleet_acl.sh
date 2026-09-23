#!/usr/bin/env bash
# Does the broker's ACL actually stop a VEN publishing as another VEN?
#
# The ACL is the only thing standing between "every VEN has its own password"
# and "that fact means something". A config that authenticates everyone and
# authorises everyone looks identical from the outside until someone spoofs a
# reading, so this asks the broker directly.
#
# Usage:  MQTT_FLEET_ROOT_SECRET=... bash scripts/test_fleet_acl.sh [Node1]
set -uo pipefail
HOST="${1:-Node1}"
REPO="/srv/docker/openadr_lab"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

if [[ -z "${MQTT_FLEET_ROOT_SECRET:-}" ]]; then
  echo "MQTT_FLEET_ROOT_SECRET is not set — cannot derive a VEN's password." >&2
  exit 2
fi

PW1=$(python3 "$SCRIPT_DIR/gen_fleet_mqtt_secrets.py" --show ven-1)
fail=0

check() { # description, expected(allow|deny), command...
  local what="$1" expect="$2"; shift 2
  if "$@" >/dev/null 2>&1; then got=allow; else got=deny; fi
  if [[ "$got" == "$expect" ]]; then
    echo "  OK   $what ($got)"
  else
    echo "  FAIL $what: expected $expect, got $got"; fail=1
  fi
}

echo "=== fleet ACL on $HOST ==="

# ven-1 may publish under its own name...
check "ven-1 publishes to its own topic" allow \
  ssh "$HOST" "docker exec vtn-lab-mqtt-1 mosquitto_pub -h 127.0.0.1 -p 1884 \
    -u ven-1 -P '$PW1' -t openadr-lab/fleet/ven-1/telemetry -m '{}'"

# ...and must not publish under anyone else's. This is the whole point.
check "ven-1 publishes as ven-2" deny \
  ssh "$HOST" "docker exec vtn-lab-mqtt-1 mosquitto_pub -h 127.0.0.1 -p 1884 \
    -u ven-1 -P '$PW1' -t openadr-lab/fleet/ven-2/telemetry -m '{}' -q 1"

# ...nor read the whole fleet, which is the BFF's job.
check "ven-1 subscribes to the whole fleet" deny \
  ssh "$HOST" "docker exec vtn-lab-mqtt-1 mosquitto_sub -h 127.0.0.1 -p 1884 \
    -u ven-1 -P '$PW1' -t 'openadr-lab/fleet/#' -C 1 -W 3"

exit $fail
