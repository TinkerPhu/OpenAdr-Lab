"""Step definitions for the EV plugged state on the timeline, in history and on the chart.

The value is the EV's own `plugged` (1 = plugged) on every surface: the timeline's
now-point and past points (measured), its future points (the plan's predicted presence),
and the persisted one-minute history rows (the fraction of the minute it was plugged in).
The Controller and History charts shade where it is below 1.
"""

from behave import then, when
from features.helpers.api_client import ven_get
from features.helpers.ui import tid
from features.helpers.wait import poll_until


def _now_point(asset_id):
    """The timeline's now-point: with no past window it is the first point returned."""
    points = ven_get(f"/timeline/{asset_id}?hours_back=0&hours_forward=1").json()
    return points[0] if isinstance(points, list) and points else None


def _value(point, key):
    values = (point or {}).get("values") or {}
    return values.get(key)


@when('I poll the /timeline/{asset_id} now-point until "{key}" equals {value:g} within {timeout:d}s')
def step_poll_now_point(context, asset_id, key, value, timeout):
    context.now_point = poll_until(
        lambda: _now_point(asset_id),
        lambda p: _value(p, key) == value,
        timeout=timeout,
        description=f"/timeline/{asset_id} now-point {key} == {value}",
    )


@then('the polled now-point has "{key}" equal to {value:g}')
def step_now_point_has(context, key, value):
    actual = _value(context.now_point, key)
    assert actual == value, f"now-point {key} is {actual!r}, expected {value!r}: {context.now_point}"


@when('I poll /history/ticks for an "{asset_id}" row with a numeric "{key}" within {timeout:d}s')
def step_poll_history_ticks_numeric(context, asset_id, key, timeout):
    def fetch():
        r = ven_get(f"/history/ticks?asset_id={asset_id}")
        return r.json() if r.status_code == 200 else []

    def has_numeric(rows):
        return isinstance(rows, list) and any(
            isinstance(row.get(key), (int, float)) for row in rows
        )

    context.history_rows = poll_until(
        fetch,
        has_numeric,
        timeout=timeout,
        interval=5,
        description=f"/history/ticks {asset_id} row with numeric {key}",
    )


@then('every polled history row carries "{key}" as a fraction between 0 and 1 or null')
def step_history_rows_fraction(context, key):
    rows = context.history_rows
    assert rows, "no history rows were polled"
    for row in rows:
        assert key in row, f"row has no {key!r} field at all: {row}"
        v = row[key]
        assert v is None or 0.0 <= v <= 1.0, f"{key}={v!r} is not a 0..1 fraction: {row}"


@then("the EV timeline chart shows an unplugged band")
def step_ev_chart_unplugged_band(context):
    # `state-shading-<spec key>-<kind>` is set by ui-charts/src/StateShading.tsx; the EV's
    # declaration lives in VEN/ui/src/components/controller/assetChartSpecs.ts.
    selector = f'{tid("asset-timeline-chart-ev")} .state-shading-ev-unplugged'
    band = context.browser_page.wait_for_selector(selector, state="attached", timeout=90000)
    assert band is not None, "no unplugged band inside asset-timeline-chart-ev"
