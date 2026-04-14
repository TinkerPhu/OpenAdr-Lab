"""Step definitions for EV charging scenarios (Chunk 4).

Covers: IMPORT_CAPACITY_LIMIT, user-request path.
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


# ── Given: POST packet directly (explicit packet creation) ───────────────────

@given("I POST an EV packet with target_soc {soc:f} and latest_end_h {hours:f}")
def step_given_post_ev_packet(context, soc, hours):
    from datetime import datetime, timedelta, timezone
    latest_end = (datetime.now(timezone.utc) + timedelta(hours=hours)).strftime(
        "%Y-%m-%dT%H:%M:%SZ"
    )
    r = ven_post("/packets", json={
        "asset_id": "ev",
        "target_soc": soc,
        "target_energy_kwh": None,
        "latest_end": latest_end,
    })
    r.raise_for_status()
    context.last_created_packet = r.json()


# ── When: poll for plan import cap ───────────────────────────────────────────

@when("I wait for the VEN /plan to have a slot with import_cap_kw at most {limit:f}")
def step_wait_for_plan_import_cap(context, limit):
    """Poll /plan until at least one slot has import_cap_kw <= limit."""
    def fetch():
        r = ven_get("/plan")
        if not r.ok:
            return None
        plan = r.json()
        slots = plan.get("slots", [])
        if not slots:
            return None
        if any(s.get("import_cap_kw", float("inf")) <= limit + 0.1 for s in slots):
            return plan
        return None

    context.ven_plan = poll_until(
        fetch,
        lambda x: x is not None,
        timeout=60,
        description=f"VEN /plan has a slot with import_cap_kw <= {limit}",
    )


# ── Then: plan allocation assertions ─────────────────────────────────────────

@then("EV allocation in all capped plan slots is at most {kw:f} kW")
def step_ev_alloc_in_capped_slots(context, kw):
    """In every slot where import_cap_kw <= kw+0.5, the EV allocation power_kw must be <= kw+0.1.

    Used for zero-cap scenarios where base_load creates unavoidable net import
    but EV should receive zero allocation.
    """
    r = ven_get("/plan")
    r.raise_for_status()
    plan = r.json()
    violations = []
    for slot in plan.get("slots", []):
        cap = slot.get("import_cap_kw", float("inf"))
        if cap > kw + 0.5:
            continue  # slot is not import-capped; skip
        ev_power = sum(
            a.get("power_kw", 0.0)
            for a in slot.get("allocations", [])
            if a.get("asset_id") == "ev"
        )
        if ev_power > kw + 0.1:
            violations.append(
                f"slot {slot.get('slot_index')}: import_cap={cap:.1f} kW, ev_power={ev_power:.2f} kW"
            )
    assert not violations, (
        f"EV allocation exceeded zero cap in {len(violations)} slot(s):\n"
        + "\n".join(violations)
    )


@then("net import in all capped plan slots is at most {kw:f} kW")
def step_net_import_in_capped_slots(context, kw):
    """In every slot where import_cap_kw <= kw+0.5, net_import_kw must be <= kw+0.1.

    Checks total grid import rather than per-asset EV power: the MILP may charge
    EV above the cap value while simultaneously discharging the home battery to
    keep net import within the contractual limit.
    """
    r = ven_get("/plan")
    r.raise_for_status()
    plan = r.json()
    violations = []
    for slot in plan.get("slots", []):
        cap = slot.get("import_cap_kw", float("inf"))
        if cap > kw + 0.5:
            continue  # slot is not import-capped; skip
        net_import = slot.get("net_import_kw", 0.0)
        if net_import > kw + 0.1:  # allow small MILP slack tolerance
            violations.append(
                f"slot {slot.get('slot_index')}: import_cap={cap:.1f} kW, net_import={net_import:.2f} kW"
            )
    assert not violations, (
        f"Net import exceeded cap in {len(violations)} import-capped slot(s):\n"
        + "\n".join(violations)
    )
