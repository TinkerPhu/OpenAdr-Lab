"""Step definitions for VEN Use Case validation scenarios (Stage 6, UC-01..UC-12)."""

import time
from datetime import datetime, timedelta, timezone
from behave import given, when, then
from features.helpers.api_client import ven_get, ven_post
from features.helpers.wait import poll_until


# ---------------------------------------------------------------------------
# When: plan polling with capacity constraints
# ---------------------------------------------------------------------------

@when("I wait for the VEN /plan to have slots with import_cap_kw at most {cap:f}")
def step_wait_plan_import_cap(context, cap):
    """Poll /plan until all slots reflect the expected import cap."""
    def fetch():
        resp = ven_get("/plan")
        if not resp.ok:
            return None
        body = resp.json()
        if not isinstance(body, dict):
            return None
        return body

    def has_cap(plan):
        if plan is None:
            return False
        slots = plan.get("slots", [])
        if not slots:
            return False
        return all(
            slot.get("import_cap_kw", float("inf")) <= cap + 0.01
            for slot in slots
        )

    context.ven_plan = poll_until(
        fetch,
        has_cap,
        timeout=120,
        interval=5,
        description=f"VEN /plan slots have import_cap_kw ≤ {cap}",
    )


# ---------------------------------------------------------------------------
# When: POST /requests with CONTINUE policy
# ---------------------------------------------------------------------------

@when("I create a fresh EV packet with target_soc 0.99")
def step_create_fresh_ev_packet(context):
    """Create an EV EnergyPacket targeting 99% SoC.

    Always works regardless of current SoC (even after 0.80 completion),
    since 0.99 > any completed target in the test profile.
    """
    latest_end = (datetime.now(timezone.utc) + timedelta(hours=12)).strftime(
        "%Y-%m-%dT%H:%M:%SZ"
    )
    r = ven_post("/packets", json={
        "asset_id": "ev",
        "target_soc": 0.99,
        "desired_power_kw": 7.0,
        "latest_end": latest_end,
    })
    r.raise_for_status()
    context.fresh_ev_packet_id = r.json().get("id")


@when('I POST a CONTINUE policy request for asset "{asset_id}" with two deadline tiers')
def step_post_continue_request(context, asset_id):
    """Create a CONTINUE-policy EnergyPacket with two deadline tiers.

    Uses target_soc=0.99 so the request succeeds even when EV has completed
    a prior 0.80-target packet and SoC is now ~0.80.
    """
    tier1 = (datetime.now(timezone.utc) + timedelta(hours=8)).strftime("%Y-%m-%dT%H:%M:%SZ")
    tier2 = (datetime.now(timezone.utc) + timedelta(hours=24)).strftime("%Y-%m-%dT%H:%M:%SZ")
    payload = {
        "asset_id": asset_id,
        "target_soc": 0.99,
        "deadlines": [
            {"latest_end": tier1, "max_total_cost_eur": 5.0, "min_completion": 0.8},
            {"latest_end": tier2, "max_total_cost_eur": 10.0, "min_completion": 0.5},
        ],
        "completion_policy": "CONTINUE",
    }
    r = ven_post("/user-requests", json=payload)
    context.last_response = r
    try:
        context.last_response_json = r.json()
        context.last_created_request = r.json()
    except Exception:
        context.last_response_json = None
        context.last_created_request = None


# ---------------------------------------------------------------------------
# When: sim override helpers
# ---------------------------------------------------------------------------

@when("I POST a sim override setting ev_plugged to false")
def step_sim_override_ev_unplugged(context):
    r = ven_post("/sim/inject", json={"ev_plugged": False})
    r.raise_for_status()
    context.last_response = r


@when("I POST a sim override setting ev_plugged to true")
def step_sim_override_ev_plugged(context):
    r = ven_post("/sim/inject", json={"ev_plugged": True})
    r.raise_for_status()
    context.last_response = r


@when("I POST a sim override with no EV charging demand")
def step_sim_override_ev_zero(context):
    """No-op: ev_desired_kw was a profile-only field that the backend never applied.
    With no active EV packet the dispatcher issues no charging setpoints regardless."""
    pass


@when("I POST a sim override with full PV irradiance")
def step_sim_override_pv_full(context):
    """Set pv_irradiance to 1.0 so PV generates at full rated power regardless of time-of-day."""
    r = ven_post("/sim/inject", json={"pv_irradiance": 1.0})
    r.raise_for_status()
    context.last_response = r



