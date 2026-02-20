"""Step definitions for VEN simulation UI override scenarios."""

import time
from behave import given, when, then
from features.helpers.api_client import vtn_post, vtn_delete, vtn_get, ven_get
from features.helpers.wait import poll_until
from features.helpers.ui import tid


# ── Helpers ───────────────────────────────────────────────────────────────────

def _ven1_trace():
    r = ven_get("/trace?limit=10")
    r.raise_for_status()
    return r.json()


def _create_charge_setpoint_event(context, event_name, value_kw):
    """Create an immediately-active CHARGE_STATE_SETPOINT event on VEN-1."""
    from features.helpers.api_client import vtn_post
    body = {
        "programID": context.saved_program_id,
        "eventName": event_name,
        "priority": 0,
        "intervals": [
            {"id": 0, "payloads": [{"type": "CHARGE_STATE_SETPOINT", "values": [value_kw]}]}
        ],
    }
    r = vtn_post("/events", context.vtn_token, json=body)
    r.raise_for_status()
    context.created_event_id = r.json()["id"]
    return r.json()


# ── Step definitions ──────────────────────────────────────────────────────────

@when('I create a CHARGE_STATE_SETPOINT event "{name}" with value {value:g}')
def step_create_charge_setpoint_event(context, name, value):
    _create_charge_setpoint_event(context, name, value)


@given('no CHARGE_STATE_SETPOINT events are active on VEN-1')
def step_no_charge_setpoint_events(context):
    """Delete any lingering CHARGE_STATE_SETPOINT events from previous scenarios."""
    events = ven_get("/events").json()
    charge_events = [
        e for e in events
        if any(
            iv.get("payloads", [{}])[0].get("type") == "CHARGE_STATE_SETPOINT"
            for iv in e.get("intervals", [])
        )
    ]
    for ev in charge_events:
        try:
            vtn_delete(f"/events/{ev['id']}", context.vtn_token)
        except Exception:
            pass  # best effort


@when('I wait for VEN-1 reactor to show mode "{mode}"')
@given('I wait for VEN-1 reactor to show mode "{mode}"')
def step_wait_reactor_mode(context, mode):
    def trace_shows_mode(trace):
        return len(trace) > 0 and trace[0].get("mode") == mode

    poll_until(
        _ven1_trace,
        trace_shows_mode,
        timeout=30,
        interval=2,
        description=f"VEN-1 reactor mode == '{mode}'",
    )


@when('I open the VEN-1 simulation UI')
@given('I open the VEN-1 simulation UI')
def step_open_ven_sim_ui(context):
    context.ven_ui.open()
    context.ven_ui.go_simulation()


# ── Override toggle assertions ────────────────────────────────────────────────

@then('the EV charge rate override toggle is shown')
def step_ev_override_toggle_shown(context):
    toggle = context.browser_page.query_selector(f'[data-testid="ev-charge-override-toggle"]')
    assert toggle is not None, "EV charge rate override toggle not found in DOM"
    assert toggle.is_visible(), "EV charge rate override toggle is not visible"


@then('the EV charge rate override toggle is not shown')
def step_ev_override_toggle_not_shown(context):
    toggle = context.browser_page.query_selector(f'[data-testid="ev-charge-override-toggle"]')
    assert toggle is None or not toggle.is_visible(), \
        "EV charge rate override toggle should not be visible when no event is active"


# ── Slider state assertions ───────────────────────────────────────────────────

@then('the EV charge rate slider is disabled')
def step_ev_slider_disabled(context):
    slider = context.browser_page.query_selector(f'[data-testid="ev-charge-slider"]')
    assert slider is not None, "EV charge rate slider not found"
    assert slider.get_attribute("disabled") is not None or slider.is_disabled(), \
        "EV charge rate slider should be disabled when event is active and no override"


@then('the EV charge rate slider is enabled')
def step_ev_slider_enabled(context):
    slider = context.browser_page.query_selector(f'[data-testid="ev-charge-slider"]')
    assert slider is not None, "EV charge rate slider not found"
    assert not slider.is_disabled(), \
        "EV charge rate slider should be enabled"


# ── Caption assertions ────────────────────────────────────────────────────────

@then('the EV charge rate caption contains "{text}"')
def step_ev_caption_contains(context, text):
    caption = context.browser_page.query_selector(f'[data-testid="ev-charge-caption"]')
    assert caption is not None, "EV charge rate caption element not found"
    actual = caption.inner_text()
    assert text in actual, (
        f"EV charge rate caption should contain '{text}', got: '{actual}'"
    )


# ── Toggle interaction ────────────────────────────────────────────────────────

@when('I click the EV charge rate override toggle')
def step_click_ev_override_toggle(context):
    toggle = context.browser_page.wait_for_selector(
        f'[data-testid="ev-charge-override-toggle"]', timeout=5000
    )
    toggle.click()
    # Give React time to update state
    time.sleep(0.5)
