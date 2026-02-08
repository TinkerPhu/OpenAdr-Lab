"""Step definitions for full end-to-end use case scenarios."""

from behave import given, when, then
from features.helpers.api_client import (
    vtn_get, vtn_post, vtn_delete,
    ven_get, ven_post, ven2_get, ven2_post,
)
from features.helpers.wait import poll_until


# ── helpers ──────────────────────────────────────────────────────────────────

def _ven1_events():
    return ven_get("/events").json()


def _ven2_events():
    return ven2_get("/events").json()


def _find_event(events, name):
    return next((e for e in events if e.get("eventName") == name), None)


def _build_intervals(ptype, count):
    """Build interval list. UC6 uses negative values for discharge."""
    if ptype == "CHARGE_STATE_SETPOINT" and count == 3:
        return [
            {"id": 0, "payloads": [{"type": ptype, "values": [80.0]}]},
            {"id": 1, "payloads": [{"type": ptype, "values": [-50.0]}]},
            {"id": 2, "payloads": [{"type": ptype, "values": [90.0]}]},
        ]
    if ptype == "PRICE" and count == 3:
        return [
            {"id": 0, "payloads": [{"type": ptype, "values": [0.12]}]},
            {"id": 1, "payloads": [{"type": ptype, "values": [0.35]}]},
            {"id": 2, "payloads": [{"type": ptype, "values": [0.15]}]},
        ]
    if count == 1:
        return [{"id": 0, "payloads": [{"type": ptype, "values": [0.0]}]}]
    return [
        {"id": i, "payloads": [{"type": ptype, "values": [100.0 - i * 25.0]}]}
        for i in range(count)
    ]


# ── program creation (with saved ID) ────────────────────────────────────────

@given('I create a program "{name}" targeting "{ven}" and save its ID')
def step_create_targeted_program_save_id(context, name, ven):
    r = vtn_post(
        "/programs",
        context.vtn_token,
        json={
            "programName": name,
            "targets": [{"type": "VEN_NAME", "values": [ven]}],
        },
    )
    r.raise_for_status()
    context.saved_program_id = r.json()["id"]


@given('I create a program "{name}" targeting both "{ven1}" and "{ven2}" and save its ID')
def step_create_dual_targeted_program_save_id(context, name, ven1, ven2):
    r = vtn_post(
        "/programs",
        context.vtn_token,
        json={
            "programName": name,
            "targets": [
                {"type": "VEN_NAME", "values": [ven1]},
                {"type": "VEN_NAME", "values": [ven2]},
            ],
        },
    )
    r.raise_for_status()
    context.saved_program_id = r.json()["id"]


@given('I create an open program "{name}" and save its ID')
def step_create_open_program_save_id(context, name):
    r = vtn_post(
        "/programs",
        context.vtn_token,
        json={"programName": name, "targets": None},
    )
    r.raise_for_status()
    context.saved_program_id = r.json()["id"]


# ── event creation ───────────────────────────────────────────────────────────

@when('I create a UC event "{name}" with type "{ptype}" priority {pri:d} and {count:d} interval')
@when('I create a UC event "{name}" with type "{ptype}" priority {pri:d} and {count:d} intervals')
def step_create_uc_event(context, name, ptype, pri, count):
    body = {
        "programID": context.saved_program_id,
        "eventName": name,
        "priority": pri,
        "intervals": _build_intervals(ptype, count),
    }
    context.response = vtn_post("/events", context.vtn_token, json=body)
    if context.response.status_code == 201:
        context.created_event = context.response.json()
        # track for cleanup / cancellation lookup
        if not hasattr(context, "uc_events"):
            context.uc_events = {}
        context.uc_events[name] = context.created_event


@when('I create a UC event "{name}" with type "{ptype}" priority {pri:d} and {count:d} interval with intervalPeriod')
def step_create_uc_event_with_ip(context, name, ptype, pri, count):
    body = {
        "programID": context.saved_program_id,
        "eventName": name,
        "priority": pri,
        "intervalPeriod": {
            "start": "2026-03-01T14:00:00Z",
            "duration": "PT4H",
        },
        "intervals": _build_intervals(ptype, count),
    }
    context.response = vtn_post("/events", context.vtn_token, json=body)
    if context.response.status_code == 201:
        context.created_event = context.response.json()
        if not hasattr(context, "uc_events"):
            context.uc_events = {}
        context.uc_events[name] = context.created_event


@when('I create a UC event "{name}" with type "{ptype}" priority {pri:d} and {count:d} interval with targets')
def step_create_uc_event_with_targets(context, name, ptype, pri, count):
    body = {
        "programID": context.saved_program_id,
        "eventName": name,
        "priority": pri,
        "targets": [{"type": "VEN_NAME", "values": ["ven-2"]}],
        "intervals": _build_intervals(ptype, count),
    }
    context.response = vtn_post("/events", context.vtn_token, json=body)
    if context.response.status_code == 201:
        context.created_event = context.response.json()
        if not hasattr(context, "uc_events"):
            context.uc_events = {}
        context.uc_events[name] = context.created_event


# ── VEN event polling ────────────────────────────────────────────────────────

@when('I wait for VEN-1 to show event "{name}"')
def step_wait_ven1_event(context, name):
    poll_until(
        _ven1_events,
        lambda events: _find_event(events, name) is not None,
        timeout=30,
        interval=3,
        description=f"VEN-1 shows event '{name}'",
    )


