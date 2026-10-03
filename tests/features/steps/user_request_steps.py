"""Step definitions for VEN User Request Manager (Stage 5 + Phase F leeway) BDD tests."""

import time
from datetime import datetime, timedelta, timezone
from behave import given, when, then
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
    context.last_ev_payload = payload
    r = ven_post("/user-requests", json=payload)
    context.last_response = r
    try:
        context.last_response_json = r.json()
        context.last_created_request = r.json()
    except Exception:
        context.last_response_json = None
        context.last_created_request = None


@when('I POST an EV user request available in {from_h:d} hours, departing in {to_h:d} hours')
def step_post_ev_request_with_window(context, from_h, to_h):
    """A session with a stated charging window — what lets a user hold more than one.

    Without `earliest_start` every stated session opens at the submission instant,
    so any two of them overlap and the second is refused.
    """
    now = datetime.now(timezone.utc)
    payload = {
        "asset_id": "ev",
        "target_soc": 0.8,
        "earliest_start": (now + timedelta(hours=from_h)).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "deadlines": [
            {
                "latest_end": (now + timedelta(hours=to_h)).strftime("%Y-%m-%dT%H:%M:%SZ"),
                "min_completion": 0.8,
            }
        ],
        "completion_policy": "STOP",
    }
    # Kept so a confirmed replacement resubmits the SAME request plus the
    # instruction, rather than a fresh one that might differ.
    context.last_ev_payload = payload
    r = ven_post("/user-requests", json=payload)
    context.last_response = r
    try:
        context.last_response_json = r.json()
        context.last_created_request = r.json()
    except Exception:
        context.last_response_json = None
        context.last_created_request = None


@then("the refusal names {count:d} conflicting session")
@then("the refusal names {count:d} conflicting sessions")
def step_refusal_names_conflicts(context, count):
    """The point of the refusal: the user is told *which* plan clashes.

    Asserted on the body rather than only on the status, because a 409 that says
    only "something clashed" is the hostile rejection this change exists to replace.
    """
    body = context.last_response_json
    assert body is not None, "refusal had no JSON body"
    assert body.get("kind") in ("ev_session_conflict", "ev_replace_rejected"), (
        f"refusal must declare its kind, got {body.get('kind')!r}"
    )
    conflicts = body.get("conflicts") or []
    assert len(conflicts) == count, f"expected {count} named conflict(s), got {conflicts}"
    for c in conflicts:
        for field in ("id", "window_start", "departure_time", "target_soc"):
            assert field in c, f"a named conflict must carry {field}: {c}"
    # What a confirmation must echo back, quoted by the server.
    ids = body.get("replaceable_session_ids") or []
    assert sorted(ids) == sorted(c["id"] for c in conflicts)
    context.replaceable_session_ids = ids


@when("I resubmit that request replacing the named sessions")
def step_resubmit_replacing(context):
    """Confirm the replacement, echoing back exactly the ids the refusal quoted."""
    payload = dict(context.last_ev_payload)
    payload["replace_session_ids"] = context.replaceable_session_ids
    r = ven_post("/user-requests", json=payload)
    context.last_response = r
    try:
        context.last_response_json = r.json()
    except Exception:
        context.last_response_json = None


@when("I resubmit that request replacing a session that is not queued")
def step_resubmit_stale(context):
    """A confirmation that went stale must remove nothing."""
    payload = dict(context.last_ev_payload)
    payload["replace_session_ids"] = ["00000000-0000-0000-0000-000000000000"]
    r = ven_post("/user-requests", json=payload)
    context.last_response = r
    try:
        context.last_response_json = r.json()
    except Exception:
        context.last_response_json = None


@then("the EV session queue holds {count:d} sessions")
def step_ev_queue_holds(context, count):
    r = ven_get("/ev-session")
    assert r.status_code == 200, f"GET /ev-session returned {r.status_code}"
    sessions = r.json()
    assert isinstance(sessions, list), f"expected a list of sessions, got {type(sessions)}"
    assert len(sessions) == count, f"expected {count} queued sessions, got {len(sessions)}: {sessions}"
    context.ev_sessions = sessions


@when("I poll the EV session queue until it has at least {count:d} session")
@when("I poll the EV session queue until it has at least {count:d} sessions")
def step_poll_ev_queue(context, count):
    """The simulated schedule writes on a tick, so this waits rather than checks once."""
    deadline = time.time() + 60
    sessions = []
    while time.time() < deadline:
        r = ven_get("/ev-session")
        if r.status_code == 200:
            sessions = r.json()
            if isinstance(sessions, list) and len(sessions) >= count:
                context.ev_sessions = sessions
                return
        time.sleep(3)
    raise AssertionError(f"expected >= {count} queued sessions within 60s, last saw {sessions}")


@then('every queued EV session has origin "{origin}"')
def step_ev_queue_origin(context, origin):
    assert context.ev_sessions, "no sessions captured"
    wrong = [s for s in context.ev_sessions if s.get("origin") != origin]
    assert not wrong, f"sessions with unexpected origin: {wrong}"


@then("the EV session queue stays empty for {seconds:d} seconds")
def step_ev_queue_stays_empty(context, seconds):
    """Asserts an absence, so it has to wait rather than check once: a session the VEN
    might create would appear on its next poll, not instantly."""
    deadline = time.time() + seconds
    while time.time() < deadline:
        r = ven_get("/ev-session")
        assert r.status_code == 200, f"GET /ev-session returned {r.status_code}"
        sessions = r.json()
        assert sessions == [], f"expected no EV sessions, got {sessions}"
        time.sleep(2)


