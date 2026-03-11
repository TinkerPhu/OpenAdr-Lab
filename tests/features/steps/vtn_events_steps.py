from datetime import datetime, timedelta, timezone

from behave import given, when, then
from features.helpers.api_client import vtn_get, vtn_post


@given('I create a program named "{name}" and save its ID')
def step_create_program_save_id(context, name):
    r = vtn_post(
        "/programs",
        context.vtn_token,
        json={"programName": name, "intervalPeriod": None, "programDescriptions": None},
    )
    if r.status_code == 409:
        programs = vtn_get("/programs", context.vtn_token).json()
        match = [p for p in programs if p.get("programName") == name]
        assert match, f"409 but program '{name}' not found in GET /programs"
        context.saved_program_id = match[0]["id"]
        return
    r.raise_for_status()
    context.saved_program_id = r.json()["id"]


@when("I create an event for the saved program")
@given("I create an event for the saved program")
def step_create_event(context):
    context.response = vtn_post(
        "/events",
        context.vtn_token,
        json={
            "programID": context.saved_program_id,
            "eventName": "test-event",
            "intervals": [
                {
                    "id": 0,
                    "payloads": [{"type": "SIMPLE", "values": [1.0]}],
                }
            ],
        },
    )


@then('the response contains a "{field}"')
def step_response_contains_a_field(context, field):
    body = context.response.json()
    assert field in body, f"Field '{field}' not found in {list(body.keys())}"


@when("I list events")
def step_list_events(context):
    context.response = vtn_get("/events", context.vtn_token)
    context.response.raise_for_status()
    context.event_list = context.response.json()


@then("the event list is not empty")
def step_event_list_not_empty(context):
    assert len(context.event_list) > 0, "Event list is empty"


@given('I create a past event "{name}" for the saved program')
def step_create_past_event(context, name):
    """Create an event whose intervalPeriod ended in the past."""
    start = datetime.now(timezone.utc) - timedelta(hours=1)
    r = vtn_post(
        "/events",
        context.vtn_token,
        json={
            "programID": context.saved_program_id,
            "eventName": name,
            "intervalPeriod": {
                "start": start.strftime("%Y-%m-%dT%H:%M:%SZ"),
                "duration": "PT1S",
            },
            "intervals": [{"id": 0, "payloads": [{"type": "SIMPLE", "values": [1.0]}]}],
        },
    )
    r.raise_for_status()
    if not hasattr(context, "uc_events"):
        context.uc_events = {}
    context.uc_events[name] = r.json()


@given('I create an open event "{name}" for the saved program')
def step_create_open_event(context, name):
    """Create an event with no duration (open-ended = always active)."""
    start = datetime.now(timezone.utc) - timedelta(minutes=5)
    r = vtn_post(
        "/events",
        context.vtn_token,
        json={
            "programID": context.saved_program_id,
            "eventName": name,
            "intervalPeriod": {
                "start": start.strftime("%Y-%m-%dT%H:%M:%SZ"),
            },
            "intervals": [{"id": 0, "payloads": [{"type": "SIMPLE", "values": [1.0]}]}],
        },
    )
    r.raise_for_status()
    if not hasattr(context, "uc_events"):
        context.uc_events = {}
    context.uc_events[name] = r.json()


@when("I list events for the saved program with active=true")
def step_list_events_active_true(context):
    context.response = vtn_get(
        "/events", context.vtn_token,
        params={"programID": context.saved_program_id, "active": "true"},
    )
    context.response.raise_for_status()
    context.event_list = context.response.json()


@when("I list events for the saved program with active=false")
def step_list_events_active_false(context):
    context.response = vtn_get(
        "/events", context.vtn_token,
        params={"programID": context.saved_program_id, "active": "false"},
    )
    context.response.raise_for_status()
    context.event_list = context.response.json()


@when("I list events for the saved program")
def step_list_events_for_program(context):
    context.response = vtn_get(
        "/events", context.vtn_token,
        params={"programID": context.saved_program_id},
    )
    context.response.raise_for_status()
    context.event_list = context.response.json()


@then('the event list contains "{name}"')
def step_event_list_contains(context, name):
    names = [e.get("eventName") for e in context.event_list]
    assert name in names, f"Expected '{name}' in event list, got: {names}"


@then('the event list does not contain "{name}"')
def step_event_list_not_contains(context, name):
    names = [e.get("eventName") for e in context.event_list]
    assert name not in names, f"Expected '{name}' NOT in event list, but found it: {names}"
