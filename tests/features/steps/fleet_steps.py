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


@when("I wait for the fleet history of the last {minutes:d} minutes to have samples")
def step_wait_for_fleet_history(context, minutes):
    """The store writes in batches, so a sample published a moment ago may not
    be queryable yet. Waiting is the honest way to say "eventually"."""
    from datetime import datetime, timedelta, timezone

    def fetch():
        start = datetime.now(timezone.utc) - timedelta(minutes=minutes)
        context.response = bff_get(
            "/api/fleet/power",
            params={"from": start.isoformat(), "stepSeconds": 5},
        )
        if context.response.status_code != 200:
            # 503 while the store is still connecting is a normal early state,
            # not a failure -- keep polling, and let the timeout message carry
            # the status if it never clears.
            return {"status": context.response.status_code,
                    "body": context.response.text[:200]}
        return context.response.json()

    context.fleet_history = poll_until(
        fetch,
        lambda b: len(b.get("fleet") or []) > 0,
        timeout=120,
        interval=5,
        description="stored fleet telemetry",
    )


@then("the fleet history says which resolution it was drawn at")
def step_history_declares_resolution(context):
    body = context.fleet_history
    assert body.get("source") in ("raw", "rollup"), (
        f"the history must say which table answered it, got {body.get('source')!r}"
    )
    assert body.get("stepSeconds"), "the history must state its bucket width"


@then("every history bucket counts the VENs it was summed from")
def step_history_buckets_count_contributors(context):
    for bucket in context.fleet_history["fleet"]:
        n = bucket.get("contributingVens")
        assert isinstance(n, int) and n >= 1, (
            f"bucket {bucket.get('ts')} carries no contributor count ({n!r}) — "
            "a total whose membership is unknown cannot be compared with another"
        )


@when("I wait for the fleet to report a reaction to the saved event")
def step_wait_for_reaction(context):
    """The chain needs three things to line up: the VEN polls the event, it
    publishes the decision, and the BFF writes it. Each is seconds, so this
    waits rather than asserts on the first try."""
    event_id = context.rate_event_id

    def fetch():
        context.response = bff_get("/api/fleet/reactions", params={"eventID": event_id})
        if context.response.status_code != 200:
            return {"status": context.response.status_code,
                    "body": context.response.text[:200]}
        return context.response.json()

    context.reactions = poll_until(
        fetch,
        lambda b: (b.get("vensSeen") or 0) >= 1,
        timeout=120,
        interval=5,
        description=f"a VEN reaction to event {event_id}",
    )


@then("each reacting VEN names the event version it saw")
def step_reaction_names_the_version(context):
    for ven in context.reactions["vens"]:
        assert ven.get("seenAt"), f"{ven.get('venName')} reports no time it saw the event"
        assert ven.get("modificationDateTime"), (
            f"{ven.get('venName')} did not say which version of the event it acted on — "
            "an edited event keeps its id, so without this the trace is ambiguous"
        )

