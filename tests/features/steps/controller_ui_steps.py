"""Step definitions for VEN Controller Dashboard UI scenarios."""

import time
from behave import given, when, then
from features.helpers.api_client import vtn_post, vtn_get, ven_get
from features.helpers.wait import poll_until
from features.helpers.ui import tid

# Re-use the shared delete helper from sim_ui_steps
from features.steps.sim_ui_steps import _delete_all_vtn_events


# ── Helpers ───────────────────────────────────────────────────────────────────

def _get_or_create_program(token, ven_name="ven-1"):
    """Return the ID of a program targeting ven_name, creating one if needed."""
    r = vtn_get("/programs", token)
    r.raise_for_status()
    for prog in r.json():
        targets = prog.get("targets") or []
        for t in targets:
            if t.get("type") == "VEN_NAME" and ven_name in (t.get("values") or []):
                return prog["id"]
    # Create a minimal program targeting this VEN
    body = {
        "programName": f"controller-ui-test-{int(time.time())}",
        "targets": [{"type": "VEN_NAME", "values": [ven_name]}],
    }
    r = vtn_post("/programs", token, json=body)
    r.raise_for_status()
    return r.json()["id"]


def _create_price_event(token, program_id, import_rate, ven_name="ven-1"):
    """POST a 2-hour PRICE event targeting ven_name to VTN."""
    body = {
        "programID": program_id,
        "eventName": f"ctrl-ui-price-{int(time.time())}",
        "priority": 0,
        "targets": [{"type": "VEN_NAME", "values": [ven_name]}],
        "intervals": [
            {
                "id": 0,
                "payloads": [{"type": "PRICE", "values": [import_rate]}],
            }
        ],
    }
    r = vtn_post("/events", token, json=body)
    r.raise_for_status()
    return r.json()


def _ven1_rates():
    r = ven_get("/rates")
    r.raise_for_status()
    return r.json()


def _ven1_plan():
    r = ven_get("/plan")
    r.raise_for_status()
    return r.json()


# ── Step definitions ──────────────────────────────────────────────────────────

@given("no price events are active on VEN-1")
def step_no_price_events(context):
    """Delete all VTN events, then wait for VEN-1 /rates to return empty snapshots."""
    _delete_all_vtn_events(context.vtn_token)

    def rates_empty(rates):
        snaps = rates.get("snapshots") if isinstance(rates, dict) else None
        return not snaps  # None or empty list

    poll_until(
        _ven1_rates,
        rates_empty,
        timeout=30,
        interval=2,
        description="VEN-1 /rates returns empty snapshots",
    )


@given("the VEN-1 controller has produced at least one plan")
def step_ven1_has_plan(context):
    """Poll GET /plan on VEN-1 until the response is a non-null plan object."""
    poll_until(
        _ven1_plan,
        lambda p: p is not None and isinstance(p, dict) and p.get("trigger") is not None,
        timeout=60,
        interval=2,
        description="VEN-1 /plan returns a non-null plan",
    )


@given("a PRICE event with import rate {rate:f} EUR/kWh is active on VEN-1")
def step_create_price_event(context, rate):
    """POST a PRICE event to VTN targeting VEN-1 and store its ID for cleanup."""
    program_id = _get_or_create_program(context.vtn_token, ven_name="ven-1")
    context.saved_program_id = program_id
    event = _create_price_event(context.vtn_token, program_id, rate, ven_name="ven-1")
    context.created_event_id = event["id"]


@given("I wait for VEN-1 to have rate data")
def step_wait_for_rate_data(context):
    """Poll GET /rates until snapshots is non-null and non-empty."""
    def rates_populated(rates):
        snaps = rates.get("snapshots") if isinstance(rates, dict) else None
        return snaps is not None and len(snaps) > 0

    poll_until(
        _ven1_rates,
        rates_populated,
        timeout=30,
        interval=2,
        description="VEN-1 /rates has non-empty snapshots",
    )


@when("I open the VEN-1 controller UI")
@given("I open the VEN-1 controller UI")
def step_open_ven_controller_ui(context):
    context.ven_ui.open()
    context.ven_ui.go_controller()


@then("the controller rate chart empty state is visible")
def step_rate_chart_empty_visible(context):
    el = context.browser_page.wait_for_selector(
        tid("controller-rate-chart-empty"), timeout=10000
    )
    assert el is not None and el.is_visible(), (
        "controller-rate-chart-empty not visible"
    )


@then("the controller packets table section is visible")
def step_packets_table_visible(context):
    el = context.browser_page.wait_for_selector(
        tid("controller-packets-table"), timeout=10000
    )
    assert el is not None and el.is_visible(), (
        "controller-packets-table not visible"
    )


@then("the controller plan card does not show an error")
def step_plan_card_no_error(context):
    content = context.browser_page.content()
    assert "TypeError" not in content, "Page contains a TypeError — crash detected"
    assert "Cannot read properties" not in content, (
        "Page contains a JS crash message"
    )
    # Also verify the packets table is visible (page rendered fully)
    el = context.browser_page.query_selector(tid("controller-packets-table"))
    assert el is not None and el.is_visible(), (
        "controller-packets-table not visible after plan card check"
    )


@then("the controller rate chart with data is visible")
def step_rate_chart_data_visible(context):
    el = context.browser_page.wait_for_selector(
        tid("controller-rate-chart"), timeout=10000
    )
    assert el is not None and el.is_visible(), (
        "controller-rate-chart not visible"
    )