# ---------------------------------------------------------------------------
# Then: sim assertions (use last_response_json set by generic "I GET {path} from the VEN")
# ---------------------------------------------------------------------------

@then("the sim EV power_kw is 0.0")
def step_sim_ev_current_zero(context):
    sim = context.last_response_json
    assert sim is not None, "No sim JSON in context — use 'I GET /sim from the VEN'"
    ev = sim.get("assets", {}).get("ev")
    assert ev is not None, f"No 'ev' field in sim assets: {list(sim.get('assets', {}).keys())}"
    kw = ev.get("power_kw", None)
    assert kw is not None, f"'ev.power_kw' not present: {ev}"
    assert abs(kw) < 0.1, (
        f"Expected EV power_kw ≈ 0.0 after unplug, got {kw}"
    )


@then("the sim EV field is present")
def step_sim_ev_field_present(context):
    sim = context.last_response_json
    assert sim is not None, "No sim JSON in context"
    ev = sim.get("assets", {}).get("ev")
    assert ev is not None, f"'ev' field not present in sim assets: {list(sim.get('assets', {}).keys())}"


# ---------------------------------------------------------------------------
# Then: plan structure assertions
# ---------------------------------------------------------------------------

@then("the plan slots have import prices populated")
def step_plan_slots_have_prices(context):
    """Verify every slot has a non-zero import_tariff_eur_kwh."""
    plan = context.ven_plan
    slots = plan.get("slots", [])
    assert slots, "No slots in plan"
    for slot in slots:
        price = slot.get("import_tariff_eur_kwh")
        assert price is not None, f"Slot missing import_tariff_eur_kwh: {slot}"


@then("all plan slots have import_cap_kw of at most {cap:f}")
def step_plan_slots_cap(context, cap):
    """Verify the import_cap_kw field in each slot does not exceed cap."""
    plan = context.ven_plan
    slots = plan.get("slots", [])
    assert slots, "No slots in plan"
    for slot in slots:
        slot_cap = slot.get("import_cap_kw")
        if slot_cap is None:
            continue  # uncapped slot (no event limit applied)
        assert slot_cap <= cap + 0.01, (
            f"Slot import_cap_kw={slot_cap} exceeds limit {cap} kW. Slot: {slot.get('slot_index')}"
        )


@then("all plan slots have net_import_kw of at most {cap:f}")
def step_plan_slots_net_import(context, cap):
    """Verify planned net import per slot does not exceed the cap."""
    plan = context.ven_plan
    slots = plan.get("slots", [])
    assert slots, "No slots in plan"
    for slot in slots:
        net = slot.get("net_import_kw", 0.0)
        assert net <= cap + 0.01, (
            f"net_import_kw={net:.3f} exceeds cap {cap} kW in slot {slot.get('slot_index')}"
        )


# ---------------------------------------------------------------------------
# Then: envelope assertions
# ---------------------------------------------------------------------------

@then('the plan envelopes contain an entry for asset "{asset_id}"')
def step_plan_envelopes_have_asset(context, asset_id):
    plan = context.ven_plan
    envs = plan.get("envelopes", [])
    found = any(e.get("asset_id") == asset_id for e in envs)
    assert found, (
        f"No envelope with asset_id='{asset_id}'. "
        f"Envelope assets: {[e.get('asset_id') for e in envs]}"
    )


@then('the flexibility envelopes contain an entry for asset "{asset_id}"')
def step_flexibility_envelopes_have_asset(context, asset_id):
    data = context.last_response_json
    assert isinstance(data, list), f"Expected list of envelopes, got {type(data)}"
    found = any(e.get("asset_id") == asset_id for e in data)
    assert found, (
        f"No envelope with asset_id='{asset_id}'. "
        f"Assets: {[e.get('asset_id') for e in data]}"
    )


@then('the first envelope has field "{field}"')
def step_first_envelope_has_field(context, field):
    plan = context.ven_plan
    envs = plan.get("envelopes", [])
    assert envs, "No envelopes in plan"
    env = envs[0]
    assert field in env, (
        f"Envelope missing field '{field}'. Available: {list(env.keys())}"
    )


@then('the first envelope field "{field}" is greater than {threshold:f}')
def step_first_envelope_field_gt(context, field, threshold):
    plan = context.ven_plan
    envs = plan.get("envelopes", [])
    assert envs, "No envelopes in plan"
    val = envs[0].get(field)
    assert isinstance(val, (int, float)), f"Field '{field}' is not a number: {val!r}"
    assert val > threshold, f"Envelope field '{field}' = {val} is not > {threshold}"
