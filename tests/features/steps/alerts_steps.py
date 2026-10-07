"""Step definitions for grid alert events (WP3.1, BL-04)."""

from datetime import datetime, timedelta, timezone

from behave import given, then, when
from features.helpers.api_client import ven_get, ven2_get, vtn_post, vtn_delete
from features.helpers.wait import poll_until


@given('I create an alert event "{name}" of type "{alert_type}" for the saved program lasting {minutes:d} minutes')
def step_create_alert_event(context, name, alert_type, minutes):
    # Event-level intervalPeriod with a bare interval — the shape of User
    # Guide Example 8.1-1. The payload value is the spec's human-readable
    # string, not a number.
    start = datetime.now(timezone.utc)
    r = vtn_post(
        "/events",
        context.vtn_token,
        json={
            "programID": context.saved_program_id,
            "eventName": name,
            "intervalPeriod": {
                "start": start.strftime("%Y-%m-%dT%H:%M:%SZ"),
                "duration": f"PT{minutes}M",
            },
            "intervals": [{
                "id": 0,
                "payloads": [{
                    "type": alert_type,
                    "values": [f"test alert: {name}"],
                }],
            }],
        },
    )
    r.raise_for_status()
    context.alert_event_id = r.json().get("id")
    context.alert_start = start
    context.alert_minutes = minutes


@when("I delete the saved alert event")
def step_delete_alert_event(context):
    r = vtn_delete(f"/events/{context.alert_event_id}", context.vtn_token)
    r.raise_for_status()


@given("I create a SIMPLE event of level {level:d} for the saved program lasting {minutes:d} minutes")
def step_create_simple_event(context, level, minutes):
    # WP3.2: SIMPLE levels 0-3. Numeric payload, event-level window (same
    # shape as the alert events above).
    start = datetime.now(timezone.utc)
    r = vtn_post(
        "/events",
        context.vtn_token,
        json={
            "programID": context.saved_program_id,
            "eventName": f"simple-level-{level}",
            "intervalPeriod": {
                "start": start.strftime("%Y-%m-%dT%H:%M:%SZ"),
                "duration": f"PT{minutes}M",
            },
            "intervals": [{
                "id": 0,
                "payloads": [{"type": "SIMPLE", "values": [level]}],
            }],
        },
    )
    r.raise_for_status()
    context.simple_event_id = r.json().get("id")


@when("I delete the saved SIMPLE event")
def step_delete_simple_event(context):
    r = vtn_delete(f"/events/{context.simple_event_id}", context.vtn_token)
    r.raise_for_status()


@given('I create a capacity event of type "{ptype}" with {kw:f} kW for the saved program lasting {minutes:d} minutes')
def step_create_capacity_event(context, ptype, kw, minutes):
    # WP3.3: capacity subscription/reservation events with a starting-now
    # window (same event-level shape as the alert/SIMPLE steps above).
    start = datetime.now(timezone.utc)
    r = vtn_post(
        "/events",
        context.vtn_token,
        json={
            "programID": context.saved_program_id,
            "eventName": f"capacity-{ptype.lower().replace('_', '-')}",
            "intervalPeriod": {
                "start": start.strftime("%Y-%m-%dT%H:%M:%SZ"),
                "duration": f"PT{minutes}M",
            },
            "intervals": [{
                "id": 0,
                "payloads": [{"type": ptype, "values": [kw]}],
            }],
        },
    )
    r.raise_for_status()
    context.capacity_event_id = r.json().get("id")


@when("I delete the saved capacity event")
def step_delete_capacity_event(context):
    r = vtn_delete(f"/events/{context.capacity_event_id}", context.vtn_token)
    r.raise_for_status()


@then("the VEN net site power reaches {target_kw:f} kW within {seconds:d} seconds with tolerance {tol:f}")
def step_net_power_reaches(context, target_kw, seconds, tol):
    # WP3.4: DISPATCH_SETPOINT steers the battery so net site power hits the
    # commanded value. /sim grid.net_power_w is in W, positive = import.
    def fetch():
        resp = ven_get("/sim")
        if not resp.ok:
            return None
        return resp.json()

    def at_target(sim):
        if sim is None:
            return False
        net_kw = sim.get("grid", {}).get("net_power_w", 0.0) / 1000.0
        return abs(net_kw - target_kw) <= tol

    poll_until(
        fetch,
        at_target,
        timeout=seconds,
        interval=3,
        description=f"net site power ≈ {target_kw} kW (±{tol})",
    )


