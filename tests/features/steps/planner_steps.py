"""Step definitions for VEN Planner (Stage 3) BDD tests."""

import uuid
from datetime import datetime, timedelta, timezone
from behave import given, when, then
from features.helpers.api_client import ven_get, vtn_post
from features.helpers.wait import poll_until


# ---------------------------------------------------------------------------
# When: poll VEN planner endpoints
# ---------------------------------------------------------------------------

@when("I wait for the VEN /plan endpoint to return a plan")
def step_wait_for_plan(context):
    def fetch():
        resp = ven_get("/plan")
        if not resp.ok:
            return None
        body = resp.json()
        if not isinstance(body, dict):
            return None
        return body

    context.ven_plan = poll_until(
        fetch,
        lambda plan: plan is not None and "id" in plan,
        timeout=60,
        description="VEN /plan returns a non-null plan",
    )


@when("I wait for the VEN /plan to be recomputed after the sim inject")
def step_wait_for_fresh_plan(context):
    """Wait for a plan whose created_at is strictly after the current time.

    Use this instead of the generic wait step whenever a sim inject
    precedes the assertion — the inject changes sim state but does not
    trigger a replan, so the existing plan may reflect a different state.
    Waiting for a fresh created_at ensures the MILP ran with the injected
    value as its starting point.
    """
    cutoff = datetime.now(timezone.utc)

    def fetch():
        resp = ven_get("/plan")
        if not resp.ok:
            return None
        body = resp.json()
        if not isinstance(body, dict):
            return None
        return body

    def is_fresh(plan):
        if plan is None or "id" not in plan:
            return False
        raw = plan.get("created_at", "")
        if not raw:
            return False
        try:
            plan_ts = datetime.fromisoformat(raw.replace("Z", "+00:00"))
            return plan_ts > cutoff
        except ValueError:
            return False

    context.ven_plan = poll_until(
        fetch, is_fresh,
        timeout=300,
        description="VEN /plan recomputed after sim inject",
    )


@when("I wait for the VEN /plan to have an EV allocation in slots")
def step_wait_for_ev_allocation(context):
    def fetch():
        resp = ven_get("/plan")
        if not resp.ok:
            return None
        body = resp.json()
        if not isinstance(body, dict):
            return None
        return body

    def has_ev_alloc(plan):
        if plan is None:
            return False
        slots = plan.get("slots", [])
        return any(
            any(a.get("asset_id") == "ev" for a in slot.get("allocations", []))
            for slot in slots
        )

    context.ven_plan = poll_until(
        fetch,
        has_ev_alloc,
        timeout=180,
        interval=5,
        description="VEN /plan has EV allocation in slots",
    )


@when("I wait for the VEN /plan to have a heater allocation in slots")
def step_wait_for_heater_allocation(context):
    def fetch():
        resp = ven_get("/plan")
        if not resp.ok:
            return None
        body = resp.json()
        if not isinstance(body, dict):
            return None
        return body

    def has_heater_alloc(plan):
        if plan is None:
            return False
        slots = plan.get("slots", [])
        return any(
            any(a.get("asset_id") == "heater" for a in slot.get("allocations", []))
            for slot in slots
        )

    context.ven_plan = poll_until(
        fetch,
        has_heater_alloc,
        timeout=180,
        interval=5,
        description="VEN /plan has heater allocation in slots",
    )


@when("I wait for the VEN /plan to have envelopes")
def step_wait_for_envelopes(context):
    def fetch():
        resp = ven_get("/plan")
        if not resp.ok:
            return None
        body = resp.json()
        if not isinstance(body, dict):
            return None
        return body

    context.ven_plan = poll_until(
        fetch,
        lambda plan: plan is not None and len(plan.get("envelopes", [])) > 0,
        timeout=180,
        interval=5,
        description="VEN /plan has at least 1 flexibility envelope",
    )


