"""Step definitions for WP4.1 (BL-28) EV user-request modes."""

from datetime import datetime, timedelta, timezone
from behave import given, when, then
from features.helpers.api_client import ven_get, ven_post, ven_delete
from features.helpers.wait import poll_until


def _post_mode_session(context, mode, budget_eur):
    departure = (datetime.now(timezone.utc) + timedelta(hours=8)).strftime(
        "%Y-%m-%dT%H:%M:%SZ"
    )
    body = {"target_soc": 0.9, "departure_time": departure, "mode": mode}
    if budget_eur is not None:
        body["budget_eur"] = budget_eur
    r = ven_post("/ev-session", json=body)
    r.raise_for_status()
    context.mode_session_created_at = datetime.now(timezone.utc)
    context.mode_session = r.json()


@given('I POST a mode EV session with mode "{mode}" and budget {budget:f}')
def step_post_mode_session_budget(context, mode, budget):
    _post_mode_session(context, mode, budget)


@given('I POST a mode EV session with mode "{mode}" and no budget')
def step_post_mode_session_no_budget(context, mode):
    _post_mode_session(context, mode, None)


@given("the PV plan forecast is pinned to 0 kW")
def step_pin_pv_forecast_zero(context):
    # pv_plan_kw pins every horizon slot of the PV forecast (deterministic,
    # unlike pv_irradiance whose offset decays back toward the sin model).
    r = ven_post("/sim/inject", json={"pv_plan_kw": 0.0})
    r.raise_for_status()


@when('I wait for a user notification containing "{text}"')
def step_wait_for_notification(context, text):
    def fetch():
        r = ven_get("/notifications")
        if not r.ok:
            return None
        return r.json()

    def has_text(notes):
        return notes is not None and any(text in n.get("message", "") for n in notes)

    context.notifications = poll_until(
        fetch,
        has_text,
        timeout=180,
        interval=5,
        description=f"notification feed contains '{text}'",
    )


@when("I wait for the VEN plan to be recomputed after the mode session")
def step_wait_plan_after_mode_session(context):
    cutoff = context.mode_session_created_at

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

    context.mode_plan = poll_until(
        fetch,
        is_fresh,
        timeout=180,
        interval=5,
        description="VEN /plan recomputed after the mode session",
    )


@then('the recomputed plan has no "{asset_id}" allocations')
def step_plan_has_no_asset_alloc(context, asset_id):
    plan = context.mode_plan
    offending = [
        (slot.get("slot_index"), a.get("power_kw"))
        for slot in plan.get("slots", [])
        for a in slot.get("allocations", [])
        if a.get("asset_id") == asset_id and a.get("power_kw", 0.0) > 0.01
    ]
    assert not offending, f"expected no {asset_id} allocations, got {offending}"


@then("the mode EV session is deleted")
def step_delete_mode_session(context):
    r = ven_delete("/ev-session")
    assert r.status_code in (204, 404), f"unexpected status {r.status_code}"


@then("the sim inject state is reset")
def step_reset_inject(context):
    r = ven_post("/sim/inject/reset", json=None)
    assert r.ok, f"inject reset failed: {r.status_code}"