@then("the VEN ev-session has target_soc {soc:f} within {seconds:d} seconds")
def step_ev_session_target(context, soc, seconds):
    def fetch():
        # `/ev-session` returns the whole queue (ev-session-queue): an EV may hold
        # several sessions, so this looks for one with the expected target rather
        # than assuming a single object.
        resp = ven_get("/ev-session")
        if not resp.ok:
            return None
        try:
            body = resp.json()
        except ValueError:
            return None
        if not isinstance(body, list):
            return None
        match = [s for s in body if abs(s.get("target_soc", -1) - soc) < 1e-6]
        return match[0] if match else None

    poll_until(
        fetch,
        lambda s: s is not None and abs(s.get("target_soc", -1) - soc) < 1e-6,
        timeout=seconds,
        interval=2,
        description=f"EvSession with target_soc {soc}",
    )


def _fetch_plan():
    resp = ven_get("/plan")
    if not resp.ok:
        return None
    body = resp.json()
    return body if isinstance(body, dict) else None


@when("I wait for the VEN /plan to have at least one slot with import_cap_kw at most {cap:f}")
def step_wait_any_slot_capped(context, cap):
    def any_capped(plan):
        if plan is None:
            return False
        return any(
            slot.get("import_cap_kw", float("inf")) <= cap + 0.01
            for slot in plan.get("slots", [])
        )

    context.ven_plan = poll_until(
        _fetch_plan,
        any_capped,
        timeout=300,
        interval=5,
        description=f"VEN /plan has at least one slot with import_cap_kw ≤ {cap}",
    )


@when("I wait for the VEN /plan to have no slot with import_cap_kw below {cap:f}")
def step_wait_no_slot_capped(context, cap):
    def none_capped(plan):
        if plan is None:
            return False
        slots = plan.get("slots", [])
        if not slots:
            return False
        return all(slot.get("import_cap_kw", float("inf")) >= cap for slot in slots)

    context.ven_plan = poll_until(
        _fetch_plan,
        none_capped,
        timeout=300,
        interval=5,
        description=f"VEN /plan has no slot with import_cap_kw < {cap}",
    )


@then('every plan slot overlapping the next {minutes:d} minutes has import_cap_kw at most {cap:f}')
def step_assert_window_slots_capped(context, minutes, cap):
    plan = context.ven_plan
    now = datetime.now(timezone.utc)
    window_end = now + timedelta(minutes=minutes)
    checked = 0
    for slot in plan.get("slots", []):
        slot_start = datetime.fromisoformat(slot["start"].replace("Z", "+00:00"))
        if slot_start >= window_end:
            continue
        # Slot overlaps [now, window_end) — must be clamped.
        slot_cap = slot.get("import_cap_kw", float("inf"))
        assert slot_cap <= cap + 0.01, (
            f"slot starting {slot['start']} inside the alert window has "
            f"import_cap_kw={slot_cap}, expected ≤ {cap}"
        )
        checked += 1
    assert checked > 0, "no plan slots overlapped the alert window at all"


@given("I create a SIMPLE event of level {level:d} for the saved program lasting {minutes:d} minutes with a randomizeStart of {window:d} minutes")
def step_create_randomized_simple_event(context, level, minutes, window):
    """R-86: the declared start is now; each VEN may begin up to `window` minutes later."""
    context.randomized_declared_start = datetime.now(timezone.utc).replace(microsecond=0)
    context.randomize_window_s = window * 60
    r = vtn_post(
        "/events",
        context.vtn_token,
        json={
            "programID": context.saved_program_id,
            "eventName": f"simple-randomized-{level}",
            "intervalPeriod": {
                "start": context.randomized_declared_start.strftime("%Y-%m-%dT%H:%M:%SZ"),
                "duration": f"PT{minutes}M",
                "randomizeStart": f"PT{window}M",
            },
            "intervals": [{"id": 0, "payloads": [{"type": "SIMPLE", "values": [level]}]}],
        },
    )
    r.raise_for_status()
    context.simple_event_id = r.json().get("id")


def _simple_window_start(get, event_id):
    r = get("/signals")
    if not r.ok:
        return None
    for w in r.json().get("simple", []):
        if w.get("event_id") == event_id:
            return datetime.fromisoformat(w["start"].replace("Z", "+00:00"))
    return None


@when("both VENs report a SIMPLE window for the saved event")
def step_both_vens_report_simple_window(context):
    def fetch():
        return (_simple_window_start(ven_get, context.simple_event_id),
                _simple_window_start(ven2_get, context.simple_event_id))

    context.ven_starts = poll_until(
        fetch, lambda pair: pair[0] is not None and pair[1] is not None,
        timeout=90, interval=3, description="both VENs to report the SIMPLE window",
    )


@then("the two VENs begin the saved event at different times within the randomizeStart window")
def step_vens_begin_at_different_times(context):
    declared = context.randomized_declared_start
    window_s = context.randomize_window_s
    first, second = context.ven_starts
    assert first != second, f"both VENs begin at {first}: nothing was staggered"
    for name, start in (("ven-1", first), ("ven-2", second)):
        delay_s = (start - declared).total_seconds()
        assert 0 <= delay_s < window_s, (
            f"{name} begins {delay_s:.0f} s after the declared start, outside [0, {window_s}) s"
        )
