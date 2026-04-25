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

@then("the first plan slot has no heater allocation")
def step_first_slot_no_heater(context):
    plan = context.ven_plan
    slots = plan.get("slots", [])
    assert slots, "Plan has no slots"
    first_allocs = slots[0].get("allocations", [])
    heater = next((a for a in first_allocs if a.get("asset_id") == "heater"), None)
    assert heater is None, (
        f"Expected no heater in first plan slot, got power_kw={heater.get('power_kw')}"
    )


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


@then("the plan has at most {count:d} heater allocations in the first {slots:d} slots")
def step_plan_at_most_n_heater_allocs_in_first_m_slots(context, count, slots):
    plan = context.ven_plan
    all_slots = plan.get("slots", [])
    first_slots = all_slots[:slots]
    heater_count = sum(
        1
        for slot in first_slots
        if any(a.get("asset_id") == "heater" for a in slot.get("allocations", []))
    )
    assert heater_count <= count, (
        f"Expected at most {count} heater allocations in first {slots} slots, "
        f"got {heater_count}. "
        f"Slot allocations: {[slot.get('allocations') for slot in first_slots]}"
    )


@then("the first heater slot power is {power:f} kW")
def step_first_heater_slot_power(context, power):
    plan = context.ven_plan
    slots = plan.get("slots", [])
    for slot in slots:
        allocs = slot.get("allocations", [])
        heater_alloc = next((a for a in allocs if a.get("asset_id") == "heater"), None)
        if heater_alloc is not None:
            actual = heater_alloc.get("power_kw", 0.0)
            assert abs(actual - power) < 0.1, (
                f"First heater slot power_kw={actual:.3f} kW, expected {power:.1f} kW"
            )
            return
    raise AssertionError(
        f"No heater allocation found in any of {len(slots)} plan slots"
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
