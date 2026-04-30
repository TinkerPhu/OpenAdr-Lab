"""Step definitions for shiftable-load lifecycle polling (Plan B AC#1-5).

These steps poll VEN /plan and /sim until a shiftable load asset appears,
disappears, or meets power assertions.
"""

from behave import when, then, given
from features.helpers.api_client import ven_get
from features.helpers.wait import poll_until


# ── Plan polling ─────────────────────────────────────────────────────────────

@when('I poll the VEN /plan until asset "{asset_id}" has an allocation')
def step_poll_plan_for_asset(context, asset_id):
    def fetch():
        resp = ven_get("/plan")
        if not resp.ok:
            return None
        return resp.json()

    def has_alloc(plan):
        if plan is None:
            return False
        for slot in plan.get("slots", []):
            for alloc in slot.get("allocations", []):
                if alloc.get("asset_id") == asset_id:
                    return True
        return False

    context.polled_plan = poll_until(
        fetch, has_alloc,
        timeout=180, interval=5,
        description=f"/plan has allocation for '{asset_id}'",
    )


@then('the polled plan has an allocation for asset "{asset_id}"')
def step_assert_plan_has_alloc(context, asset_id):
    plan = context.polled_plan
    found = any(
        alloc.get("asset_id") == asset_id
        for slot in plan.get("slots", [])
        for alloc in slot.get("allocations", [])
    )
    assert found, (
        f"Expected plan to have allocation for '{asset_id}', "
        f"but none found in {len(plan.get('slots', []))} slots."
    )


# ── Sim polling (asset appears) ─────────────────────────────────────────────

@when('I poll the VEN /sim until asset "{asset_id}" appears')
@given('I poll the VEN /sim until asset "{asset_id}" appears')
def step_poll_sim_until_asset_appears(context, asset_id):
    def fetch():
        resp = ven_get("/sim")
        if not resp.ok:
            return None
        return resp.json()

    def asset_present(sim):
        if sim is None:
            return False
        return asset_id in sim.get("assets", {})

    context.polled_sim = poll_until(
        fetch, asset_present,
        timeout=300, interval=3,
        description=f"/sim has asset '{asset_id}'",
    )


@then('the polled sim has asset "{asset_id}" with power_kw > 0')
def step_assert_sim_asset_power(context, asset_id):
    sim = context.polled_sim
    assets = sim.get("assets", {})
    asset = assets.get(asset_id)
    assert asset is not None, (
        f"Asset '{asset_id}' not in sim. Available: {list(assets.keys())}"
    )
    power = asset.get("power_kw", 0)
    assert power > 0, (
        f"Expected power_kw > 0 for '{asset_id}', got {power}"
    )


# ── Sim polling (asset disappears) ──────────────────────────────────────────

@when('I poll the VEN /sim until asset "{asset_id}" disappears')
def step_poll_sim_until_asset_disappears(context, asset_id):
    def fetch():
        resp = ven_get("/sim")
        if not resp.ok:
            return None
        return resp.json()

    def asset_gone(sim):
        if sim is None:
            return False
        return asset_id not in sim.get("assets", {})

    context.polled_sim = poll_until(
        fetch, asset_gone,
        timeout=180, interval=3,
        description=f"/sim no longer has asset '{asset_id}'",
    )


@then('the polled sim does not have asset "{asset_id}"')
def step_assert_sim_asset_gone(context, asset_id):
    sim = context.polled_sim
    assets = sim.get("assets", {})
    assert asset_id not in assets, (
        f"Expected '{asset_id}' to be gone from sim, but still present. "
        f"Assets: {list(assets.keys())}"
    )
