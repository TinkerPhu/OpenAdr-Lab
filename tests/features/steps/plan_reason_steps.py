"""Step definitions for VEN Planner PlanReason audit trail (Phase D CP3)."""

import uuid
from datetime import datetime, timezone, timedelta
from behave import given, when, then
from features.helpers.api_client import ven_get, ven_post, vtn_post
from features.helpers.wait import poll_until


# ── When steps ────────────────────────────────────────────────────────────────

@when('I wait for the VEN /plan to have steps for asset "{asset_id}"')
def step_wait_for_steps(context, asset_id):
    def fetch():
        resp = ven_get("/plan")
        if not resp.ok:
            return None
        body = resp.json()
        if not isinstance(body, dict):
            return None
        steps = body.get("steps", [])
        if not any(s.get("asset_id") == asset_id for s in steps):
            return None
        return body

    context.ven_plan = poll_until(
        fetch,
        lambda plan: plan is not None,
        timeout=60,
        description=f"VEN /plan has steps for asset '{asset_id}'",
    )
    context.last_checked_asset = asset_id


# ── Given steps ───────────────────────────────────────────────────────────────

@given("I create a 4-hour PRICE event at {price:f} EUR/kWh for the saved program")
def step_create_price_event(context, price):
    now = datetime.now(timezone.utc)
    intervals = [
        {
            "id": i,
            "intervalPeriod": {
                "start": (now + timedelta(hours=i)).strftime("%Y-%m-%dT%H:%M:%SZ"),
                "duration": "PT1H",
            },
            "payloads": [{"type": "PRICE", "values": [price]}],
        }
        for i in range(4)
    ]
    r = vtn_post("/events", context.vtn_token, json={
        "programID": context.saved_program_id,
        "eventName": f"reason-test-{uuid.uuid4().hex[:8]}",
        "priority": 1,
        "intervals": intervals,
    })
    r.raise_for_status()
    context.planner_event_id = r.json().get("id")


@given(
    "I POST an EV packet with target_soc {soc:f}, desired_power_kw {power:f},"
    " and latest_end_h {hours:f}"
)
def step_post_ev_packet(context, soc, power, hours):
    latest_end = (datetime.now(timezone.utc) + timedelta(hours=hours)).strftime(
        "%Y-%m-%dT%H:%M:%SZ"
    )
    r = ven_post("/packets", json={
        "asset_id": "ev",
        "target_soc": soc,
        "desired_power_kw": power,
        "latest_end": latest_end,
    })
    r.raise_for_status()
    context.posted_packet_id = r.json().get("id")


# ── Then steps ────────────────────────────────────────────────────────────────

@then('at least one PlanStep for asset "{asset_id}" has reason kind "{kind}"')
def step_plan_step_has_reason_kind(context, asset_id, kind):
    plan = context.ven_plan
    steps = [s for s in plan.get("steps", []) if s.get("asset_id") == asset_id]
    assert steps, f"No steps found for asset '{asset_id}'"
    matching = [s for s in steps if s.get("reason", {}).get("kind") == kind]
    assert matching, (
        f"No step for '{asset_id}' with reason.kind='{kind}'. "
        f"Actual kinds: {[s.get('reason', {}).get('kind') for s in steps[:5]]}"
    )
    context.last_matching_steps = matching


@then("that PlanStep has setpoint_kw greater than 0.0")
def step_setpoint_positive(context):
    steps = context.last_matching_steps
    found = any(s.get("setpoint_kw", 0.0) > 1e-6 for s in steps)
    assert found, (
        f"No matching step has setpoint_kw > 0. "
        f"Values: {[s.get('setpoint_kw') for s in steps]}"
    )


@then("that PlanStep has setpoint_kw less than 0.0")
def step_setpoint_negative(context):
    steps = context.last_matching_steps
    found = any(s.get("setpoint_kw", 0.0) < -1e-6 for s in steps)
    assert found, (
        f"No matching step has setpoint_kw < 0. "
        f"Values: {[s.get('setpoint_kw') for s in steps]}"
    )


@then('all PlanSteps for asset "{asset_id}" have reason kind "{kind}"')
def step_all_steps_have_reason(context, asset_id, kind):
    plan = context.ven_plan
    steps = [s for s in plan.get("steps", []) if s.get("asset_id") == asset_id]
    assert steps, f"No steps found for asset '{asset_id}'"
    bad = [s for s in steps if s.get("reason", {}).get("kind") != kind]
    assert not bad, (
        f"{len(bad)} step(s) for '{asset_id}' have wrong reason.kind. "
        f"Expected all '{kind}', got: {[s.get('reason', {}).get('kind') for s in bad[:5]]}"
    )


@then('the response body has an empty "steps" array')
def step_response_steps_empty(context):
    body = context.last_response.json()
    steps = body.get("steps")
    assert steps is not None, "Response missing 'steps' field"
    assert steps == [], f"Expected steps=[], got {len(steps)} item(s)"
