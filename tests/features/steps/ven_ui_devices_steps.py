"""Step definitions for the Devices page baseline-override BDD scenarios (BL-42).

Reuses the generic testid-driven steps already defined in planner_ui_steps.py
("I see an element with testid ...", "I click the element with testid ...")
per design.md decision 6 — only genuinely new interactions get new step defs
here: navigating to the Devices page, resetting the baseline override via the
VEN API, filling a field by testid, and asserting a field's value / disabled
state / visible text.
"""

from behave import given, when, then
from features.helpers.ui import tid
from features.helpers.api_client import ven_delete


# ── Background ────────────────────────────────────────────────────────────────

@given("no baseline override is active")
def step_no_baseline_override(context):
    """Clear any baseline override left over from a previous scenario/run."""
    r = ven_delete("/baseline-override")
    assert r.status_code in (200, 204, 404), (
        f"Failed to reset baseline override: {r.status_code} {r.text}"
    )


@when("I navigate to the Devices page")
def step_navigate_devices(context):
    context.ven_ui.go_devices()


# ── Field interactions ────────────────────────────────────────────────────────

@when('I fill the field "{testid}" with "{value}"')
def step_fill_field(context, testid, value):
    context.browser_page.fill(tid(testid), value)


@when('I fill the datetime-local field "{testid}" with "{value}"')
def step_fill_datetime_local(context, testid, value):
    context.browser_page.fill(tid(testid), value)


@then('the field "{testid}" has value "{value}"')
def step_field_has_value(context, testid, value):
    el = context.browser_page.wait_for_selector(tid(testid), timeout=45000)
    assert el is not None, f'Field "{testid}" not found'
    actual = el.input_value()
    assert actual == value, f'Field "{testid}" expected value "{value}", got "{actual}"'


@then('the element with testid "{testid}" is disabled')
def step_element_disabled(context, testid):
    el = context.browser_page.wait_for_selector(tid(testid), timeout=45000)
    assert el is not None, f'Element "{testid}" not found'
    assert el.is_disabled(), f'Element "{testid}" expected to be disabled'


@then('I see the text "{text}"')
def step_see_text(context, text):
    context.browser_page.wait_for_selector(f'text="{text}"', timeout=45000)
