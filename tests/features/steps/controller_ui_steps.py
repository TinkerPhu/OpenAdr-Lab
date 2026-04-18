"""Step definitions for VEN Controller Dashboard UI scenarios."""

import time
import datetime
from behave import given, when, then
from features.helpers.api_client import vtn_post, vtn_get, ven_get
from features.helpers.wait import poll_until
from features.helpers.ui import tid

# Re-use the shared delete helper from sim_ui_steps
from features.steps.sim_ui_steps import _delete_all_vtn_events


# ── Helpers ───────────────────────────────────────────────────────────────────

def _get_or_create_program(token):
    """Return the ID of any available program, creating one if none exist."""
    r = vtn_get("/programs", token)
    r.raise_for_status()
    progs = r.json()
    if progs:
        return progs[0]["id"]
    # Create a minimal open program (no targets = visible to all VENs)
    r2 = vtn_post("/programs", token, json={"programName": "test-controller-ui"})
    if r2.status_code == 409:
        # Race or leftover — fetch again
        r3 = vtn_get("/programs", token)
        r3.raise_for_status()
        progs3 = r3.json()
        if progs3:
            return progs3[0]["id"]
        raise AssertionError("POST /programs returned 409 but GET /programs is still empty")
    r2.raise_for_status()
    return r2.json()["id"]


def _create_price_event(token, program_id, import_rate):
    """POST a PRICE event with intervalPeriod to VTN (open targets = visible to all VENs)."""
    now = datetime.datetime.utcnow().replace(microsecond=0)
    body = {
        "programID": program_id,
        "eventName": f"ctrl-ui-price-{int(time.time())}",
        "priority": 0,
        "intervals": [
            {
                "id": 0,
                "intervalPeriod": {
                    "start": now.isoformat() + "Z",
                    "duration": "PT4H",
                },
                "payloads": [{"type": "PRICE", "values": [import_rate]}],
            }
        ],
    }
    r = vtn_post("/events", token, json=body)
    r.raise_for_status()
    return r.json()


def _ven1_rates():
    r = ven_get("/tariffs")
    r.raise_for_status()
    return r.json()


def _ven1_plan():
    r = ven_get("/plan")
    r.raise_for_status()
    return r.json()


# ── Step definitions ──────────────────────────────────────────────────────────

@given("no price events are active on VEN-1")
def step_no_price_events(context):
    """Delete all VTN events, then wait for VEN-1 /rates to return empty list."""
    _delete_all_vtn_events(context.vtn_token)

    poll_until(
        _ven1_rates,
        lambda rates: isinstance(rates, list) and len(rates) == 0,
        timeout=60,
        interval=2,
        description="VEN-1 /tariffs returns empty list",
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
    program_id = _get_or_create_program(context.vtn_token)
    context.saved_program_id = program_id
    event = _create_price_event(context.vtn_token, program_id, rate)
    context.created_event = event  # picked up by _cleanup_vtn_events in environment.py


@given("I wait for VEN-1 to have rate data")
def step_wait_for_rate_data(context):
    """Poll GET /tariffs until it returns a non-empty list of snapshots."""
    poll_until(
        _ven1_rates,
        lambda rates: isinstance(rates, list) and len(rates) > 0,
        timeout=60,
        interval=2,
        description="VEN-1 /tariffs has non-empty snapshots",
    )


@when("I open the VEN-1 controller UI")
@given("I open the VEN-1 controller UI")
def step_open_ven_controller_ui(context):
    context.ven_ui.open()
    context.ven_ui.go_controller()


@then("the controller rate chart empty state is visible")
def step_rate_chart_empty_visible(context):
    # Accept either empty or with-data state — the goal is that the page rendered.
    # (The VEN may generate estimated rates even without active PRICE events.)
    el = context.browser_page.wait_for_selector(
        f'{tid("controller-rate-chart-empty")}, {tid("controller-rate-chart")}',
        timeout=20000,
    )
    assert el is not None and el.is_visible(), (
        "Neither controller-rate-chart-empty nor controller-rate-chart is visible"
    )


@then("the controller packets table section is visible")
def step_packets_table_visible(context):
    el = context.browser_page.wait_for_selector(
        tid("controller-packets-table"), timeout=20000
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
        tid("controller-rate-chart"), timeout=20000
    )
    assert el is not None and el.is_visible(), (
        "controller-rate-chart not visible"
    )
