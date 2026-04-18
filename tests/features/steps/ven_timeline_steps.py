"""Step definitions for VEN Asset Timeline Endpoints (speckit 005)."""

from datetime import datetime, timezone
from behave import given, then
from features.helpers.api_client import ven_get


# ── Given ────────────────────────────────────────────────────────────────────

@given("the VEN is running")
def step_ven_is_running(context):
    r = ven_get("/health")
    assert r.status_code == 200, f"VEN health check failed: {r.status_code}"


# ── Then ─────────────────────────────────────────────────────────────────────

@then("the response JSON is an object")
def step_response_is_object(context):
    resp = getattr(context, "last_response", None)
    assert resp is not None, "No response in context"
    data = resp.json()
    assert isinstance(data, dict), f"Expected JSON object, got {type(data).__name__}: {str(data)[:200]}"
    context.last_response_json = data


@then("every timeline point has a ts field")
def step_every_point_has_ts(context):
    data = getattr(context, "last_response_json", None) or context.last_response.json()
    assert isinstance(data, list), "Response is not a list"
    for i, point in enumerate(data):
        assert "ts" in point, f"Point {i} missing 'ts' field: {point}"


@then("every timeline point has a values object")
def step_every_point_has_values(context):
    data = getattr(context, "last_response_json", None) or context.last_response.json()
    assert isinstance(data, list), "Response is not a list"
    for i, point in enumerate(data):
        assert "values" in point, f"Point {i} missing 'values' field: {point}"
        # values can be null (empty grid bucket) or a dict
        assert point["values"] is None or isinstance(point["values"], dict), (
            f"Point {i} 'values' is not an object or null: {point['values']}"
        )


@then('the timeline all response contains key "{key}"')
def step_timeline_all_contains_key(context, key):
    data = getattr(context, "last_response_json", None) or context.last_response.json()
    assert isinstance(data, dict), "Response is not a JSON object"
    assert key in data, f"Key '{key}' not found in timeline/all response. Keys: {list(data.keys())}"


@then("all timeline points are at or after now")
def step_all_points_at_or_after_now(context):
    from datetime import timedelta
    # Record "now" with 15s tolerance for clock skew between test runner and VEN.
    cutoff = datetime.now(timezone.utc) - timedelta(seconds=15)
    data = getattr(context, "last_response_json", None) or context.last_response.json()
    assert isinstance(data, list), "Response is not a list"
    for i, point in enumerate(data):
        ts_str = point.get("ts", "")
        ts_str_clean = ts_str.replace("Z", "+00:00")
        try:
            ts_dt = datetime.fromisoformat(ts_str_clean)
        except ValueError:
            assert False, f"Point {i} has unparseable ts: {ts_str!r}"
        assert ts_dt >= cutoff, (
            f"Point {i} ts={ts_str} is before now-15s={cutoff.isoformat()}"
        )
