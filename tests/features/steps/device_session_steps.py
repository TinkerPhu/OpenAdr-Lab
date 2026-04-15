"""Step definitions for device-session API (Phase C).

Covers: EvSession, HeaterTarget, ShiftableLoad CRUD + plan integration.
"""

from datetime import datetime, timedelta, timezone
from behave import given, when
from features.helpers.api_client import ven_get, ven_post, ven_delete


# ── EV Session ───────────────────────────────────────────────────────────────

@given("I POST an EV session with target_soc {soc:f} and departure in {hours:f} hours")
def step_given_post_ev_session(context, soc, hours):
    departure = (datetime.now(timezone.utc) + timedelta(hours=hours)).strftime(
        "%Y-%m-%dT%H:%M:%SZ"
    )
    r = ven_post("/ev-session", json={
        "target_soc": soc,
        "departure_time": departure,
        "opportunistic": False,
    })
    r.raise_for_status()
    context.last_response = r
    context.last_response_json = r.json()


@when("I POST an EV session with target_soc {soc:f} and departure in {hours:f} hours")
def step_when_post_ev_session(context, soc, hours):
    departure = (datetime.now(timezone.utc) + timedelta(hours=hours)).strftime(
        "%Y-%m-%dT%H:%M:%SZ"
    )
    r = ven_post("/ev-session", json={
        "target_soc": soc,
        "departure_time": departure,
        "opportunistic": False,
    })
    context.last_response = r
    try:
        context.last_response_json = r.json()
    except Exception:
        context.last_response_json = None


@when("I GET the EV session from /ev-session")
def step_when_get_ev_session(context):
    r = ven_get("/ev-session")
    context.last_response = r
    try:
        context.last_response_json = r.json()
    except Exception:
        context.last_response_json = None


@when("I DELETE the EV session")
def step_when_delete_ev_session(context):
    r = ven_delete("/ev-session")
    context.last_response = r
    try:
        context.last_response_json = r.json()
    except Exception:
        context.last_response_json = None


@given("I DELETE the EV session")
def step_given_delete_ev_session(context):
    r = ven_delete("/ev-session")
    context.last_response = r


# ── Heater Target ────────────────────────────────────────────────────────────

@when("I POST a heater target of {temp:f} C ready in {hours:f} hours")
def step_when_post_heater_target(context, temp, hours):
    ready_by = (datetime.now(timezone.utc) + timedelta(hours=hours)).strftime(
        "%Y-%m-%dT%H:%M:%SZ"
    )
    r = ven_post("/heater-target", json={
        "target_temp_c": temp,
        "ready_by": ready_by,
    })
    context.last_response = r
    try:
        context.last_response_json = r.json()
    except Exception:
        context.last_response_json = None


@given("I POST a heater target of {temp:f} C ready in {hours:f} hours")
def step_given_post_heater_target(context, temp, hours):
    ready_by = (datetime.now(timezone.utc) + timedelta(hours=hours)).strftime(
        "%Y-%m-%dT%H:%M:%SZ"
    )
    r = ven_post("/heater-target", json={
        "target_temp_c": temp,
        "ready_by": ready_by,
    })
    r.raise_for_status()
    context.last_response = r
    context.last_response_json = r.json()


@when("I GET the heater target from /heater-target")
def step_when_get_heater_target(context):
    r = ven_get("/heater-target")
    context.last_response = r
    try:
        context.last_response_json = r.json()
    except Exception:
        context.last_response_json = None


@when("I DELETE the heater target")
def step_when_delete_heater_target(context):
    r = ven_delete("/heater-target")
    context.last_response = r


@given("I DELETE the heater target")
def step_given_delete_heater_target(context):
    ven_delete("/heater-target")


# ── Shiftable Loads ──────────────────────────────────────────────────────────

@when('I POST a shiftable load for asset "{asset_id}" at {kw:f} kW for {minutes:d} minutes within {window:d} hours')
def step_when_post_shiftable_load(context, asset_id, kw, minutes, window):
    now = datetime.now(timezone.utc)
    earliest_start = now.strftime("%Y-%m-%dT%H:%M:%SZ")
    latest_end = (now + timedelta(hours=window)).strftime("%Y-%m-%dT%H:%M:%SZ")
    r = ven_post("/shiftable-loads", json={
        "asset_id": asset_id,
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
    r = ven_post("/shiftable-loads", json={
        "asset_id": asset_id,
        "power_kw": kw,
        "duration_min": minutes,
        "earliest_start": earliest_start,
        "latest_end": latest_end,
    })
    r.raise_for_status()
    context.last_response = r
    context.last_response_json = r.json()
    context.last_shiftable_load_id = r.json().get("id")


@when("I GET the shiftable loads from /shiftable-loads")
def step_when_get_shiftable_loads(context):
    r = ven_get("/shiftable-loads")
    context.last_response = r
    try:
        context.last_response_json = r.json()
    except Exception:
        context.last_response_json = None


@when('I DELETE shiftable load with saved id')
def step_when_delete_shiftable_load(context):
    load_id = context.last_shiftable_load_id
    r = ven_delete(f"/shiftable-loads/{load_id}")
    context.last_response = r


# Note: generic assertion steps (response status, JSON field, JSON array)
# are defined in entity_model_steps.py — do not duplicate here.
