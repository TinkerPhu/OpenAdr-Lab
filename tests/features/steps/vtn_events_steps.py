from behave import given, when, then
from features.helpers.api_client import vtn_get, vtn_post


@given('I create a program named "{name}" and save its ID')
def step_create_program_save_id(context, name):
    r = vtn_post(
        "/programs",
        context.vtn_token,
        json={"programName": name, "intervalPeriod": None, "programDescriptions": None},
    )
    r.raise_for_status()
    body = r.json()
    context.saved_program_id = body["id"]


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
