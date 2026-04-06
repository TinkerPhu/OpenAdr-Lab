"""Step definitions for EV charging scenarios (Chunk 4).

Covers: SoC ceiling, IMPORT_CAPACITY_LIMIT, user-request path, battery tariff logic.
"""

import uuid
from datetime import datetime, timedelta, timezone
from behave import given, when, then
from features.helpers.api_client import ven_get, ven_post, vtn_post
from features.helpers.wait import poll_until


# ── Given: sim inject ────────────────────────────────────────────────────────

@given("I inject ev_soc_target {soc:f} via sim inject")
def step_given_inject_ev_soc_target(context, soc):
    r = ven_post("/sim/inject", json={"ev_soc_target": soc})
    r.raise_for_status()


@when("I inject ev_soc_target {soc:f} via sim inject")
def step_when_inject_ev_soc_target(context, soc):
    r = ven_post("/sim/inject", json={"ev_soc_target": soc})
    r.raise_for_status()


# ── Given: user request (Given wrapper for existing When step) ───────────────

@given("I POST a user request for EV with target_soc {soc:f} and latest_end in {hours:d} hours")
def step_given_post_user_request_ev(context, soc, hours):
    latest_end = (datetime.now(timezone.utc) + timedelta(hours=hours)).strftime(
        "%Y-%m-%dT%H:%M:%SZ"
    )
    r = ven_post("/user-requests", json={
        "asset_id": "ev",
        "target_soc": soc,
        "deadlines": [{"latest_end": latest_end, "min_completion": 0.8}],
        "completion_policy": "STOP",
    })
    r.raise_for_status()
    context.last_created_request = r.json()


# ── Given: VTN event helpers ─────────────────────────────────────────────────

@given("I create a cheap-then-expensive PRICE event for the saved program")
def step_create_cheap_expensive_price_event(context):
    """Three intervals: 1h cheap → 3h expensive → 20h neutral.

    The two-pass planner only schedules grid charging when depletion is predicted.
    With SoC=0.20, baseline=0.5 kW, no PV:
      - Shadow sim discharges at 0.40 EUR/kWh (slots 12-47) → depletion ~slot 37
      - Cheapest eligible slot before depletion: slot 0 (0.05 EUR/kWh)
      → charge_plan schedules slot 0 → CHEAP_TARIFF fires
      → EXPENSIVE_TARIFF fires during the expensive period

    The 20h neutral interval prevents LOCF from carrying 0.40 into flexible slots,
    which would push the median toward 0.40 and raise expensive_threshold above 0.40.

    sorted tariffs: [0.05×12, 0.20×(240+24), 0.40×12] → median = 0.20
    eff = sqrt(0.92) ≈ 0.959
    cheap_threshold     = 0.20 × 0.959 = 0.192  →  0.05 < 0.192  → CHEAP_TARIFF ✓
    expensive_threshold = 0.20 / 0.959 = 0.209  →  0.40 > 0.209  → EXPENSIVE_TARIFF ✓
    """
    now = datetime.now(timezone.utc)
    r = vtn_post("/events", context.vtn_token, json={
        "programID": context.saved_program_id,
        "eventName": f"ev-scenario-g-{uuid.uuid4().hex[:8]}",
        "priority": 1,
        "intervals": [
            {
                "id": 0,
                "intervalPeriod": {
                    "start": now.strftime("%Y-%m-%dT%H:%M:%SZ"),
                    "duration": "PT1H",
                },
                "payloads": [{"type": "PRICE", "values": [0.05]}],
            },
            {
                "id": 1,
                "intervalPeriod": {
                    "start": (now + timedelta(hours=1)).strftime("%Y-%m-%dT%H:%M:%SZ"),
                    "duration": "PT3H",
                },
                "payloads": [{"type": "PRICE", "values": [0.40]}],
            },
            {
                "id": 2,
                "intervalPeriod": {
                    "start": (now + timedelta(hours=4)).strftime("%Y-%m-%dT%H:%M:%SZ"),
                    "duration": "PT20H",
                },
                "payloads": [{"type": "PRICE", "values": [0.20]}],
            },
        ],
    })
    r.raise_for_status()
    context.planner_event_id = r.json().get("id")


# ── When: poll for plan import cap ───────────────────────────────────────────

