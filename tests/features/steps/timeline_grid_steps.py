"""Step definitions for Uniform-Grid Timeline API (RF-05c)."""

from datetime import datetime, timezone
from behave import then


# ── Helpers ──────────────────────────────────────────────────────────────────

def _parse_ts(ts_str):
    """Parse ISO 8601 timestamp to epoch seconds."""
    clean = ts_str.replace("Z", "+00:00")
    return datetime.fromisoformat(clean).timestamp()


def _get_timeline_all(context):
    """Get the parsed /timeline/all response as a dict."""
    data = getattr(context, "last_response_json", None)
    if data is None:
        data = context.last_response.json()
        context.last_response_json = data
    return data


def _find_now_index(points, resolution=None):
    """Find the index of the now-point (non-grid-aligned timestamp).

    Strategy: find the single point whose epoch is NOT a multiple of
    the grid resolution. If resolution is not provided, infer it from
    the most common inter-point spacing.

    Falls back to the gap-based heuristic when all points happen to
    be grid-aligned (now falls exactly on a grid boundary).
    """
    if len(points) < 3:
        return None

    ts_list = [_parse_ts(p["ts"]) for p in points]

    # Determine resolution
    if resolution is None:
        from collections import Counter
        deltas = [round(ts_list[i + 1] - ts_list[i]) for i in range(len(ts_list) - 1)]
        resolution = Counter(deltas).most_common(1)[0][0]

    # Primary: find the single non-aligned point
    unaligned = [i for i, ts in enumerate(ts_list) if int(ts) % resolution != 0]
    if len(unaligned) == 1:
        return unaligned[0]

    # Fallback: when now lands on a grid boundary, find the point
    # closest to the current time.
    import time
    now_epoch = time.time()
    closest_idx = min(range(len(ts_list)), key=lambda i: abs(ts_list[i] - now_epoch))
    # Only accept if it's not at the very edges
    if 0 < closest_idx < len(ts_list) - 1:
        return closest_idx

    return None


# ── Then — US1: Grid alignment ───────────────────────────────────────────────

@then("all asset arrays have the same length")
def step_all_same_length(context):
    data = _get_timeline_all(context)
    lengths = {k: len(v) for k, v in data.items()}
    unique = set(lengths.values())
    assert len(unique) == 1, f"Asset array lengths differ: {lengths}"


@then("all asset arrays have identical ts values at each index")
def step_identical_ts(context):
    data = _get_timeline_all(context)
    assets = list(data.values())
    if len(assets) < 2:
        return  # nothing to compare
    ref = assets[0]
    for i, point in enumerate(ref):
        ref_ts = point["ts"]
        for asset_name, arr in data.items():
            assert arr[i]["ts"] == ref_ts, (
                f"ts mismatch at index {i}: ref={ref_ts}, {asset_name}={arr[i]['ts']}"
            )


@then("the grid portions have uniform spacing of {seconds:d} seconds")
def step_uniform_spacing(context, seconds):
    data = _get_timeline_all(context)
    # Use any asset — all have the same ts values
    points = list(data.values())[0]
    now_idx = _find_now_index(points)

    # Check history portion (before now-point)
    if now_idx and now_idx >= 2:
        for i in range(1, now_idx):
            t0 = _parse_ts(points[i - 1]["ts"])
            t1 = _parse_ts(points[i]["ts"])
            delta = round(t1 - t0)
            assert delta == seconds, (
                f"History grid spacing at index {i}: expected {seconds}s, got {delta}s"
            )

    # Check future portion (after now-point)
    if now_idx is not None and now_idx + 2 < len(points):
        for i in range(now_idx + 2, len(points)):
            t0 = _parse_ts(points[i - 1]["ts"])
            t1 = _parse_ts(points[i]["ts"])
            delta = round(t1 - t0)
            assert delta == seconds, (
                f"Future grid spacing at index {i}: expected {seconds}s, got {delta}s"
            )


