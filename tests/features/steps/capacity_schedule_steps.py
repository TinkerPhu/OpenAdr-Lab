"""Step definitions for the time-aware capacity schedule (GB-48): limits, subscriptions, reservations."""

import time
from datetime import datetime, timedelta, timezone

from behave import given, then, when
from features.helpers.api_client import ven_get, vtn_delete, vtn_post
from features.helpers.wait import poll_until


def _iso(dt):
    return dt.strftime("%Y-%m-%dT%H:%M:%SZ")


def _post_event(context, body):
    r = vtn_post("/events", context.vtn_token, json=body)
    r.raise_for_status()
    context.schedule_event_id = r.json().get("id")
    # A scenario may announce several events; the scenario cleanup deletes every one in uc_events.
    if not isinstance(getattr(context, "uc_events", None), dict):
        context.uc_events = {}
    context.uc_events[body["eventName"]] = {"id": context.schedule_event_id}


def _announce(context, payload_type, kw, minutes, duration, name):
    start = datetime.now(timezone.utc) + timedelta(minutes=minutes)
    context.limit_start = start
    context.limit_end = start + timedelta(minutes=duration)
    _post_event(context, {
        "programID": context.saved_program_id,
        "eventName": name,
        "intervalPeriod": {"start": _iso(start), "duration": f"PT{duration}M"},
        "intervals": [{"id": 0, "payloads": [{"type": payload_type, "values": [kw]}]}],
    })


@given("I announce an import capacity limit of {kw:g} kW starting in {minutes:d} minutes for {duration:d} minutes")
def step_announce_limit(context, kw, minutes, duration):
    _announce(context, "IMPORT_CAPACITY_LIMIT", kw, minutes, duration, "announced-import-limit")


@given("I announce an import capacity reservation of {kw:g} kW starting in {minutes:d} minutes for {duration:d} minutes")
@when("I announce an import capacity reservation of {kw:g} kW starting in {minutes:d} minutes for {duration:d} minutes")
def step_announce_reservation(context, kw, minutes, duration):
    _announce(context, "IMPORT_CAPACITY_RESERVATION", kw, minutes, duration, "announced-import-reservation")


@given("I announce an import capacity subscription of {kw:g} kW starting in {minutes:d} minutes for {duration:d} minutes")
def step_announce_subscription(context, kw, minutes, duration):
    _announce(context, "IMPORT_CAPACITY_SUBSCRIPTION", kw, minutes, duration, "announced-import-subscription")


@given("I send a dynamic operating envelope of {first:f} kW then {second:f} kW in contiguous {minutes:d}-minute intervals starting in {offset:d} minutes")
def step_send_doe(context, first, second, minutes, offset):
    # OpenADR 3.1 User Guide Example 8.10.1-1: one event-level intervalPeriod,
    # intervals without their own — contiguous per §7.3.
    start = datetime.now(timezone.utc).replace(second=0, microsecond=0) + timedelta(minutes=offset)
    context.doe_start = start
    context.doe_minutes = minutes
    _post_event(context, {
        "programID": context.saved_program_id,
        "eventName": "doe-contiguous",
        "intervalPeriod": {"start": _iso(start), "duration": f"PT{minutes}M"},
        "intervals": [
            {"id": 0, "payloads": [{"type": "IMPORT_CAPACITY_LIMIT", "values": [first]}]},
            {"id": 1, "payloads": [{"type": "IMPORT_CAPACITY_LIMIT", "values": [second]}]},
        ],
    })


@when("I delete the schedule event")
def step_delete_schedule_event(context):
    vtn_delete(f"/events/{context.schedule_event_id}", context.vtn_token).raise_for_status()


def _plan():
    r = ven_get("/plan")
    body = r.json() if r.ok else None
    return body if isinstance(body, dict) else None


def _parse(ts):
    return datetime.fromisoformat(ts.replace("Z", "+00:00"))


@then("the VEN plan caps only the slots overlapping the announced window at {kw:g} kW")
def step_plan_caps_window_only(context, kw):
    def capped_correctly(plan):
        if not plan or not plan.get("slots"):
            return False
        overlapping, other = [], []
        for slot in plan["slots"]:
            start, end = _parse(slot["start"]), _parse(slot["end"])
            overlaps = start < context.limit_end and context.limit_start < end
            (overlapping if overlaps else other).append(slot["import_cap_kw"])
        return (
            overlapping
            and all(cap <= kw + 1e-6 for cap in overlapping)
            and other
            and all(cap > kw + 1e-6 for cap in other)
        )

    poll_until(_plan, capped_correctly, timeout=300, interval=5,
               description=f"only slots overlapping the window capped at {kw} kW")