@when("I wait for the VEN /plan to have firm slots with import_cap_kw at most {limit:f}")
def step_wait_for_plan_import_cap(context, limit):
    """Poll /plan until at least one firm slot has import_cap_kw <= limit."""
    def fetch():
        r = ven_get("/plan")
        if not r.ok:
            return None
        plan = r.json()
        slots = plan.get("firm_slots", [])
        if not slots:
            return None
        if any(s.get("import_cap_kw", float("inf")) <= limit + 0.1 for s in slots):
            return plan
        return None

    context.ven_plan = poll_until(
        fetch,
        lambda x: x is not None,
        timeout=60,
        description=f"VEN /plan has a firm slot with import_cap_kw <= {limit}",
    )


# ── When: combined tariff-reason poll for scenario g ────────────────────────

@when('I wait for both "EXPENSIVE_TARIFF" and "CHEAP_TARIFF" PlanSteps for asset "{asset_id}"')
def step_wait_for_both_tariff_reasons(context, asset_id):
    """Poll until the plan has BOTH EXPENSIVE_TARIFF and CHEAP_TARIFF for the asset
    in the same plan snapshot.  This avoids race conditions when they occupy
    different slots of the same plan cycle."""
    def fetch():
        r = ven_get("/plan")
        if not r.ok:
            return None
        plan = r.json()
        steps = plan.get("steps", [])
        bat_steps = [s for s in steps if s.get("asset_id") == asset_id]
        kinds = {s.get("reason", {}).get("kind") for s in bat_steps}
        # Diagnostic: print the distribution every call for debugging
        if bat_steps:
            from collections import Counter
            cnt = Counter(s.get("reason", {}).get("kind") for s in bat_steps)
            print(f"  [debug] {asset_id} reason counts: {dict(cnt)}")
        if "EXPENSIVE_TARIFF" in kinds and "CHEAP_TARIFF" in kinds:
            context.bat_both_plan = plan
            return plan
        return None

    poll_until(
        fetch,
        lambda x: x is not None,
        timeout=120,
        description=f"VEN /plan has both EXPENSIVE_TARIFF and CHEAP_TARIFF for '{asset_id}'",
    )


# ── Then: plan allocation assertions ─────────────────────────────────────────

@then('a "{reason_kind}" PlanStep for "{asset_id}" has setpoint_kw less than 0.0')
def step_plan_step_setpoint_lt(context, reason_kind, asset_id):
    """Assert that context.bat_both_plan contains a step with the given reason and negative setpoint."""
    plan = context.bat_both_plan
    steps = [
        s for s in plan.get("steps", [])
        if s.get("asset_id") == asset_id and s.get("reason", {}).get("kind") == reason_kind
    ]
    assert steps, f"No {reason_kind} step found for asset '{asset_id}'"
    assert any(s.get("setpoint_kw", 0.0) < -1e-6 for s in steps), (
        f"No {reason_kind} step for '{asset_id}' has setpoint_kw < 0. "
        f"Values: {[s.get('setpoint_kw') for s in steps]}"
    )


@then('a "{reason_kind}" PlanStep for "{asset_id}" has setpoint_kw greater than 0.0')
def step_plan_step_setpoint_gt(context, reason_kind, asset_id):
    """Assert that context.bat_both_plan contains a step with the given reason and positive setpoint."""
    plan = context.bat_both_plan
    steps = [
        s for s in plan.get("steps", [])
        if s.get("asset_id") == asset_id and s.get("reason", {}).get("kind") == reason_kind
    ]
    assert steps, f"No {reason_kind} step found for asset '{asset_id}'"
    assert any(s.get("setpoint_kw", 0.0) > 1e-6 for s in steps), (
        f"No {reason_kind} step for '{asset_id}' has setpoint_kw > 0. "
        f"Values: {[s.get('setpoint_kw') for s in steps]}"
    )


@then("all EV allocations in capped firm slots are at most {kw:f} kW")
def step_ev_alloc_in_capped_slots(context, kw):
    """In every firm slot where import_cap_kw <= kw+0.5, the EV allocation must be <= kw."""
    r = ven_get("/plan")
    r.raise_for_status()
    plan = r.json()
    violations = []
    for slot in plan.get("firm_slots", []):
        cap = slot.get("import_cap_kw", float("inf"))
        if cap > kw + 0.5:
            continue  # Slot is not import-capped; skip
        ev_power = sum(
            a.get("power_kw", 0.0)
            for a in slot.get("allocations", [])
            if a.get("asset_id") == "ev"
        )
        if ev_power > kw + 1e-6:
            violations.append(
                f"slot {slot.get('slot_index')}: import_cap={cap} kW, EV={ev_power:.2f} kW"
            )
    assert not violations, (
        f"EV over-allocated in {len(violations)} import-capped firm slot(s):\n"
        + "\n".join(violations)
    )
