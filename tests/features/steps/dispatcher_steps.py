"""Step definitions for VEN Dispatcher (Stage 4) BDD tests."""

import time
from datetime import datetime, timedelta, timezone
from behave import given, when, then
from features.helpers.api_client import ven_get, ven_post, VEN_BASE_URL
from features.helpers.wait import poll_until


# ---------------------------------------------------------------------------
# When: create packets via POST /packets
# ---------------------------------------------------------------------------

@when("I POST a new EV packet with target_soc {soc:f} to /packets")
def step_post_ev_packet(context, soc):
    """Create a new EV EnergyPacket via POST /packets."""
    latest_end = (datetime.now(timezone.utc) + timedelta(hours=12)).strftime(
        "%Y-%m-%dT%H:%M:%SZ"
    )
    payload = {
        "asset_id": "ev",
        "target_soc": soc,
        "desired_power_kw": 7.0,
        "latest_end": latest_end,
    }
    r = ven_post("/packets", json=payload)
    context.last_response = r
    try:
        context.last_response_json = r.json()
    except Exception:
        context.last_response_json = None


# ---------------------------------------------------------------------------
# When: poll for packet/ledger state
# ---------------------------------------------------------------------------

@when('I poll VEN /packets until asset "{asset_id}" has status "{status}"')
def step_poll_packet_status(context, asset_id, status):
    """Poll GET /packets until a packet for the given asset reaches the expected status."""
    def fetch():
        r = ven_get("/packets")
        r.raise_for_status()
        return r.json()

    def check(packets):
        return any(
            p.get("asset_id") == asset_id and p.get("status") == status
            for p in packets
        )

    context.last_response_json = poll_until(
        fetch, check, timeout=15,
        description=f"packet {asset_id} reaches {status}",
    )


@when('I poll VEN /ledger until field "{field}" is greater than {threshold:f}')
def step_poll_ledger_field(context, field, threshold):
    """Poll GET /ledger until a dotted field exceeds a threshold."""
    def fetch():
        r = ven_get("/ledger")
        r.raise_for_status()
        return r.json()

    def resolve(data, path):
        for part in path.split("."):
            if not isinstance(data, dict):
                return None
            data = data.get(part)
        return data

    def check(data):
        val = resolve(data, field)
        return isinstance(val, (int, float)) and val > threshold

    context.last_response_json = poll_until(
        fetch, check, timeout=15,
        description=f"VEN /ledger field '{field}' > {threshold}",
    )


# ---------------------------------------------------------------------------
# Then: packet status assertions
# ---------------------------------------------------------------------------

@then('at least one packet with asset_id "{asset_id}" has status "{status}"')
def step_packet_has_status(context, asset_id, status):
    """Verify at least one packet for the given asset has the expected status."""
    data = context.last_response_json
    assert isinstance(data, list), f"Expected list of packets, got {type(data)}: {data}"
    matching = [p for p in data if p.get("asset_id") == asset_id]
    assert matching, f"No packets with asset_id='{asset_id}'. All asset_ids: {[p.get('asset_id') for p in data]}"
    statuses = [p.get("status") for p in matching]
    assert any(s == status for s in statuses), (
        f"No packet with asset_id='{asset_id}' has status '{status}'. "
        f"Actual statuses: {statuses}"
    )


@then('the response JSON field "{field_path}" is the string "{expected}"')
def step_response_json_field_is_string(context, field_path, expected):
    """Assert a nested JSON field equals a specific string value."""
    data = context.last_response_json
    assert data is not None, "Response was not JSON"

    def resolve(d, path):
        parts = path.split(".")
        for part in parts:
            if not isinstance(d, dict):
                return None
            d = d.get(part)
        return d

    val = resolve(data, field_path)
    assert val == expected, (
        f"Field '{field_path}' = {val!r}, expected string '{expected}'"
    )


# ---------------------------------------------------------------------------
# Layer 1 — reactive battery correction
# ---------------------------------------------------------------------------

@when("I inject base_load_kw {kw:f} with alpha {alpha:f} via sim inject")
def step_inject_base_load(context, kw, alpha):
    """Inject a persistent base-load offset into the VEN sim."""
    r = ven_post("/sim/inject", json={"base_load_kw": kw, "base_load_alpha": alpha})
    r.raise_for_status()


@then("within {seconds:d} seconds the VEN sim battery power_kw is less than {threshold:f}")
def step_battery_power_below(context, seconds, threshold):
    """Poll /sim until battery.power_kw < threshold, then reset the inject."""
    def fetch():
        r = ven_get("/sim")
        r.raise_for_status()
        return r.json()

    def check(data):
        battery = data.get("assets", {}).get("battery", {})
        power = battery.get("power_kw")
        return isinstance(power, (int, float)) and power < threshold

    try:
        poll_until(
            fetch, check, timeout=seconds,
            description=f"battery.power_kw < {threshold}",
        )
    finally:
        # Reset inject so subsequent scenarios start clean
        try:
            ven_post("/sim/inject", json={"base_load_kw": 0.0, "base_load_alpha": None})
        except Exception:
            pass


# ---------------------------------------------------------------------------
# Replan trigger polling
# ---------------------------------------------------------------------------

@when('I poll VEN trace until a PlanCycle with trigger "{trigger}" appears')
def step_poll_trace_for_trigger(context, trigger):
    """Poll GET /trace/events until a PlanCycle event with the given trigger_reason appears."""
    def fetch():
        r = ven_get("/trace/events")
        r.raise_for_status()
        return r.json()

    def check(events):
        return any(
            e.get("type") == "PlanCycle" and e.get("trigger_reason") == trigger
            for e in events
        )

    try:
        context.trace_events = poll_until(
            fetch, check, timeout=90,
            description=f"PlanCycle with trigger_reason={trigger!r}",
        )
    finally:
        try:
            ven_post("/sim/inject", json={"base_load_kw": 0.0, "base_load_alpha": None})
        except Exception:
            pass


@then('a PlanCycle with trigger "{trigger}" was found in the trace')
def step_assert_trigger_found(context, trigger):
    events = getattr(context, "trace_events", [])
    assert any(
        e.get("type") == "PlanCycle" and e.get("trigger_reason") == trigger
        for e in events
    ), (
        f"No PlanCycle with trigger_reason={trigger!r}. "
        f"Events: {[e.get('trigger_reason') for e in events if e.get('type') == 'PlanCycle']}"
    )
