"""Step definitions for VEN Dispatcher (Stage 4) BDD tests."""

from datetime import datetime, timedelta, timezone
from behave import when, then
from features.helpers.api_client import ven_post, VEN_BASE_URL


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
