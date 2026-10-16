"""Step definitions for ven_multi_asset.feature (W7): several assets of one kind, each under its
own id. Runs against the test-ven-multi container (VEN/profiles/multi_asset_test.yaml)."""

from behave import then
from features.helpers.api_client import ven_delete, ven_get
from features.helpers.wait import poll_until


@then('the VEN lists the assets "{first}" and "{second}" of type "{asset_type}"')
def step_assets_of_type(context, first, second, asset_type):
    assets = context.last_response_json["assets"]
    for asset_id in (first, second):
        assert asset_id in assets, f"{asset_id} missing from /sim: {sorted(assets)}"
        assert assets[asset_id]["asset_type"] == asset_type, assets[asset_id]["asset_type"]
    # Each is listed in the profile's roster order.
    assert assets[first]["roster_index"] < assets[second]["roster_index"]


@then('within {seconds:d} seconds the plan forecasts the arrays "{first}" and "{second}" each on its own')
def step_plan_forecasts_each_array(context, seconds, first, second):
    def plan():
        r = ven_get("/plan")
        return r.json() if r.ok else None

    def forecasts_both(p):
        slots = (p or {}).get("slots") or []
        return bool(slots) and all(
            {first, second} <= set(s.get("forecast_by_asset", {})) for s in slots
        )

    p = poll_until(plan, forecasts_both, timeout=seconds, interval=3,
                   description=f"a plan forecasting {first} and {second}")
    for asset_id in (first, second):
        power = p["slots"][0]["forecast_by_asset"][asset_id]
        assert power["generation"] is True, power


@then('the EV "{owner}" holds the new session and the EV "{other}" holds none')
def step_session_belongs_to_its_ev(context, owner, other):
    created = context.last_created_request
    sessions = ven_get("/ev-session").json()
    sessions = sessions if isinstance(sessions, list) else [sessions]
    mine = [s for s in sessions if s and s.get("id") == created.get("session_id")]
    assert mine, f"the new session is not listed: {sessions}"
    assert mine[0]["asset_id"] == owner, mine[0]
    assert not any(s and s.get("asset_id") == other for s in sessions), sessions
    context.add_cleanup(lambda: ven_delete(f"/user-requests/{created['id']}"))


@then('within {seconds:d} seconds a report for the event names the resources "{a}", "{b}" and "{c}"')
def step_report_names_resources(context, seconds, a, b, c):
    """`/reports` echoes the VTN's reports of this VEN's client; the per-asset names are what
    only a VEN with these assets sends (R-143)."""
    wanted = {a, b, c}

    def reports():
        r = ven_get("/reports")
        return r.json() if r.ok else []

    def names_all(rs):
        return any(
            wanted <= {res.get("resourceName") for res in rep.get("resources", [])}
            for rep in rs
            if rep.get("eventID") == context.saved_event_id
        )

    poll_until(reports, names_all, timeout=seconds, interval=5,
               description=f"a report for the event naming {sorted(wanted)}")
