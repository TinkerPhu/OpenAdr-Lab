"""Step definitions used as EV-session / shiftable-load *setup fixtures* by many other
feature files (dispatcher, planner, shiftable_lifecycle, uc_normal/stress/vtn_coordination,
ui_planner, 05_ev_charging_scenarios, isolated/shiftable_lifecycle).

BL-41: these used to POST directly to the now-removed POST/DELETE /ev-session,
/heater-target, /shiftable-loads routes (a simpler, superseded CRUD API). Rewritten to go
through the unified POST /user-requests (Stage 5) flow instead, which constructs the same
underlying EvSession/HeaterTarget/ShiftableLoad domain objects — same Gherkin step
phrasing, so none of the ~10 other feature files that use these steps as setup needed to
change. GET /ev-session (read-only) was kept — see routes/hems/ev.rs — since a
simulated-usage session has no linked UserRequest and is otherwise unobservable.
"""

import time
from datetime import datetime, timedelta, timezone
from behave import given, when
from features.helpers.api_client import ven_get, ven_post, ven_delete


# ── EV Session (now via /user-requests) ────────────────────────────────────────

@given("I POST an EV session with target_soc {soc:f} and departure in {hours:f} hours")
def step_given_post_ev_session(context, soc, hours):
    departure = (datetime.now(timezone.utc) + timedelta(hours=hours)).strftime(
        "%Y-%m-%dT%H:%M:%SZ"
    )
    r = ven_post("/user-requests", json={
        "asset_id": "ev",
        "target_soc": soc,
        "deadlines": [{"latest_end": departure}],
    })
    # The body names what refused it (a 409 lists the clashing sessions); raise_for_status
    # alone dropped that, leaving "409 Conflict" with nothing to go on.
    assert r.ok, f"POST /user-requests (EV) refused: {r.status_code} {r.text}"
    context.last_response = r
    context.last_response_json = r.json()


# ── Shiftable Loads (now via /user-requests) ────────────────────────────────────

def _post_shiftable_load(context, asset_id, kw, minutes, opens_in, window, require_ok):
    """POST a shiftable-load request whose window opens `opens_in` from now and stays open for
    `window`. On success, its request id is what DELETE /user-requests/:id needs (the request's
    own id, not the linked session_id)."""
    earliest_start = datetime.now(timezone.utc) + opens_in
    r = ven_post("/user-requests", json={
        "asset_id": asset_id,
        "deadlines": [],
        "power_kw": kw,
        "duration_min": minutes,
        "earliest_start": earliest_start.strftime("%Y-%m-%dT%H:%M:%SZ"),
        "latest_end": (earliest_start + window).strftime("%Y-%m-%dT%H:%M:%SZ"),
    })
    context.last_response = r
    if require_ok:
        assert r.ok, f"POST /user-requests (shiftable) refused: {r.status_code} {r.text}"
    try:
        context.last_response_json = r.json()
    except Exception:
        context.last_response_json = None
    if r.ok:
        context.last_shiftable_load_id = context.last_response_json.get("id")


@when('I POST a shiftable load for asset "{asset_id}" at {kw:f} kW for {minutes:d} minutes within {window:d} hours')
def step_when_post_shiftable_load(context, asset_id, kw, minutes, window):
    _post_shiftable_load(context, asset_id, kw, minutes, timedelta(0), timedelta(hours=window), False)


@given('I POST a shiftable load for asset "{asset_id}" at {kw:f} kW for {minutes:d} minutes within {window:d} hours')
def step_given_post_shiftable_load(context, asset_id, kw, minutes, window):
    _post_shiftable_load(context, asset_id, kw, minutes, timedelta(0), timedelta(hours=window), True)


@given('I POST a shiftable load for asset "{asset_id}" at {kw:f} kW for {minutes:d} minutes within {window_min:d} minutes')
def step_given_post_shiftable_load_min_window(context, asset_id, kw, minutes, window_min):
    _post_shiftable_load(context, asset_id, kw, minutes, timedelta(0), timedelta(minutes=window_min), True)


# Planning-delay margin: a load posted closer than this to the next slot boundary waits for the
# boundary first, so the plan that places it is made inside the slot it must start in.
_SLOT_MARGIN_S = 300


def _seconds_to_next_slot_boundary():
    """Seconds until the plan's next slot starts, read from the plan in force (its first slot
    anchors the grid, `horizon.step_size_s` is the slot length), not from a copy of the profile."""
    plan = ven_get("/plan").json()
    assert plan, "GET /plan returned no plan; a slot boundary cannot be derived"
    step_s = plan["horizon"]["step_size_s"]
    anchor = datetime.fromisoformat(plan["slots"][0]["start"][:19] + "+00:00")
    elapsed_s = (datetime.now(timezone.utc) - anchor).total_seconds()
    return step_s - (elapsed_s % step_s)


# A window only the current slot can start in: it ends one minute before a start in the next
# slot could finish. The planner cannot move the load to a cheaper later slot (GB-35: posted at
# 07:32 beside a running 2 kW load, wm-3 and wm-5 were planned into the 08:00 slot and the
# 420 s start poll timed out), so "has started" no longer depends on the time of day or on what
# else is running.
@given('I POST a shiftable load for asset "{asset_id}" at {kw:f} kW for {minutes:d} minutes that can only start in the current slot')
def step_given_post_shiftable_load_current_slot(context, asset_id, kw, minutes):
    to_boundary_s = _seconds_to_next_slot_boundary()
    if to_boundary_s < _SLOT_MARGIN_S:
        time.sleep(to_boundary_s + 5)
        to_boundary_s = _seconds_to_next_slot_boundary()
    window = timedelta(seconds=to_boundary_s) + timedelta(minutes=minutes) - timedelta(minutes=1)
    _post_shiftable_load(context, asset_id, kw, minutes, timedelta(0), window, True)


# A window that opens later: the load is planned but does not start during the scenario, so it
# can still be cancelled and cannot run on into later scenarios (a started load is not
# cancellable). GB-35: a main-pass wm-1 left running for an hour pushed the @isolated pass's
# loads out of their first slot.
@given('I POST a shiftable load for asset "{asset_id}" at {kw:f} kW for {minutes:d} minutes opening in {opens_h:d} hours for {window:d} hours')
def step_given_post_shiftable_load_opening_later(context, asset_id, kw, minutes, opens_h, window):
    _post_shiftable_load(context, asset_id, kw, minutes, timedelta(hours=opens_h), timedelta(hours=window), True)


@when('I DELETE shiftable load with saved id')
def step_when_delete_shiftable_load(context):
    request_id = context.last_shiftable_load_id
    r = ven_delete(f"/user-requests/{request_id}")
    context.last_response = r


# Note: generic assertion steps (response status, JSON field, JSON array)
# are defined in entity_model_steps.py — do not duplicate here.
