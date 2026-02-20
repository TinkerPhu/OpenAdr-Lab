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


def _is_slider_disabled(page, testid):
    """Check if a MUI Slider is visually disabled.

    MUI v5 Slider sets a `Mui-disabled` CSS class on the root <span> element
    when disabled, rather than putting `disabled` / `aria-disabled` on the inner
    <input type="range"> element.  Playwright's is_disabled() and
    get_attribute("disabled") both miss this pattern.  We use page.evaluate()
    to inspect the actual DOM.
    """
    result = page.evaluate(f"""() => {{
        const input = document.querySelector('[data-testid="{testid}"]');
        if (!input) return {{"found": false, "disabled": false}};
        // Check both the input itself and its closest MuiSlider ancestor
        const root = input.closest('[class*="MuiSlider-root"]') || input.parentElement;
        const rootMuiDisabled = root ? root.classList.contains("Mui-disabled") : false;
        const inputDisabled = input.hasAttribute("disabled");
        const inputAriaDisabled = input.getAttribute("aria-disabled") === "true";
        return {{
            "found": true,
            "disabled": inputDisabled || inputAriaDisabled || rootMuiDisabled,
            "inputDisabled": inputDisabled,
            "inputAriaDisabled": inputAriaDisabled,
            "rootMuiDisabled": rootMuiDisabled
        }};
    }}""")
    return bool(result.get("disabled", False))


def _delete_all_vtn_events(token):
    """Delete all events visible to any-business from VTN (with pagination)."""
    skip = 0
    limit = 200
    deleted = 0
    while True:
        r = vtn_get("/events", token, params={"skip": skip, "limit": limit})
        r.raise_for_status()
        events = r.json()
        if not events:
            break
        for ev in events:
            vtn_delete(f"/events/{ev['id']}", token)
            deleted += 1
        if len(events) < limit:
            break
        skip += limit
    return deleted


# ── Step definitions ──────────────────────────────────────────────────────────

@when('I create a CHARGE_STATE_SETPOINT event "{name}" with value {value:g}')
def step_create_charge_setpoint_event(context, name, value):
    _create_charge_setpoint_event(context, name, value)


@given('no CHARGE_STATE_SETPOINT events are active on VEN-1')
def step_no_charge_setpoint_events(context):
    """Delete ALL events from VTN so the reactor can go IDLE.

    Simply deleting CHARGE_STATE_SETPOINT events is insufficient when the full
    test suite has accumulated IMPORT_CAP / EXPORT_CAP events from earlier
    scenarios — those also make isEvEventActive=true in the UI. Deleting every
    event guarantees a clean slate so the reactor transitions to IDLE.
    Uses pagination (limit=200) to handle large test databases.
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
        timeout=60,
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
    # data-testid is on the FormControlLabel (<label>), which is always visible
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
    # Wait for slider to be present, then evaluate its disabled state via Mui class
    context.browser_page.wait_for_selector(
        '[data-testid="ev-charge-slider"]', timeout=5000
    )
    assert _is_slider_disabled(context.browser_page, "ev-charge-slider"), \
        "EV charge rate slider should be disabled when event is active and no override"


@then('the EV charge rate slider is enabled')
def step_ev_slider_enabled(context):
    context.browser_page.wait_for_selector(
        '[data-testid="ev-charge-slider"]', timeout=5000
    )
    assert not _is_slider_disabled(context.browser_page, "ev-charge-slider"), \
        "EV charge rate slider should be enabled"


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
