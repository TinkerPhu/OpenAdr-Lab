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

@given("I create a cheap-then-expensive 2-interval PRICE event for the saved program")
def step_create_cheap_expensive_price_event(context):
    """Two consecutive 1-hour intervals: 0.05 EUR/kWh (cheap) then 0.40 EUR/kWh (expensive).

    With a default background tariff of ~0.20 EUR/kWh, the median of the firm-horizon
    slots lands around 0.20.  After applying efficiency scaling:
      cheap_threshold  ≈ 0.20 * sqrt(0.85) ≈ 0.184  →  0.05 < threshold → CHEAP_TARIFF
      expensive_threshold ≈ 0.20 / sqrt(0.85) ≈ 0.217 →  0.40 > threshold → EXPENSIVE_TARIFF
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
                    "duration": "PT1H",
                },
                "payloads": [{"type": "PRICE", "values": [0.40]}],
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


# ── Then: plan allocation assertions ─────────────────────────────────────────

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