@when('I wait for VEN-2 to show event "{name}"')
def step_wait_ven2_event(context, name):
    poll_until(
        _ven2_events,
        lambda events: _find_event(events, name) is not None,
        timeout=30,
        interval=3,
        description=f"VEN-2 shows event '{name}'",
    )


@when('I wait for VEN-1 to no longer show event "{name}"')
def step_wait_ven1_event_gone(context, name):
    poll_until(
        _ven1_events,
        lambda events: _find_event(events, name) is None,
        timeout=30,
        interval=3,
        description=f"VEN-1 no longer shows event '{name}'",
    )


# ── negative VEN assertions ─────────────────────────────────────────────────

@then('VEN-1 does not have event "{name}"')
def step_ven1_no_event(context, name):
    events = _ven1_events()
    assert _find_event(events, name) is None, (
        f"Event '{name}' unexpectedly found on VEN-1"
    )


@then('VEN-2 does not have event "{name}"')
def step_ven2_no_event(context, name):
    events = _ven2_events()
    assert _find_event(events, name) is None, (
        f"Event '{name}' unexpectedly found on VEN-2"
    )


# ── VEN-side event structure checks ──────────────────────────────────────────

def _get_ven_event(fetch_fn, name):
    events = fetch_fn()
    evt = _find_event(events, name)
    assert evt is not None, f"Event '{name}' not found"
    return evt


@then('the VEN-1 event "{name}" has payload type "{ptype}"')
def step_ven1_event_payload_type(context, name, ptype):
    evt = _get_ven_event(_ven1_events, name)
    intervals = evt.get("intervals", [])
    assert len(intervals) > 0, "No intervals on VEN-1 event"
    actual = intervals[0]["payloads"][0]["type"]
    assert actual == ptype, f"Expected payload type '{ptype}', got '{actual}'"


@then('the VEN-2 event "{name}" has payload type "{ptype}"')
def step_ven2_event_payload_type(context, name, ptype):
    evt = _get_ven_event(_ven2_events, name)
    intervals = evt.get("intervals", [])
    assert len(intervals) > 0, "No intervals on VEN-2 event"
    actual = intervals[0]["payloads"][0]["type"]
    assert actual == ptype, f"Expected payload type '{ptype}', got '{actual}'"


@then('the VEN-1 event "{name}" has priority {pri:d}')
def step_ven1_event_priority(context, name, pri):
    evt = _get_ven_event(_ven1_events, name)
    actual = evt.get("priority")
    assert actual == pri, f"Expected priority {pri}, got {actual}"


@then('the VEN-1 event "{name}" has {count:d} intervals')
def step_ven1_event_interval_count(context, name, count):
    evt = _get_ven_event(_ven1_events, name)
    actual = len(evt.get("intervals", []))
    assert actual == count, f"Expected {count} intervals, got {actual}"


@then('the VEN-2 event "{name}" has {count:d} intervals')
def step_ven2_event_interval_count(context, name, count):
    evt = _get_ven_event(_ven2_events, name)
    actual = len(evt.get("intervals", []))
    assert actual == count, f"Expected {count} intervals, got {actual}"


@then('the VEN-1 event "{name}" has an intervalPeriod')
def step_ven1_event_has_interval_period(context, name):
    evt = _get_ven_event(_ven1_events, name)
    ip = evt.get("intervalPeriod")
    assert ip is not None, "intervalPeriod missing on VEN-1 event"
    assert "start" in ip, "intervalPeriod missing 'start'"


# ── event deletion ───────────────────────────────────────────────────────────

@when('I delete event "{name}"')
def step_delete_event_by_name(context, name):
    event_id = context.uc_events[name]["id"]
    context.response = vtn_delete(f"/events/{event_id}", context.vtn_token)


# ── report submission and verification ───────────────────────────────────────

@when('I submit a report via VEN-1 for event "{name}"')
def step_submit_report_ven1(context, name):
    evt = _get_ven_event(_ven1_events, name)
    payload = {
        "programID": evt.get("programID", ""),
        "eventID": evt["id"],
        "clientName": "ven-1",
        "resources": [],
    }
    r = ven_post("/reports", json=payload)
    assert r.status_code in (200, 201), (
        f"VEN-1 report submission failed: {r.status_code} {r.text[:200]}"
    )
    context.submitted_report = payload


@when('I submit a report via VEN-2 for event "{name}"')
def step_submit_report_ven2(context, name):
    evt = _get_ven_event(_ven2_events, name)
    payload = {
        "programID": evt.get("programID", ""),
        "eventID": evt["id"],
        "clientName": "ven-2",
        "resources": [],
    }
    r = ven2_post("/reports", json=payload)
    assert r.status_code in (200, 201), (
        f"VEN-2 report submission failed: {r.status_code} {r.text[:200]}"
    )
    context.submitted_report = payload


@then('the report for event "{name}" from "{client}" appears in VTN')
def step_report_visible_in_vtn(context, name, client):
    event_id = context.submitted_report["eventID"]

    def fetch():
        r = vtn_get("/reports", context.vtn_token)
        r.raise_for_status()
        return r.json()

    poll_until(
        fetch,
        lambda reports: any(
            r.get("clientName") == client and r.get("eventID") == event_id
            for r in reports
        ),
        timeout=30,
        interval=3,
        description=f"Report from '{client}' for event '{name}' in VTN",
    )
