"""Step definitions used as EV-session / shiftable-load *setup fixtures* by many other
feature files (dispatcher, planner, shiftable_lifecycle, uc_normal/stress/vtn_coordination,
ui_planner, 05_ev_charging_scenarios, isolated/shiftable_lifecycle).

BL-41: these used to POST directly to the now-removed POST/DELETE /ev-session,
/heater-target, /shiftable-loads routes (a simpler, superseded CRUD API). Rewritten to go
through the unified POST /user-requests (Stage 5) flow instead, which constructs the same
underlying EvSession/HeaterTarget/ShiftableLoad domain objects — same Gherkin step
phrasing, so none of the ~10 other feature files that use these steps as setup needed to
change. GET /ev-session (read-only) was kept — see routes/hems/ev.rs — since a
VTN-triggered CHARGE_STATE_SETPOINT session has no linked UserRequest and is otherwise
unobservable.
"""

from datetime import datetime, timedelta, timezone
from behave import given, when
from features.helpers.api_client import ven_post, ven_delete


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
    r.raise_for_status()
    context.last_response = r
    context.last_response_json = r.json()


# ── Shiftable Loads (now via /user-requests) ────────────────────────────────────

@when('I POST a shiftable load for asset "{asset_id}" at {kw:f} kW for {minutes:d} minutes within {window:d} hours')
def step_when_post_shiftable_load(context, asset_id, kw, minutes, window):
    now = datetime.now(timezone.utc)
    earliest_start = now.strftime("%Y-%m-%dT%H:%M:%SZ")
    latest_end = (now + timedelta(hours=window)).strftime("%Y-%m-%dT%H:%M:%SZ")
    r = ven_post("/user-requests", json={
        "asset_id": asset_id,
        "deadlines": [],
        "power_kw": kw,
        "duration_min": minutes,
        "earliest_start": earliest_start,
        "latest_end": latest_end,
    })
    context.last_response = r
    try:
        context.last_response_json = r.json()
    except Exception:
        context.last_response_json = None


@given('I POST a shiftable load for asset "{asset_id}" at {kw:f} kW for {minutes:d} minutes within {window:d} hours')
def step_given_post_shiftable_load(context, asset_id, kw, minutes, window):
    now = datetime.now(timezone.utc)
    earliest_start = now.strftime("%Y-%m-%dT%H:%M:%SZ")
    latest_end = (now + timedelta(hours=window)).strftime("%Y-%m-%dT%H:%M:%SZ")
    r = ven_post("/user-requests", json={
        "asset_id": asset_id,
        "deadlines": [],
        "power_kw": kw,
        "duration_min": minutes,
        "earliest_start": earliest_start,
        "latest_end": latest_end,
    })
    r.raise_for_status()
    context.last_response = r
    context.last_response_json = r.json()
    # /user-requests returns the UserRequest, not the ShiftableLoad — its own "id" is what
    # DELETE /user-requests/:id needs (not the linked session_id).
    context.last_shiftable_load_id = r.json().get("id")


@given('I POST a shiftable load for asset "{asset_id}" at {kw:f} kW for {minutes:d} minutes within {window_min:d} minutes')
def step_given_post_shiftable_load_min_window(context, asset_id, kw, minutes, window_min):
    now = datetime.now(timezone.utc)
    earliest_start = now.strftime("%Y-%m-%dT%H:%M:%SZ")
    latest_end = (now + timedelta(minutes=window_min)).strftime("%Y-%m-%dT%H:%M:%SZ")
    r = ven_post("/user-requests", json={
        "asset_id": asset_id,
        "deadlines": [],
        "power_kw": kw,
        "duration_min": minutes,
        "earliest_start": earliest_start,
        "latest_end": latest_end,
    })
    r.raise_for_status()
    context.last_response = r
    context.last_response_json = r.json()
    context.last_shiftable_load_id = r.json().get("id")


@when('I DELETE shiftable load with saved id')
def step_when_delete_shiftable_load(context):
    request_id = context.last_shiftable_load_id
    r = ven_delete(f"/user-requests/{request_id}")
    context.last_response = r


# Note: generic assertion steps (response status, JSON field, JSON array)
# are defined in entity_model_steps.py — do not duplicate here.
