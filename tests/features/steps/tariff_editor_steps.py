"""Step definitions for the tariff editor use case (tariff_editor.feature)."""

from datetime import datetime, timedelta, timezone

from behave import given, when, then
from features.helpers.api_client import bff_get, bff_post, ven_get
from features.helpers.tariff import price_at
from features.helpers.ui import tid
from features.helpers.wait import poll_until

HOURS = 24
# A recognisable day: 0.10 all day except a cheap and an expensive hour, so a wrong shift, a
# missing repeat and a flattened series all fail differently.
PATTERN_PRICES = [0.10] * HOURS
PATTERN_PRICES[1] = 0.33
PATTERN_PRICES[17] = 0.45
CHECK_HOUR = 1


def _iso(t):
    return t.strftime("%Y-%m-%dT%H:%M:%SZ")


def _next_hour():
    return (datetime.now(timezone.utc) + timedelta(hours=1)).replace(minute=0, second=0, microsecond=0)


def _draft_body(start, forever=True):
    """The event body the editor's `toEventInput` produces: explicit starts and durations,
    the recurrence in event.duration, and the payload descriptors."""
    body = {
        "eventName": "tariff-editor-daily",
        "priority": 0,
        "intervalPeriod": {"start": _iso(start)},
        "payloadDescriptors": [{"payloadType": "PRICE", "units": "KWH", "currency": "EUR"}],
        "intervals": [
            {
                "id": h,
                "intervalPeriod": {"start": _iso(start + timedelta(hours=h)), "duration": "PT1H"},
                "payloads": [{"type": "PRICE", "values": [PATTERN_PRICES[h]]}],
            }
            for h in range(HOURS)
        ],
    }
    if forever:
        body["duration"] = "P9999Y"
    return body


@when("I request the signal registry from the BFF")
def step_request_registry(context):
    r = bff_get("/api/signals/registry")
    assert r.ok, r.text
    context.signal_registry = r.json()


@then('the signal registry declares "{payload_type}" in "{units}" and "{currency}"')
def step_registry_declares(context, payload_type, units, currency):
    row = next((x for x in context.signal_registry if x["payloadType"] == payload_type), None)
    assert row is not None, f"{payload_type} missing from {context.signal_registry}"
    assert (row["units"], row["currency"]) == (units, currency), row


@when("I preview a 24-hour tariff that repeats forever over {hours:d} hours through the BFF")
def step_preview_forever(context, hours):
    start = _next_hour()
    r = bff_post(
        "/api/signals/preview",
        json={"event": _draft_body(start), "from": _iso(start), "to": _iso(start + timedelta(hours=hours))},
    )
    assert r.ok, r.text
    context.tariff_preview = r.json()


@then("the preview holds {count:d} bands")
def step_preview_count(context, count):
    assert len(context.tariff_preview["bands"]) == count, len(context.tariff_preview["bands"])


@then("the second day of the preview repeats the first")
def step_preview_repeats(context):
    bands = context.tariff_preview["bands"]
    first, second = bands[:HOURS], bands[HOURS:]
    assert [b["value"] for b in first] == PATTERN_PRICES
    assert [b["value"] for b in second] == PATTERN_PRICES


@then("the preview is not truncated")
def step_preview_not_truncated(context):
    assert context.tariff_preview["truncated"] is False


@given("the tariff editor publishes a daily PRICE pattern that repeats forever")
def step_publish_pattern(context):
    start = _next_hour()
    context.pattern_start = start
    body = _draft_body(start)
    body["programID"] = context.saved_program_id
    r = bff_post("/api/events", json=body)
    assert r.ok, r.text
    context.rate_event_id = r.json()["id"]  # the scenario cleanup removes events by this id


@when("I wait for the VEN /tariffs endpoint to show the pattern's price on the second day")
def step_wait_second_day(context):
    at = context.pattern_start + timedelta(days=1, hours=CHECK_HOUR, minutes=10)

    def fetch():
        resp = ven_get("/tariffs")
        return resp.json() if resp.ok else []

    context.ven_rates = poll_until(
        fetch,
        lambda rates: isinstance(rates, list) and price_at(rates, at) == PATTERN_PRICES[CHECK_HOUR],
        timeout=90,
        description="VEN /tariffs carries the repeated pattern into the second day",
    )
    context.second_day_at = at


@then("the VEN /tariffs price on the second day equals the pattern's price at that hour")
def step_second_day_equals(context):
    assert price_at(context.ven_rates, context.second_day_at) == PATTERN_PRICES[CHECK_HOUR]


@then("the VEN /tariffs price on the second day differs from the one an hour earlier")
def step_second_day_differs(context):
    earlier = price_at(context.ven_rates, context.second_day_at - timedelta(hours=1))
    assert earlier == PATTERN_PRICES[CHECK_HOUR - 1], earlier
    assert earlier != PATTERN_PRICES[CHECK_HOUR]


