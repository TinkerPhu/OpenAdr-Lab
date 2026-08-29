"""Step definitions for heater tank MILP trajectory model BDD tests."""

import uuid
from datetime import datetime, timedelta, timezone
from behave import given, then
from features.helpers.api_client import ven_get, vtn_post


# ---------------------------------------------------------------------------
# Given: VTN cheap PRICE event (N-hour variant)
# ---------------------------------------------------------------------------

@given("I create a cheap 3-hour PRICE event for the saved program")
def step_create_cheap_3h_price(context):
    now = datetime.now(timezone.utc)
    intervals = [
        {
            "id": i,
            "intervalPeriod": {
                "start": (now + timedelta(hours=i)).strftime("%Y-%m-%dT%H:%M:%SZ"),
                "duration": "PT1H",
            },
            "payloads": [{"type": "PRICE", "values": [0.05]}],
        }
        for i in range(3)
    ]
    r = vtn_post(
        "/events",
        context.vtn_token,
        json={
            "programID": context.saved_program_id,
            "eventName": f"heater-tank-cheap-{uuid.uuid4().hex[:8]}",
            "priority": 1,
            "intervals": intervals,
        },
    )
    r.raise_for_status()
    context.heater_tank_event_id = r.json().get("id")


# ---------------------------------------------------------------------------
# Then: heater plan allocation assertions
# ---------------------------------------------------------------------------

@then("the plan has no heater allocations at full power in the first {slots:d} slots")
def step_no_full_power_heater_in_first_n_slots(context, slots):
    plan = context.ven_plan
    all_slots = plan.get("slots", [])
    first_slots = all_slots[:slots]
    full_power = 3.0  # max_kw for test profile
    for i, slot in enumerate(first_slots):
        for alloc in slot.get("allocations", []):
            if alloc.get("asset_id") == "heater":
                power = alloc.get("power_kw", 0.0)
                assert power < full_power - 0.1, (
                    f"Slot {i} has heater at {power:.2f} kW which is at/near full power "
                    f"({full_power} kW) — expected only mid-tier near T_max"
                )


@then("at least one of the first {n:d} plan slots has a heater allocation")
def step_at_least_one_of_first_n_slots_has_heater(context, n):
    plan = context.ven_plan
    slots = plan.get("slots", [])
    first_slots = slots[:n]
    found = any(
        any(a.get("asset_id") == "heater" for a in slot.get("allocations", []))
        for slot in first_slots
    )
    assert found, (
        f"No heater allocation in first {n} slots (checked {len(first_slots)} slots). "
        f"Total plan slots: {len(slots)}"
    )
