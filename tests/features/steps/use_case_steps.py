from behave import when, then
from features.helpers.api_client import vtn_post, vtn_get, vtn_delete


@when('I create an event with payload type "{ptype}" and priority {priority:d}')
def step_create_event_with_priority(context, ptype, priority):
    context.response = vtn_post(
        "/events",
        context.vtn_token,
        json={
            "programID": context.saved_program_id,
            "eventName": f"test-{ptype.lower()}-pri{priority}",
            "priority": priority,
            "intervals": [
                {"id": 0, "payloads": [{"type": ptype, "values": [0]}]},
            ],
        },
    )
    if context.response.status_code == 201:
        context.created_event = context.response.json()


@when('I create an event with payload type "{ptype}" and values [{values}]')
def step_create_event_with_values(context, ptype, values):
    vals = [float(v.strip()) for v in values.split(",")]
    context.response = vtn_post(
        "/events",
        context.vtn_token,
        json={
            "programID": context.saved_program_id,
            "eventName": f"test-{ptype.lower()}",
            "intervals": [
                {"id": 0, "payloads": [{"type": ptype, "values": vals}]},
            ],
        },
    )
    if context.response.status_code == 201:
        context.created_event = context.response.json()


@when('I create a multi-interval event with payload type "{ptype}"')
def step_create_multi_interval_event(context, ptype):
    context.response = vtn_post(
        "/events",
        context.vtn_token,
        json={
            "programID": context.saved_program_id,
            "eventName": f"test-{ptype.lower()}-multi",
            "priority": 5,
            "intervals": [
                {"id": 0, "payloads": [{"type": ptype, "values": [100.0]}]},
                {"id": 1, "payloads": [{"type": ptype, "values": [50.0]}]},
                {"id": 2, "payloads": [{"type": ptype, "values": [100.0]}]},
            ],
        },
    )
    if context.response.status_code == 201:
        context.created_event = context.response.json()


@when('I create an event with payload type "{ptype}" and intervalPeriod')
def step_create_event_with_interval_period(context, ptype):
    context.response = vtn_post(
        "/events",
        context.vtn_token,
        json={
            "programID": context.saved_program_id,
            "eventName": f"test-{ptype.lower()}-timed",
            "priority": 3,
            "intervalPeriod": {
                "start": "2026-03-01T14:00:00Z",
                "duration": "PT4H",
            },
            "intervals": [
                {"id": 0, "payloads": [{"type": ptype, "values": [50.0]}]},
            ],
        },
    )
    if context.response.status_code == 201:
        context.created_event = context.response.json()


@when('I create an event with payload type "{ptype}" and targets')
def step_create_event_with_targets(context, ptype):
    context.response = vtn_post(
        "/events",
        context.vtn_token,
        json={
            "programID": context.saved_program_id,
            "eventName": f"test-{ptype.lower()}-targeted",
            "priority": 2,
            "targets": [
                {"type": "VEN_NAME", "values": ["ven-1-name"]},
            ],
            "intervals": [
                {"id": 0, "payloads": [{"type": ptype, "values": [0.0]}]},
            ],
        },
    )
    if context.response.status_code == 201:
        context.created_event = context.response.json()


@when("I delete the created event")
def step_delete_created_event(context):
    event_id = context.created_event["id"]
    context.response = vtn_delete(f"/events/{event_id}", context.vtn_token)
    context.deleted_event_id = event_id


@then("the event response has priority {priority:d}")
def step_event_has_priority(context, priority):
    body = context.created_event
    assert body.get("priority") == priority, f"Expected priority {priority}, got {body.get('priority')}"


@then('the event response has payload type "{ptype}"')
def step_event_has_payload_type(context, ptype):
    body = context.created_event
    intervals = body.get("intervals", [])
    assert len(intervals) > 0, "No intervals in response"
    payloads = intervals[0].get("payloads", [])
    assert len(payloads) > 0, "No payloads in first interval"
    actual = payloads[0].get("type")
    assert actual == ptype, f"Expected payload type '{ptype}', got '{actual}'"


@then("the event response has {count:d} intervals")
def step_event_has_n_intervals(context, count):
    body = context.created_event
    intervals = body.get("intervals", [])
    assert len(intervals) == count, f"Expected {count} intervals, got {len(intervals)}"


@then("the event response has an intervalPeriod")
def step_event_has_interval_period(context):
    body = context.created_event
    ip = body.get("intervalPeriod")
    assert ip is not None, "intervalPeriod is missing"
    assert "start" in ip, "intervalPeriod missing 'start'"


@then("the event response has targets")
def step_event_has_targets(context):
    body = context.created_event
    targets = body.get("targets")
    assert targets is not None and len(targets) > 0, f"Expected targets, got {targets}"


@then("the event no longer exists")
def step_event_no_longer_exists(context):
    r = vtn_get(f"/events/{context.deleted_event_id}", context.vtn_token)
    assert r.status_code == 404, f"Expected 404 for deleted event, got {r.status_code}"