@then("the VEN plan caps every slot outside the announced window at {kw:g} kW")
def step_plan_caps_outside_window(context, kw):
    # The limit window is covered by the (tighter) limit; every other slot falls back to the
    # subscription, which is the default once the limit schedule is exhausted (§8.10.1).
    def capped_correctly(plan):
        if not plan or not plan.get("slots"):
            return False
        outside = [
            slot["import_cap_kw"]
            for slot in plan["slots"]
            if not (_parse(slot["start"]) < context.limit_end and context.limit_start < _parse(slot["end"]))
        ]
        return bool(outside) and all(abs(cap - kw) < 1e-6 for cap in outside)

    poll_until(_plan, capped_correctly, timeout=300, interval=5,
               description=f"every slot outside the limit window capped at {kw} kW")


@then("the VEN reports no import limit in force")
def step_no_limit_now(context):
    r = ven_get("/capacity")
    r.raise_for_status()
    assert r.json().get("import_limit_kw") is None, r.json()


@then("the VEN reports an import limit of {kw:f} kW in force within {seconds:d} seconds")
def step_limit_in_force(context, kw, seconds):
    poll_until(
        lambda: ven_get("/capacity").json(),
        lambda cap: cap.get("import_limit_kw") is not None and abs(cap["import_limit_kw"] - kw) < 1e-6,
        timeout=seconds,
        interval=5,
        description=f"/capacity import_limit_kw == {kw}",
    )


@then("the VEN reports an import limit of {kw:g} kW on the first read after it starts")
def step_limit_in_force_on_first_read(context, kw):
    # The limit is read from the schedule at the time of the request, not from a value stored at
    # the last event poll: once the schedule holds the event, the first read at or after its start
    # must already show it.
    def holds_event(rows):
        return any(r.get("import_limit_event_id") == context.schedule_event_id for r in rows)

    poll_until(
        lambda: ven_get("/capacity/schedule").json(),
        holds_event,
        timeout=90,
        interval=2,
        description="/capacity/schedule holds the announced event",
    )
    wait_s = (context.limit_start - datetime.now(timezone.utc)).total_seconds()
    if wait_s > 0:
        time.sleep(wait_s + 1)
    cap = ven_get("/capacity").json()
    assert cap.get("import_limit_kw") is not None and abs(cap["import_limit_kw"] - kw) < 1e-6, cap


def _import_request_kw():
    r = ven_get("/signals")
    r.raise_for_status()
    request = r.json().get("reservation_request")
    return None if request is None else request["import_kw"]


@given("I note the VEN's import reservation request")
def step_note_import_request(context):
    poll_until(_import_request_kw, lambda kw: kw is not None and kw > 0, timeout=120, interval=5,
               description="a positive import reservation request under the announced subscription")
    context.import_request_before_kw = _import_request_kw()


@then("the VEN's import reservation request is {kw:g} kW lower within {seconds:d} seconds")
def step_import_request_lower(context, kw, seconds):
    # The request is measured against the contracted allowance *now*, so a reservation window
    # shrinks it while it runs.
    poll_until(
        _import_request_kw,
        lambda now_kw: now_kw is not None and abs((context.import_request_before_kw - now_kw) - kw) < 0.02,
        timeout=seconds,
        interval=5,
        description=f"import reservation request {kw} kW below {context.import_request_before_kw}",
    )


@then("the VEN capacity schedule shows {first:f} kW then {second:f} kW back to back")
def step_schedule_contiguous(context, first, second):
    def entries():
        r = ven_get("/capacity/schedule")
        return r.json() if r.ok else []

    def contiguous(rows):
        mine = sorted(
            (_parse(r["interval_start"]), r["import_limit_kw"])
            for r in rows
            if r.get("import_limit_event_id") == context.schedule_event_id
        )
        step = timedelta(minutes=context.doe_minutes)
        return mine == [(context.doe_start, first), (context.doe_start + step, second)]

    poll_until(entries, contiguous, timeout=90, interval=5,
               description="DOE intervals back to back in /capacity/schedule")
