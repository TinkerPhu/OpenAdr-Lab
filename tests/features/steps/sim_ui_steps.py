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


def _wait_slider_disabled(page, testid, expect_disabled: bool, timeout_ms=10000):
    """Poll via wait_for_function until the MUI Slider <input> native disabled attribute matches.

    MUI v5 Slider sets the native 'disabled' boolean attribute on the hidden
    <input type="range"> element when the slider is disabled. This is the same
    mechanism that @testing-library's toBeDisabled() checks in unit tests and is
    more reliable than checking the 'Mui-disabled' CSS class on the root span.

    slotProps={{ input: { "data-testid": ... } }} places the testid directly on
    the <input type="range"> element (confirmed via MUI v5 useSlotProps source).
    """
    condition = "true" if expect_disabled else "false"
    js = f"""(testid) => {{
        const wrapper = document.querySelector('[data-testid="' + testid + '"]');
        if (!wrapper) return false;
        const input = wrapper.querySelector('input[type="range"]');
        if (!input) return false;
        return input.disabled === {condition};
    }}"""
    page.wait_for_function(js, arg=testid, timeout=timeout_ms)


def _delete_all_vtn_events(token):
    """Delete all reports then all events visible to the given token from VTN.

    Reports must be deleted first — the report table has a FK on event_id with
    ON DELETE RESTRICT, so deleting events while reports reference them returns
    409 Conflict. The business token can delete reports (BusinessUser extractor).

    Note: openleadr-rs does not accept 'skip' / 'limit' pagination params on
    GET /events — calling without params returns all events.
    """
    # Delete all reports first to remove the FK constraint on event_id
    r = vtn_get("/reports", token)
    r.raise_for_status()
    for rpt in r.json():
        r = vtn_delete(f"/reports/{rpt['id']}", token)
        r.raise_for_status()

    # Now delete events
    r = vtn_get("/events", token)
    r.raise_for_status()
    for ev in r.json():
        r = vtn_delete(f"/events/{ev['id']}", token)
        r.raise_for_status()

    # Verify all events were actually removed before the caller proceeds
    r = vtn_get("/events", token)
    r.raise_for_status()
    remaining = r.json()
    assert len(remaining) == 0, (
        f"Events not deleted: {[e['eventName'] for e in remaining]}"
    )


# ── Step definitions ──────────────────────────────────────────────────────────

@when('I create a CHARGE_STATE_SETPOINT event "{name}" with value {value:g}')
def step_create_charge_setpoint_event(context, name, value):
    _create_charge_setpoint_event(context, name, value)


@given('no CHARGE_STATE_SETPOINT events are active on VEN-1')
def step_no_charge_setpoint_events(context):
    """Delete ALL events from VTN so the reactor can go IDLE.

    Simply deleting CHARGE_STATE_SETPOINT events is insufficient when the full
    test suite has accumulated IMPORT_CAP / EXPORT_CAP events from earlier
    scenarios — those also make isEvEventActive=true in the UI.
    """
    _delete_all_vtn_events(context.vtn_token)


@when('I wait for VEN-1 reactor to show mode "{mode}"')
@given('I wait for VEN-1 reactor to show mode "{mode}"')
def step_wait_reactor_mode(context, mode):
    def trace_shows_mode(trace):
        return len(trace) > 0 and trace[0].get("mode") == mode

    poll_until(
        _ven1_trace,
        trace_shows_mode,
        timeout=90,
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
    # data-testid is on FormControlLabel (<label>), which is always visible
    toggle = context.browser_page.wait_for_selector(
        '[data-testid="ev-charge-override-toggle"]', timeout=10000
    )
    assert toggle is not None, "EV charge rate override toggle not found in DOM"
    assert toggle.is_visible(), "EV charge rate override toggle is not visible"


@then('the EV charge rate override toggle is not shown')
def step_ev_override_toggle_not_shown(context):
    toggle = context.browser_page.query_selector('[data-testid="ev-charge-override-toggle"]')
    assert toggle is None or not toggle.is_visible(), \
        "EV charge rate override toggle should not be visible when no event is active"


# ── Slider state assertions ───────────────────────────────────────────────────

@then('the EV charge rate slider is disabled')
def step_ev_slider_disabled(context):
    try:
        _wait_slider_disabled(context.browser_page, "ev-charge-slider", expect_disabled=True)
    except Exception:
        raise AssertionError(
            "EV charge rate slider should be disabled when event is active and no override"
        )


@then('the EV charge rate slider is enabled')
def step_ev_slider_enabled(context):
    try:
        _wait_slider_disabled(context.browser_page, "ev-charge-slider", expect_disabled=False)
    except Exception:
        raise AssertionError("EV charge rate slider should be enabled")


# ── Caption assertions ────────────────────────────────────────────────────────

@then('the EV charge rate caption contains "{text}"')
def step_ev_caption_contains(context, text):
    caption = context.browser_page.query_selector('[data-testid="ev-charge-caption"]')
    assert caption is not None, "EV charge rate caption element not found"
    actual = caption.inner_text()
    assert text in actual, (
        f"EV charge rate caption should contain '{text}', got: '{actual}'"
    )


# ── Toggle interaction ────────────────────────────────────────────────────────

@when('I click the EV charge rate override toggle')
def step_click_ev_override_toggle(context):
    # data-testid is on FormControlLabel (<label>), which is fully visible and clickable
    toggle = context.browser_page.wait_for_selector(
        '[data-testid="ev-charge-override-toggle"]', timeout=5000
    )
    toggle.click()
    # Give React time to update state
    time.sleep(0.5)