# ---------------------------------------------------------------------------
# Given: VTN cheap PRICE event
# ---------------------------------------------------------------------------

@given("I create a cheap 4-hour PRICE event for the saved program")
def step_create_cheap_4h_price(context):
    now = datetime.now(timezone.utc)
    intervals = []
    for i in range(4):
        start = now + timedelta(hours=i)
        intervals.append({
            "id": i,
            "intervalPeriod": {
                "start": start.strftime("%Y-%m-%dT%H:%M:%SZ"),
                "duration": "PT1H",
            },
            "payloads": [{"type": "PRICE", "values": [0.05]}],
        })

    r = vtn_post(
        "/events",
        context.vtn_token,
        json={
            "programID": context.saved_program_id,
            "eventName": f"planner-cheap-{uuid.uuid4().hex[:8]}",
            "priority": 1,
            "intervals": intervals,
        },
    )
    r.raise_for_status()
    context.planner_event_id = r.json().get("id")


# ---------------------------------------------------------------------------
# Then: assertions on /packets and /plan
# ---------------------------------------------------------------------------

@then("the packets list has at least {count:d} item")
@then("the packets list has at least {count:d} items")
def step_packets_at_least(context, count):
    data = context.last_response_json
    assert isinstance(data, list), f"Expected list, got {type(data)}: {data}"
    assert len(data) >= count, f"Expected >= {count} packets, got {len(data)}"


@then('at least one packet has asset_id "{asset_id}"')
def step_packet_has_asset(context, asset_id):
    data = context.last_response_json
    assert any(p.get("asset_id") == asset_id for p in data), (
        f"No packet with asset_id '{asset_id}'. Packets: {[p.get('asset_id') for p in data]}"
    )


@then('the plan has field "{field}"')
def step_plan_has_field(context, field):
    plan = context.ven_plan
    assert field in plan, f"Plan missing field '{field}'. Keys: {list(plan.keys())}"


@then("the plan.slots is a non-empty array")
def step_plan_slots_nonempty(context):
    plan = context.ven_plan
    slots = plan.get("slots", [])
    assert isinstance(slots, list) and len(slots) > 0, (
        f"plan slots is empty or not a list: {slots}"
    )


@then("the plan.envelopes is a non-empty array")
def step_plan_envelopes_nonempty(context):
    plan = context.ven_plan
    envs = plan.get("envelopes", [])
    assert isinstance(envs, list) and len(envs) > 0, (
        f"envelopes is empty or not a list. Full plan keys: {list(plan.keys())}"
    )


@then('at least one firm slot has an allocation for asset "{asset_id}"')
def step_firm_slot_has_allocation(context, asset_id):
    plan = context.ven_plan
    slots = plan.get("slots", [])
    found = any(
        any(a.get("asset_id") == asset_id for a in slot.get("allocations", []))
        for slot in slots
    )
    assert found, (
        f"No slot has an allocation for asset '{asset_id}'. "
        f"Checked {len(slots)} slots."
    )

@then('the plan contains a packet with asset_id"{asset_id}" in a non-terminal status')
def step_plan_packet_non_terminal(context, asset_id):
    plan = context.ven_plan
    terminal = {"COMPLETED", "PARTIAL_COMPLETED", "ABANDONED", "FAILED"}
    packets = plan.get("packets", [])
    found = any(
        p.get("asset_id") == asset_id and p.get("status") not in terminal
        for p in packets
    )
    assert found, (
        f"No non-terminal packet with asset_id '{asset_id}'. "
        f"Packets: {[(p.get('asset_id'), p.get('status')) for p in packets]}"
    )


@then("the plan contains at least {count:d} packet")
@then("the plan contains at least {count:d} packets")
def step_plan_has_packets(context, count):
    plan = context.ven_plan
    packets = plan.get("packets", [])
    assert len(packets) >= count, f"Expected >= {count} packets in plan, got {len(packets)}"
