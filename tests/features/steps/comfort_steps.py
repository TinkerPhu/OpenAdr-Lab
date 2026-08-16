"""Step definitions for the WP4.2 comfort-curve override API (BL-19) and the
BL-34 verification that the curve actually shapes the solved plan."""

from datetime import datetime, timedelta, timezone
from behave import given, when, then
from features.helpers.api_client import ven_get, ven_post, ven_delete
from features.helpers.wait import poll_until


def _parse_points(spec):
    """"0.5:0.40,1.0:0.10" -> ComfortRate list (fill:price pairs, co2 defaults to
    0.0). An optional third segment sets the CO2 bid explicitly:
    "0.5:0.40:300,1.0:0.10:100" (fill:price:co2, gCO2/kWh)."""
    rates = []
    for pair in spec.split(","):
        parts = pair.split(":")
        fill, bid = parts[0], parts[1]
        co2 = parts[2] if len(parts) > 2 else "0.0"
        rates.append({
            "fill": float(fill),
            "max_marginal_price": float(bid),
            "max_marginal_co2": float(co2),
        })
    return rates


@given('the comfort curve for asset "{asset_id}" reports source "{source}"')
def step_given_comfort_source(context, asset_id, source):
    # Self-healing: a crashed earlier run may have left a persisted override.
    if source == "default":
        ven_delete(f"/assets/{asset_id}/comfort_curve")
    _assert_comfort_source(context, asset_id, source)


@then('the comfort curve for asset "{asset_id}" reports source "{source}"')
def step_then_comfort_source(context, asset_id, source):
    _assert_comfort_source(context, asset_id, source)


def _assert_comfort_source(context, asset_id, source):
    r = ven_get(f"/assets/{asset_id}/comfort_curve")
    r.raise_for_status()
    body = r.json()
    assert body["source"] == source, f"expected source {source}, got {body}"
    context.comfort_curve = body


@when('I set a comfort curve for asset "{asset_id}" with points "{points}"')
def step_set_comfort_curve(context, asset_id, points):
    r = ven_post(f"/assets/{asset_id}/comfort_curve", json=_parse_points(points))
    r.raise_for_status()
    context.last_response = r


@when('I try to set a comfort curve for asset "{asset_id}" with points "{points}"')
def step_try_set_comfort_curve(context, asset_id, points):
    context.last_response = ven_post(
        f"/assets/{asset_id}/comfort_curve", json=_parse_points(points)
    )


@then('the comfort curve for asset "{asset_id}" has {n:d} points')
def step_comfort_point_count(context, asset_id, n):
    r = ven_get(f"/assets/{asset_id}/comfort_curve")
    r.raise_for_status()
    rates = r.json()["rates"]
    assert len(rates) == n, f"expected {n} points, got {rates}"


@when('I delete the comfort curve override for asset "{asset_id}"')
def step_delete_comfort_curve(context, asset_id):
    r = ven_delete(f"/assets/{asset_id}/comfort_curve")
    assert r.status_code in (204, 404), f"unexpected status {r.status_code}"


@then("the comfort curve request is rejected with status {code:d}")
def step_comfort_rejected(context, code):
    actual = context.last_response.status_code
    assert actual == code, f"expected {code}, got {actual}: {context.last_response.text}"


# ---------------------------------------------------------------------------
# BL-34: the resolved curve reaches the MILP solver, not just AssetRequestSlice
# storage. Goes through /user-requests (the route the curve actually threads
# through — the legacy /ev-session route has no comfort_rates concept at all).
# ---------------------------------------------------------------------------

@when('I POST a soft-deadline user request for asset "{asset_id}" with target_soc {soc:f} and latest_end in {hours:d} hours')
def step_post_soft_deadline_request(context, asset_id, soc, hours):
    latest_end = (datetime.now(timezone.utc) + timedelta(hours=hours)).strftime(
        "%Y-%m-%dT%H:%M:%SZ"
    )
    payload = {
        "asset_id": asset_id,
        "target_soc": soc,
        "deadlines": [{"latest_end": latest_end, "min_completion": 0.8}],
        "completion_policy": "CONTINUE",
        "soft_deadline": True,
    }
    r = ven_post("/user-requests", json=payload)
    r.raise_for_status()
    context.comfort_request_created_at = datetime.now(timezone.utc)
    context.comfort_request = r.json()


@when("I wait for the VEN plan to be recomputed after the comfort-curve session")
def step_wait_plan_after_comfort_session(context):
    cutoff = context.comfort_request_created_at

    def fetch():
        r = ven_get("/plan")
        if not r.ok:
            return None
        body = r.json()
        return body if isinstance(body, dict) else None

    def is_fresh(plan):
        if plan is None or "id" not in plan:
            return False
        raw = plan.get("created_at", "")
        try:
            return datetime.fromisoformat(raw.replace("Z", "+00:00")) > cutoff
        except ValueError:
            return False

    context.comfort_plan = poll_until(
        fetch,
        is_fresh,
        timeout=180,
        interval=5,
        description="VEN /plan recomputed after the comfort-curve session",
    )


def _asset_charging_kw(plan, asset_id):
    return [
        a.get("power_kw", 0.0)
        for slot in plan.get("slots", [])
        for a in slot.get("allocations", [])
        if a.get("asset_id") == asset_id
    ]


def _wait_for_comfort_plan_matching(context, predicate, description):
    """The "recomputed after" step only guarantees a plan *stamped* after the
    request was created — under host load a cycle already in flight when the
    request lands can still finish and get stamped with a fresh created_at
    without having read the new request (TOCTOU on wall-clock freshness, not
    plan content). Re-poll /plan itself against the actual assertion so a
    later cycle that does reflect the request is picked up within budget."""
    def fetch():
        r = ven_get("/plan")
        return r.json() if r.ok else None

    context.comfort_plan = poll_until(
        fetch, predicate, timeout=30, interval=3, description=description
    )


@then('the comfort-curve-driven plan has no "{asset_id}" charging')
def step_comfort_plan_no_charging(context, asset_id):
    def no_charging(plan):
        kw = _asset_charging_kw(plan, asset_id) if plan else []
        return not [p for p in kw if p > 0.01]

    if not no_charging(context.comfort_plan):
        _wait_for_comfort_plan_matching(
            context, no_charging, f"plan has no {asset_id} charging"
        )
    kw = _asset_charging_kw(context.comfort_plan, asset_id)
    offending = [p for p in kw if p > 0.01]
    assert not offending, f"expected no {asset_id} charging, got {kw}"


@then('the comfort-curve-driven plan has "{asset_id}" charging')
def step_comfort_plan_has_charging(context, asset_id):
    def has_charging(plan):
        kw = _asset_charging_kw(plan, asset_id) if plan else []
        return any(p > 0.01 for p in kw)

    if not has_charging(context.comfort_plan):
        _wait_for_comfort_plan_matching(
            context, has_charging, f"plan has {asset_id} charging"
        )
    kw = _asset_charging_kw(context.comfort_plan, asset_id)
    assert any(p > 0.01 for p in kw), f"expected {asset_id} charging, got {kw}"


@when("I DELETE the comfort-curve-driven user request")
def step_delete_comfort_request(context):
    req_id = context.comfort_request.get("id")
    assert req_id, f"no id in {context.comfort_request}"
    ven_delete(f"/user-requests/{req_id}")