@when("I ask the BFF for the pattern of that event")
def step_ask_pattern(context):
    listed = bff_get("/api/events")
    assert listed.ok, listed.text
    event = next(e for e in listed.json() if e["id"] == context.rate_event_id)
    r = bff_post("/api/signals/preview", json={"event": event, "pattern": True})
    assert r.ok, r.text
    context.tariff_pattern = r.json()["pattern"]


@then("the pattern holds {count:d} intervals")
def step_pattern_count(context, count):
    assert len(context.tariff_pattern) == count, len(context.tariff_pattern)


@then("the pattern's prices are the ones published")
def step_pattern_prices(context):
    got = [iv["payloads"][0]["value"] for iv in context.tariff_pattern]
    assert got == PATTERN_PRICES, got


# -- the operator's path: drag on the chart, publish from the editor --


def _price_cells(page):
    """{cell test id: text} of every PRICE cell of the editor's grid."""
    return page.evaluate(
        """() => Object.fromEntries([...document.querySelectorAll('[data-testid^="cell-PRICE-"]')]
            .map(el => [el.dataset.testid, el.value]))"""
    )


@when("I open the tariff editor on that event")
def step_open_editor(context):
    page = context.ui.page
    page.click(tid("nav-tariff"))
    page.wait_for_selector(tid("tariff-heading"))
    page.wait_for_selector(
        f'{tid("tariff-event-select")} option[value="{context.rate_event_id}"]',
        state="attached",
        timeout=60000,
    )
    page.select_option(tid("tariff-event-select"), context.rate_event_id)
    # A handle exists only once the BFF has resolved the draft into bands to draw them on.
    page.wait_for_selector('[data-testid^="handle-value-"]', state="attached", timeout=60000)


@when("I drag a price handle on the chart to the top of the plot")
def step_drag_handle(context):
    page = context.ui.page
    context.cells_before = _price_cells(page)
    handle = page.locator('[data-testid^="handle-value-"]').first
    box = handle.bounding_box()
    x, y = box["x"] + box["width"] / 2, box["y"] + box["height"] / 2
    top = page.locator(f'{tid("tariff-chart-card")} .recharts-cartesian-grid').first.bounding_box()["y"] + 1
    covering = page.evaluate(
        "([x, y]) => document.elementFromPoint(x, y)?.dataset?.testid ?? document.elementFromPoint(x, y)?.tagName",
        [x, y],
    )
    assert covering == (handle.get_attribute("data-testid")), (
        f"something else sits over the handle at ({x:.0f}, {y:.0f}): {covering}"
    )
    page.mouse.move(x, y)
    page.mouse.down()
    page.mouse.move(x, top, steps=12)
    page.wait_for_selector(tid("drag-readout"), timeout=5000)
    context.drag_readout = page.locator(tid("drag-readout")).text_content()
    page.mouse.up()


@then("exactly one grid price changed, to the value the drag readout showed")
def step_one_price_changed(context):
    page = context.ui.page
    changed = poll_until(
        lambda: {k: v for k, v in _price_cells(page).items() if context.cells_before.get(k) != v},
        lambda c: len(c) >= 1,
        timeout=10,
        description="the dragged price reaches the grid",
    )
    assert len(changed) == 1, f"a drag edits one row, the grid changed {changed}"
    (cell, text), = changed.items()
    context.dragged_row = int(cell.rsplit("-", 1)[1])
    context.dragged_price = float(text)
    shown = float(context.drag_readout.split()[0])
    assert abs(context.dragged_price - shown) < 1e-9, f"grid {text}, readout {context.drag_readout!r}"
    assert context.dragged_price != PATTERN_PRICES[context.dragged_row], "the drag moved nothing"


@when("I publish the draft from the editor")
def step_publish_draft(context):
    page = context.ui.page
    page.click(tid("tariff-publish-btn"))
    page.click(tid("publish-confirm"))
    page.wait_for_selector(tid("tariff-published-note"), timeout=30000)


@then("the VTN holds a second event for the program carrying the dragged price")
def step_vtn_holds_published(context):
    listed = bff_get("/api/events")
    assert listed.ok, listed.text
    mine = [
        e for e in listed.json()
        if e["programID"] == context.saved_program_id and e["id"] != context.rate_event_id
    ]
    assert len(mine) == 1, [e["id"] for e in mine]
    event = mine[0]
    context.published_event = event
    context.created_event = {"id": event["id"]}  # removed by the scenario cleanup
    prices = [iv["payloads"][0]["values"][0] for iv in event["intervals"]]
    expected = list(PATTERN_PRICES)
    expected[context.dragged_row] = context.dragged_price
    assert prices == expected, prices
    assert event.get("duration") == "P9999Y", event.get("duration")
    assert event["priority"] <= 0, event["priority"]


@when("I wait for the VEN /tariffs endpoint to show the dragged price")
def step_wait_dragged_price(context):
    at = context.pattern_start + timedelta(hours=context.dragged_row, minutes=10)

    def fetch():
        resp = ven_get("/tariffs")
        return resp.json() if resp.ok else []

    poll_until(
        fetch,
        lambda rates: isinstance(rates, list) and price_at(rates, at) == context.dragged_price,
        timeout=120,
        description="VEN /tariffs carries the price dragged on the chart",
    )
