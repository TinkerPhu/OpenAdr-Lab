"""Step definitions for VEN User Request Manager (Stage 5) BDD tests."""

from datetime import datetime, timedelta, timezone
from behave import when, then
from features.helpers.api_client import ven_get, ven_post, ven_delete


# ---------------------------------------------------------------------------
# When: create user requests
# ---------------------------------------------------------------------------

@when('I POST a user request for asset "{asset_id}" with target_soc {soc:f} and latest_end in {hours:d} hours')
def step_post_user_request(context, asset_id, soc, hours):
    latest_end = (datetime.now(timezone.utc) + timedelta(hours=hours)).strftime(
        "%Y-%m-%dT%H:%M:%SZ"
    )
    payload = {
        "asset_id": asset_id,
        "target_soc": soc,
        "deadlines": [
            {
                "latest_end": latest_end,
                "min_completion": 0.8,
            }
        ],
        "completion_policy": "STOP",
    }
    r = ven_post("/requests", json=payload)
    context.last_response = r
    try:
        context.last_response_json = r.json()
        context.last_created_request = r.json()
    except Exception:
        context.last_response_json = None
        context.last_created_request = None


@when('I POST a user request for asset "{asset_id}" with target_soc {soc:f} and max_cost {cost:f} EUR')
def step_post_user_request_with_budget(context, asset_id, soc, cost):
    latest_end = (datetime.now(timezone.utc) + timedelta(hours=12)).strftime(
        "%Y-%m-%dT%H:%M:%SZ"
    )
    payload = {
        "asset_id": asset_id,
        "target_soc": soc,
        "deadlines": [
            {
                "latest_end": latest_end,
                "max_total_cost_eur": cost,
                "min_completion": 0.8,
            }
        ],
        "completion_policy": "STOP",
    }
    r = ven_post("/requests", json=payload)
    context.last_response = r
    try:
        context.last_response_json = r.json()
        context.last_created_request = r.json()
    except Exception:
        context.last_response_json = None
        context.last_created_request = None


@when('I POST a multi-tier user request for asset "{asset_id}"')
def step_post_multi_tier_request(context, asset_id):
    """Two deadline tiers: cheap (tonight) then fallback (tomorrow)."""
    tier1 = (datetime.now(timezone.utc) + timedelta(hours=8)).strftime("%Y-%m-%dT%H:%M:%SZ")
    tier2 = (datetime.now(timezone.utc) + timedelta(hours=24)).strftime("%Y-%m-%dT%H:%M:%SZ")
    payload = {
        "asset_id": asset_id,
        "target_soc": 0.80,
        "deadlines": [
            {"latest_end": tier1, "max_total_cost_eur": 5.0, "min_completion": 0.8},
            {"latest_end": tier2, "max_total_cost_eur": 1.0, "min_completion": 0.5},
        ],
        "completion_policy": "STOP",
    }
    r = ven_post("/requests", json=payload)
    context.last_response = r
    try:
        context.last_response_json = r.json()
        context.last_created_request = r.json()
    except Exception:
        context.last_response_json = None
        context.last_created_request = None


@when("I save the request ID")
def step_save_request_id(context):
    req = getattr(context, "last_created_request", None)
    assert req is not None, "No user request in context to save"
    context.saved_request_id = req.get("id")
    context.saved_packet_id = req.get("packet_id")
    assert context.saved_request_id, f"Request has no 'id' field: {req}"


@when("I DELETE the saved user request")
def step_delete_user_request(context):
    req_id = context.saved_request_id
    assert req_id, "No saved_request_id in context"
    r = ven_delete(f"/requests/{req_id}")
    context.last_response = r
    context.last_response_json = None


# ---------------------------------------------------------------------------
# Then: assertions on /requests and cancellation
# ---------------------------------------------------------------------------

@then("the requests list has at least {count:d} item")
@then("the requests list has at least {count:d} items")
def step_requests_at_least(context, count):
    data = context.last_response_json
    assert isinstance(data, list), f"Expected list, got {type(data)}: {data}"
    assert len(data) >= count, f"Expected >= {count} requests, got {len(data)}"


@then("the cancelled packet is in ABANDONED status")
def step_cancelled_packet_abandoned(context):
    """After DELETE /requests/:id, GET /packets and verify the packet is ABANDONED."""
    packet_id = getattr(context, "saved_packet_id", None)
    assert packet_id, "No saved_packet_id in context — did 'I save the request ID' run?"

    r = ven_get("/packets")
    r.raise_for_status()
    packets = r.json()
    assert isinstance(packets, list), f"Expected list of packets, got {type(packets)}"

    matched = [p for p in packets if p.get("id") == packet_id]
    assert matched, (
        f"Packet {packet_id} not found in /packets. "
        f"IDs: {[p.get('id') for p in packets]}"
    )
    status = matched[0].get("status")
    assert status == "ABANDONED", (
        f"Expected packet {packet_id} to be ABANDONED, got '{status}'"
    )