@then("all grid-portion timestamps are multiples of {seconds:d} seconds")
def step_round_boundaries(context, seconds):
    data = _get_timeline_all(context)
    points = list(data.values())[0]

    # Collect all non-aligned timestamps.  Allow exactly one (the now-point).
    unaligned = []
    for i, point in enumerate(points):
        epoch = int(_parse_ts(point["ts"]))
        if epoch % seconds != 0:
            unaligned.append((i, point["ts"], epoch))

    assert len(unaligned) <= 1, (
        f"Found {len(unaligned)} unaligned timestamps "
        f"(expected at most 1 now-point): "
        + ", ".join(
            f"index {i} ({ts}, epoch={e})" for i, ts, e in unaligned
        )
    )


# ── Then — US2: Now-point ────────────────────────────────────────────────────

@then("each asset array has a now-point between history and future grid portions")
def step_has_now_point(context):
    data = _get_timeline_all(context)
    for asset_id, points in data.items():
        now_idx = _find_now_index(points)
        assert now_idx is not None, (
            f"Asset '{asset_id}': could not find now-point in array of {len(points)} points"
        )
        assert 0 < now_idx < len(points) - 1, (
            f"Asset '{asset_id}': now-point at index {now_idx} is not between "
            f"history and future (array length {len(points)})"
        )


@then("the now-point ts is identical across all assets")
def step_now_point_same_ts(context):
    data = _get_timeline_all(context)
    now_timestamps = {}
    for asset_id, points in data.items():
        now_idx = _find_now_index(points)
        assert now_idx is not None, f"Asset '{asset_id}': no now-point found"
        now_timestamps[asset_id] = points[now_idx]["ts"]
    unique = set(now_timestamps.values())
    assert len(unique) == 1, f"Now-point timestamps differ across assets: {now_timestamps}"


@then("at least one future point has null values")
def step_future_null_values(context):
    data = _get_timeline_all(context)
    for asset_id, points in data.items():
        has_null = any(p["values"] is None for p in points)
        if has_null:
            return
    assert False, "No asset has a point with null values"


@then("each value is an array of objects with ts and values fields")
def step_format_unchanged(context):
    data = _get_timeline_all(context)
    for asset_id, arr in data.items():
        assert isinstance(arr, list), f"'{asset_id}' is not an array"
        for i, point in enumerate(arr):
            assert isinstance(point, dict), (
                f"'{asset_id}[{i}]' is not an object: {type(point).__name__}"
            )
            assert "ts" in point, f"'{asset_id}[{i}]' missing 'ts'"
            assert "values" in point, f"'{asset_id}[{i}]' missing 'values'"


# ── Then — US3: Resolution ───────────────────────────────────────────────────

@then("the total array length is between {low:d} and {high:d}")
def step_array_length_range(context, low, high):
    data = _get_timeline_all(context)
    length = len(list(data.values())[0])
    assert low <= length <= high, (
        f"Array length {length} not in range [{low}, {high}]"
    )


# ── Then — US4: Single-asset endpoint ────────────────────────────────────────

@then("the single-asset grid portions have uniform spacing of {seconds:d} seconds")
def step_single_asset_uniform(context, seconds):
    points = context.last_response.json()
    now_idx = _find_now_index(points)

    if now_idx and now_idx >= 2:
        for i in range(1, now_idx):
            t0 = _parse_ts(points[i - 1]["ts"])
            t1 = _parse_ts(points[i]["ts"])
            delta = round(t1 - t0)
            assert delta == seconds, (
                f"History grid spacing at index {i}: expected {seconds}s, got {delta}s"
            )

    if now_idx is not None and now_idx + 2 < len(points):
        for i in range(now_idx + 2, len(points)):
            t0 = _parse_ts(points[i - 1]["ts"])
            t1 = _parse_ts(points[i]["ts"])
            delta = round(t1 - t0)
            assert delta == seconds, (
                f"Future grid spacing at index {i}: expected {seconds}s, got {delta}s"
            )


@then("the single-asset array has a now-point")
def step_single_asset_now_point(context):
    points = context.last_response.json()
    now_idx = _find_now_index(points)
    assert now_idx is not None, (
        f"Could not find now-point in single-asset array of {len(points)} points"
    )
