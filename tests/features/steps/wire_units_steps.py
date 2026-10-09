"""Step definitions for reading incoming values through their declared unit (GB-50)."""

import json
from datetime import datetime, timedelta, timezone

from behave import given, then
from features.helpers.api_client import ven_get, vtn_post
from features.helpers.wait import poll_until

UNIT_REFUSALS_KEY = "events:units"


def _send(context, payload_type, value, descriptor):
    """One event in force now for ten minutes, with `descriptor` as its only payloadDescriptor
    (None = the event declares nothing). Saved like the capacity-schedule events, so the
    existing "I delete the schedule event" step removes it."""
    start = datetime.now(timezone.utc) - timedelta(minutes=1)
    body = {
        "programID": context.saved_program_id,
        "eventName": f"wire-units-{payload_type.lower()}",
        "intervalPeriod": {"start": start.strftime("%Y-%m-%dT%H:%M:%SZ"), "duration": "PT10M"},
        "intervals": [{"id": 0, "payloads": [{"type": payload_type, "values": [value]}]}],
    }
    if descriptor is not None:
        body["payloadDescriptors"] = [{"payloadType": payload_type, **descriptor}]
    r = vtn_post("/events", context.vtn_token, json=body)
    assert r.status_code == 201, f"POST /events: {r.status_code} {r.text[:300]}"
    context.schedule_event_id = r.json().get("id")


@given('I send an "{payload_type}" of {value:g} in force now that declares no unit')
def step_send_undeclared(context, payload_type, value):
    _send(context, payload_type, value, None)


@given('I send an "{payload_type}" of {value:g} in force now declared as {declaration}')
def step_send_declared(context, payload_type, value, declaration):
    _send(context, payload_type, value, json.loads(declaration))


def _health():
    r = ven_get("/health")
    assert r.status_code == 200, f"GET /health: {r.status_code}"
    return r.json()


def _unit_rejection(body):
    """The wire_conformance detail when it is about units, else None. The detail joins every
    standing rejection, so the units one is recognised by its own wording."""
    component = body["components"]["wire_conformance"]
    detail = component.get("detail") or ""
    return detail if component["status"] == "degraded" and "declared unit" in detail else None


@then('the VEN health lists "{payload_type}" under wire assumptions within {timeout:d} seconds')
def step_health_lists_assumption(context, payload_type, timeout):
    body = poll_until(
        _health,
        lambda b: (b.get("wire_assumptions") or {}).get(payload_type, 0) >= 1,
        timeout=timeout,
        interval=2,
        description=f"/health wire_assumptions lists {payload_type}",
    )
    context.ven_health = body


@then("the VEN health does not blame units for a wire rejection")
def step_health_no_unit_rejection(context):
    detail = _unit_rejection(_health())
    assert detail is None, f"an assumed default must not be a rejection: {detail}"


@then('the VEN health reports a wire rejection naming "{named}" within {timeout:d} seconds')
def step_health_names_refusal(context, named, timeout):
    body = poll_until(
        _health,
        lambda b: named in (_unit_rejection(b) or ""),
        timeout=timeout,
        interval=2,
        description=f"/health wire_conformance degraded naming {named!r}",
    )
    detail = _unit_rejection(body)
    assert body["status"] == "degraded", f"a refused unit degrades health: {body}"
    assert context.schedule_event_id in detail, f"the event is named: {detail}"


@then("the VEN health stops reporting a units wire rejection within {timeout:d} seconds")
def step_health_recovers(context, timeout):
    poll_until(
        _health,
        lambda b: _unit_rejection(b) is None,
        timeout=timeout,
        interval=2,
        description="/health no longer reports a units wire rejection",
    )
