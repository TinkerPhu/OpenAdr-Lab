"""Step definitions for asset forecast() and past() BDD tests (speckit 007)."""

import time
from datetime import datetime, timezone, timedelta
from behave import given, when, then
from features.helpers.api_client import ven_get


# ── Given ─────────────────────────────────────────────────────────────────────

@given("the VEN has been running for at least {seconds:d} seconds")
def step_ven_running_for(context, seconds):
    """Poll until the VEN has been running long enough for history to accumulate."""
    r = ven_get("/health")
    assert r.status_code == 200, f"VEN not healthy: {r.status_code}"
    time.sleep(max(0, seconds))


# ── When ──────────────────────────────────────────────────────────────────────

@when("I GET /forecast/{asset_id}?timespan_s={timespan_s} from the VEN")
def step_get_forecast(context, asset_id, timespan_s):
    context.forecast_ts_requested = timespan_s
    context.forecast_request_time = datetime.now(timezone.utc)
    r = ven_get(f"/forecast/{asset_id}", params={"timespan_s": timespan_s})
    context.last_response = r
    if r.status_code == 200:
        context.forecast_json = r.json()
    else:
        context.forecast_json = None


@when("I GET /timeline/{asset_id}?hours_back={hours_back} from the VEN")
def step_get_timeline(context, asset_id, hours_back):
    context.history_hours_back = float(hours_back)
    context.history_request_time = datetime.now(timezone.utc)
    r = ven_get(f"/timeline/{asset_id}", params={"hours_back": hours_back})
    context.last_response = r
    if r.status_code == 200:
        context.timeline_json = r.json()
    else:
        context.timeline_json = None


# ── Then: forecast ────────────────────────────────────────────────────────────

@then("the forecast response has a non-empty samples list")
def step_forecast_non_empty(context):
    data = context.forecast_json
    assert data is not None, "No forecast JSON in context"
    assert "samples" in data, f"Response missing 'samples': {data}"
    assert len(data["samples"]) > 0, f"Expected non-empty samples, got empty list"


@then("the forecast samples list is empty")
def step_forecast_empty(context):
    data = context.forecast_json
    assert data is not None, "No forecast JSON in context"
    assert "samples" in data, f"Response missing 'samples': {data}"
    assert len(data["samples"]) == 0, f"Expected empty samples, got {len(data['samples'])} items"


@then("the forecast quantity is \"{expected}\"")
def step_forecast_quantity(context, expected):
    data = context.forecast_json
    assert data is not None, "No forecast JSON in context"
    assert data.get("quantity") == expected, (
        f"Expected quantity '{expected}', got '{data.get('quantity')}'"
    )


@then("the forecast unit is \"{expected}\"")
def step_forecast_unit(context, expected):
    data = context.forecast_json
    assert data is not None, "No forecast JSON in context"
    assert data.get("unit") == expected, (
        f"Expected unit '{expected}', got '{data.get('unit')}'"
    )


@then("the forecast interpolation is \"{expected}\"")
def step_forecast_interpolation(context, expected):
    data = context.forecast_json
    assert data is not None, "No forecast JSON in context"
    assert data.get("interpolation") == expected, (
        f"Expected interpolation '{expected}', got '{data.get('interpolation')}'"
    )


@then("the forecast samples are in ascending timestamp order")
def step_forecast_ascending(context):
    samples = context.forecast_json.get("samples", [])
    timestamps = [s["ts"] for s in samples]
    assert timestamps == sorted(timestamps), "Forecast samples are not in ascending timestamp order"


@then("the last forecast sample is within {tolerance:d} second of now plus {offset_s:d} seconds")
def step_forecast_boundary_point(context, tolerance, offset_s):
    samples = context.forecast_json.get("samples", [])
    assert samples, "No samples to check boundary point"
    last_ts_str = samples[-1]["ts"].replace("Z", "+00:00")
    last_ts = datetime.fromisoformat(last_ts_str)
    expected = context.forecast_request_time + timedelta(seconds=offset_s)
    delta = abs((last_ts - expected).total_seconds())
    assert delta <= tolerance, (
        f"Last sample at {last_ts} is {delta:.1f}s away from expected boundary {expected}"
    )


@then("all forecast sample values are 0.0")
def step_forecast_all_zero(context):
    samples = context.forecast_json.get("samples", [])
    for i, s in enumerate(samples):
        assert s["value"] == 0.0, f"Sample {i} has non-zero value: {s['value']}"


@then("all forecast sample values are equal")
def step_forecast_all_equal(context):
    samples = context.forecast_json.get("samples", [])
    if len(samples) < 2:
        return  # trivially equal
    first_val = samples[0]["value"]
    for i, s in enumerate(samples[1:], start=1):
        assert s["value"] == first_val, (
            f"Sample {i} value {s['value']} differs from first {first_val}"
        )


# ── Then: history ─────────────────────────────────────────────────────────────

@then("the timeline response has a non-empty samples list")
def step_timeline_non_empty(context):
    data = context.timeline_json
    assert data is not None, "No timeline JSON in context"
    assert "samples" in data, f"Response missing 'samples': {list(data.keys()) if isinstance(data, dict) else type(data)}"
    assert len(data["samples"]) > 0, "Expected non-empty samples, got empty list"


@then("the timeline response is a valid history response")
def step_timeline_valid(context):
    data = context.timeline_json
    assert data is not None, "No timeline JSON in context"
    assert "samples" in data, f"Response missing 'samples' key"


@then("the history quantity is \"{expected}\"")
def step_history_quantity(context, expected):
    data = context.timeline_json
    assert data is not None, "No timeline JSON in context"
    assert data.get("quantity") == expected, (
        f"Expected quantity '{expected}', got '{data.get('quantity')}'"
    )


@then("the history unit is \"{expected}\"")
def step_history_unit(context, expected):
    data = context.timeline_json
    assert data is not None, "No timeline JSON in context"
    assert data.get("unit") == expected, (
        f"Expected unit '{expected}', got '{data.get('unit')}'"
    )


@then("the history interpolation is \"{expected}\"")
def step_history_interpolation(context, expected):
    data = context.timeline_json
    assert data is not None, "No timeline JSON in context"
    assert data.get("interpolation") == expected, (
        f"Expected interpolation '{expected}', got '{data.get('interpolation')}'"
    )


@then("the history samples are in ascending timestamp order")
def step_history_ascending(context):
    samples = context.timeline_json.get("samples", [])
    timestamps = [s["ts"] for s in samples]
    assert timestamps == sorted(timestamps), "History samples are not in ascending timestamp order"


@then("the first history sample is within {tolerance:d} second of now minus {offset_s:d} seconds")
def step_history_boundary_point(context, tolerance, offset_s):
    samples = context.timeline_json.get("samples", [])
    assert samples, "No samples to check boundary point"
    first_ts_str = samples[0]["ts"].replace("Z", "+00:00")
    first_ts = datetime.fromisoformat(first_ts_str)
    expected = context.history_request_time - timedelta(seconds=offset_s)
    delta = abs((first_ts - expected).total_seconds())
    assert delta <= tolerance, (
        f"First sample at {first_ts} is {delta:.1f}s away from expected boundary {expected}"
    )


@then("no history sample has a future timestamp")
def step_history_no_future(context):
    now = datetime.now(timezone.utc)
    samples = context.timeline_json.get("samples", [])
    for i, s in enumerate(samples):
        ts_str = s["ts"].replace("Z", "+00:00")
        ts = datetime.fromisoformat(ts_str)
        assert ts <= now + timedelta(seconds=2), (
            f"Sample {i} has future timestamp: {ts} > {now}"
        )
