"""Step definitions for VEN simulation UI override scenarios."""

import time
from behave import given, when, then
from features.helpers.api_client import vtn_post, vtn_delete, vtn_get, ven_get, ven_post
from features.helpers.wait import poll_until
from features.helpers.ui import tid


# ── Helpers ───────────────────────────────────────────────────────────────────

def _ven1_trace():
    r = ven_get("/trace/events?limit=10")
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
    """Wait until the MUI Slider inside the Box wrapper has the expected disabled state.

    The Slider is wrapped in <Box data-testid={sliderTestId}> in Simulation.tsx.
    MUI sets the native 'disabled' attribute on the hidden <input type="range">
    when disabled. We use Playwright wait_for_selector with a CSS :disabled /
    :not([disabled]) pseudo-class scoped to the wrapper — no custom JavaScript
    required, and state='attached' works on hidden elements.
    """
    if expect_disabled:
        selector = f'[data-testid="{testid}"] input[type="range"]:disabled'
    else:
        selector = f'[data-testid="{testid}"] input[type="range"]:not([disabled])'
    page.wait_for_selector(selector, state="attached", timeout=timeout_ms)


def _delete_all_vtn_events(token):
    """Delete all reports then all events visible to the given token from VTN.

    Reports must be deleted first — the report table has a FK on event_id with
    ON DELETE RESTRICT, so deleting events while reports reference them returns
    409 Conflict. The business token can delete reports (BusinessUser extractor).

    VEN-1 runs at ~1Hz and may submit a new report between our report-delete and
    event-delete passes. We retry the whole loop up to 3 times to handle this race.

    Note: openleadr-rs does not accept 'skip' / 'limit' pagination params on
    GET /events — calling without params returns all events.
    """
    for attempt in range(3):
        # Delete all reports to remove the FK constraint on event_id
        r = vtn_get("/reports", token)
        r.raise_for_status()
        for rpt in r.json():
            r2 = vtn_delete(f"/reports/{rpt['id']}", token)
            if r2.status_code not in (200, 204, 404):
                r2.raise_for_status()

        # Delete all events; if a 409 is returned a new report snuck in — retry
        r = vtn_get("/events", token)
        r.raise_for_status()
        conflict = False
        for ev in r.json():
            r2 = vtn_delete(f"/events/{ev['id']}", token)
            if r2.status_code == 409:
                conflict = True
                break
            r2.raise_for_status()

        if not conflict:
            break
        time.sleep(1)  # give VEN time to finish submitting the report
    else:
        raise AssertionError("Could not delete all events after 3 attempts (persistent 409)")

    # Verify all events were actually removed before the caller proceeds
    r = vtn_get("/events", token)
    r.raise_for_status()
    remaining = r.json()
    assert len(remaining) == 0, (
        f"Events not deleted: {[e['eventName'] for e in remaining]}"
    )


# ── Step definitions ──────────────────────────────────────────────────────────

@given('the VEN-1 sim overrides are reset')
def step_reset_ven_overrides(context):
    """POST empty UserOverrides to VEN-1 to clear any persisted override values.

    Without this reset, environmental overrides set in scenario N bleed into
    scenario N+1, causing unexpected sim state in subsequent scenarios.
    """
    r = ven_post("/sim/override", json={})
    r.raise_for_status()


@when('I create a CHARGE_STATE_SETPOINT event "{name}" with value {value:g}')
def step_create_charge_setpoint_event(context, name, value):
    _create_charge_setpoint_event(context, name, value)


@given('no CHARGE_STATE_SETPOINT events are active on VEN-1')
def step_no_charge_setpoint_events(context):
    """Delete ALL events from VTN so the controller returns to no-event state.

    Simply deleting CHARGE_STATE_SETPOINT events is insufficient when the full
    test suite has accumulated IMPORT_CAP / EXPORT_CAP events from earlier
    scenarios — those also make isEvEventActive=true in the UI.
    """
    _delete_all_vtn_events(context.vtn_token)




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
