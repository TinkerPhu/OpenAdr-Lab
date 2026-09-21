"""Steps for the fleet telemetry feed (fleet-monitor phase 0 §6/§7)."""

from behave import then, when
from features.helpers.api_client import bff_get
from features.helpers.wait import poll_until


@when("I GET BFF health")
def step_get_bff_health(context):
    context.response = bff_get("/api/health")


@then("the BFF health shows the fleet feed connected")
def step_bff_health_fleet_connected(context):
    body = context.response.json()
    fleet = body.get("fleet")
    assert fleet is not None, f"/api/health does not report the fleet feed: {body}"
    assert fleet.get("enabled"), (
        "the test stack configures FLEET_MQTT_HOST, so the feed must be enabled; "
        f"got {fleet}"
    )
    assert fleet.get("connected"), (
        f"the fleet feed is enabled but not connected to the broker: {fleet}"
    )


@when("I wait for the fleet feed to have at least {count:d} VEN")
def step_wait_for_fleet_vens(context, count):
    """Telemetry arrives on the VEN's tick cadence, so the first message may
    be a few seconds out. Waiting beats asserting against an empty feed and
    calling it a pass."""

    def fetch():
        context.response = bff_get("/api/fleet/power")
        return context.response.json()

    context.fleet = poll_until(
        fetch,
        lambda b: len(b.get("vens") or []) >= count,
        timeout=90,
        interval=3,
        description=f"at least {count} VEN on the fleet feed",
    )


@then("every listed VEN either reports a power value or none at all")
def step_ven_power_is_value_or_absent(context):
    for ven in context.fleet["vens"]:
        value = ven.get("netPowerW")
        assert value is None or isinstance(value, (int, float)), (
            f"{ven.get('venName')} reports a non-numeric power {value!r} — a VEN "
            "that has not spoken must be null, never a stand-in number"
        )


@then("the fleet sum equals the sum of the reporting VENs")
def step_fleet_sum_is_consistent(context):
    reporting = [
        v["netPowerW"] for v in context.fleet["vens"] if v.get("netPowerW") is not None
    ]
    fleet = context.fleet["fleet"]
    assert fleet["contributingVens"] == len(reporting), (
        f"the sum claims {fleet['contributingVens']} contributors but "
        f"{len(reporting)} VENs reported a value"
    )
    expected = sum(reporting)
    assert abs(fleet["netPowerW"] - expected) < 1e-6, (
        f"fleet sum {fleet['netPowerW']} != {expected} from its own VEN list"
    )
