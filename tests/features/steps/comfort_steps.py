"""Step definitions for the WP4.2 comfort-curve override API (BL-19)."""

from behave import given, when, then
from features.helpers.api_client import ven_get, ven_post, ven_delete


def _parse_points(spec):
    """"0.5:0.40,1.0:0.10" -> ComfortRate list (fill:bid pairs)."""
    rates = []
    for pair in spec.split(","):
        fill, bid = pair.split(":")
        rates.append({
            "fill": float(fill),
            "max_marginal_price": float(bid),
            "max_marginal_co2": 0.0,
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