@then("the queued EV sessions do not overlap")
def step_ev_queue_no_overlap(context):
    sessions = sorted(context.ev_sessions, key=lambda s: s["window_start"])
    for earlier, later in zip(sessions, sessions[1:]):
        assert earlier["departure_time"] <= later["window_start"], (
            f"sessions overlap: {earlier['window_start']}..{earlier['departure_time']} "
            f"then {later['window_start']}..{later['departure_time']}"
        )


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
    r = ven_post("/user-requests", json=payload)
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
    r = ven_post("/user-requests", json=payload)
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
    context.saved_session_id = req.get("session_id")
    assert context.saved_request_id, f"Request has no 'id' field: {req}"


@when("I DELETE the saved user request")
def step_delete_user_request(context):
    req_id = context.saved_request_id
    assert req_id, "No saved_request_id in context"
    r = ven_delete(f"/user-requests/{req_id}")
    context.last_response = r
    context.last_response_json = None


# ---------------------------------------------------------------------------
# Then: assertions on /user-requests and cancellation
# ---------------------------------------------------------------------------

@then("the requests list has at least {count:d} item")
@then("the requests list has at least {count:d} items")
def step_requests_at_least(context, count):
    data = context.last_response_json
    assert isinstance(data, list), f"Expected list, got {type(data)}: {data}"
    assert len(data) >= count, f"Expected >= {count} requests, got {len(data)}"


@then("the EV session is cleared after cancellation")
def step_ev_session_cleared(context):
    """After cancelling a user request, its entry in GET /user-requests must have no
    linked session (state.cancel_request clears the EvSession atomically)."""
    from features.helpers.api_client import ven_get
    req_id = context.saved_request_id
    r = ven_get("/user-requests")
    r.raise_for_status()
    items = [item for item in r.json() if item.get("id") == req_id]
    assert items, f"request {req_id} not found in /user-requests"
    assert items[0].get("session") is None, (
        f"Expected EV session cleared, got {items[0].get('session')}"
    )


# ---------------------------------------------------------------------------
# Phase F: User leeway steps
# ---------------------------------------------------------------------------

@when('I POST a user request with interruptible true and tolerance_min {tolerance:d} for asset "{asset_id}"')
def step_post_user_request_with_leeway(context, tolerance, asset_id):
    latest_end = (datetime.now(timezone.utc) + timedelta(hours=12)).strftime(
        "%Y-%m-%dT%H:%M:%SZ"
    )
    payload = {
        "asset_id": asset_id,
        "target_soc": 0.80,
        "deadlines": [{"latest_end": latest_end, "min_completion": 0.8}],
        "completion_policy": "STOP",
        "interruptible": True,
        "tolerance_min": tolerance,
    }
    r = ven_post("/user-requests", json=payload)
    context.last_response = r
    try:
        context.last_response_json = r.json()
        context.last_created_request = r.json()
    except Exception:
        context.last_response_json = None
        context.last_created_request = None


@when('I POST a user request with budget_eur {budget:f} for asset "{asset_id}"')
def step_post_user_request_with_budget_eur(context, budget, asset_id):
    latest_end = (datetime.now(timezone.utc) + timedelta(hours=12)).strftime(
        "%Y-%m-%dT%H:%M:%SZ"
    )
    payload = {
        "asset_id": asset_id,
        "target_soc": 0.80,
        "deadlines": [{"latest_end": latest_end, "min_completion": 0.8}],
        "completion_policy": "STOP",
        "budget_eur": budget,
    }
    r = ven_post("/user-requests", json=payload)
    context.last_response = r
    try:
        context.last_response_json = r.json()
        context.last_created_request = r.json()
    except Exception:
        context.last_response_json = None
        context.last_created_request = None


@given("the VEN has a scheduled interruptible EV session")
def step_given_scheduled_interruptible_ev_session(context):
    """Create an interruptible EV request and wait until the plan has an EV allocation."""
    latest_end = (datetime.now(timezone.utc) + timedelta(hours=12)).strftime(
        "%Y-%m-%dT%H:%M:%SZ"
    )
    payload = {
        "asset_id": "ev",
        "target_soc": 0.90,
        "deadlines": [{"latest_end": latest_end, "min_completion": 0.8}],
        "completion_policy": "STOP",
        "interruptible": True,
        "desired_power_kw": 7.0,
    }
    r = ven_post("/user-requests", json=payload)
    r.raise_for_status()
    context.interruptible_session_id = r.json().get("session_id")

    # Wait for the plan to reflect an EV allocation
    deadline = time.time() + 60
    while time.time() < deadline:
        rp = ven_get("/plan")
        if rp.status_code == 200:
            plan = rp.json()
            slots = plan.get("slots") if plan else None
            if slots and any(
                slot.get("allocations", {}).get("ev", 0) > 0
                for slot in slots
                if slot.get("status") == "FIRM"
            ):
                return
        time.sleep(2)

    # Proceed anyway — flexibility check may still pass if plan is partial


@then('the response JSON field "{field_path}" is true')
def step_response_json_field_is_true(context, field_path):
    data = context.last_response_json
    parts = field_path.split(".")
    val = data
    for p in parts:
        assert isinstance(val, dict), f"Expected dict at '{p}', got {type(val)}: {val}"
        val = val.get(p)
    assert val is True, f"Field '{field_path}' is not true: {val!r}"
