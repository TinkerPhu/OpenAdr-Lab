"""Step definitions for VEN Asset Timeline Endpoints (speckit 005)."""

from datetime import datetime, timezone
from behave import given, then, when
from features.helpers.api_client import ven_get


# ── Helpers ──────────────────────────────────────────────────────────────────

def _parse_ts(ts_str):
    """Parse ISO 8601 timestamp string to epoch seconds (float)."""
    clean = ts_str.replace("Z", "+00:00")
    return datetime.fromisoformat(clean).timestamp()


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
    # Record "now" with 30s tolerance for clock skew between test runner and VEN.
    cutoff = datetime.now(timezone.utc) - timedelta(seconds=30)
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
            f"Point {i} ts={ts_str} is before now-30s={cutoff.isoformat()}"
        )


# ── T019-T021: planned_state_by_asset forecast keys ──────────────────────────

@when('I poll /timeline/{asset_id} for future points with "{key}" key within {timeout:d}s')
def step_poll_timeline_future_key(context, asset_id, key, timeout):
    """Poll the asset timeline until at least one future point has *key* in values.

    Stores the final point list in ``context.last_response_json``.
    Raises TimeoutError if the planner has not populated the key within *timeout* seconds.
    """
    from features.helpers.wait import poll_until
    now_ts = datetime.now(timezone.utc).timestamp()
    context.poll_now_ts = now_ts

    def fetch():
        return ven_get(f"/timeline/{asset_id}?hours_back=0&hours_forward=4").json()

    def has_future_with_key(points):
        if not isinstance(points, list):
            return False
        # Use a 30-second margin to exclude the now-point (ts ≈ now_ts + latency).
        # This ensures only real future grid buckets are accepted, and forces the
        # poll to wait until the planner has run and LOCF has propagated planned
        # state values into the future grid.
        return any(
            p.get("values") and key in p["values"]
            for p in points
            if _parse_ts(p.get("ts", "1970-01-01T00:00:00+00:00")) > now_ts + 30
        )

    context.last_response_json = poll_until(
        fetch,
        has_future_with_key,
        timeout=timeout,
        description=f"future /timeline/{asset_id} points with '{key}' key",
    )


@then('the response has at least one future point with values key "{key}"')
def step_response_has_future_point_with_key(context, key):
    data = context.last_response_json
    assert isinstance(data, list), f"Expected list, got {type(data).__name__}"
    # Reuse the now_ts captured at @when time to avoid a timestamp race
    # (the "future" point may become "past" by the time @then runs).
    now_ts = getattr(context, "poll_now_ts", datetime.now(timezone.utc).timestamp())
    future_with_key = [
        p for p in data
        if _parse_ts(p.get("ts", "1970-01-01T00:00:00+00:00")) > now_ts + 30
        and p.get("values") and key in p["values"]
    ]
    assert future_with_key, (
        f"No future timeline point (>30s ahead) has values['{key}']. "
        f"Sample points: {data[:3]}"
    )
