"""Step definitions for WP4.1 (BL-28) EV user-request modes."""

from datetime import datetime, timedelta, timezone
from behave import given, when, then
from features.helpers.api_client import ven_get, ven_post, ven_delete
from features.helpers.wait import poll_until


def _post_mode_session(context, mode, budget_eur):
    departure = (datetime.now(timezone.utc) + timedelta(hours=8)).strftime(
        "%Y-%m-%dT%H:%M:%SZ"
    )
    body = {
        "asset_id": "ev",
        "target_soc": 0.9,
        "deadlines": [{"latest_end": departure}],
        "mode": mode,
    }
    if budget_eur is not None:
        body["budget_eur"] = budget_eur
    r = ven_post("/user-requests", json=body)
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
    # Only a notification raised from here on counts. The ring is cumulative and
    # survives between scenarios, so matching against all of it lets a re-run
    # "pass" in 0.1s on the previous run's notifications -- observed
    # 2026-09-22: three consecutive 0.1s passes of a scenario that injects 3 kW
    # and waits for two edges, none of which exercised anything.
    #
    # "New" cannot mean "a new id", though: the notifier deduplicates by
    # `dedup_key`, so raising the same notification again increments `count` and
    # advances `last_seen_at` on the *existing* entry. An id-based check was the
    # first fix here and it turned the false pass into a false failure -- the
    # edge fired, the ring said so, and the step never saw it. The fingerprint
    # below is what actually changes when a notification is raised again.
    def fingerprint(note):
        return (note.get("id"), note.get("count"), note.get("last_seen_at"))

    # The baseline is the ring as it stood at the *scenario's* first wait, not
    # at this step's. Waiting per-step assumes each edge is caused by the step
    # immediately before it, and the arbiter does not work that way: it carries
    # its own last-applied setpoint forward as the baseline
    # (`controller::arbiter::reconcile`), so removing a disturbance leaves the
    # correction itself deviating from plan and the lever engages and releases
    # on its own schedule. Observed 2026-09-22: active at :41, cleared at :43
    # -- before the inject was cleared -- then active again at :54 and held for
    # the full 300 s. Per-scenario scoping still cannot pass on an earlier
    # scenario's notifications, which is the property that matters; it just
    # stops asserting a causality the design never promised.
    if not hasattr(context, "notification_baseline"):
        r = ven_get("/notifications")
        context.notification_baseline = {fingerprint(n) for n in r.json()} if r.ok else set()
    before = context.notification_baseline

    def fetch():
        r = ven_get("/notifications")
        if not r.ok:
            return None
        return r.json()

    def has_text(notes):
        return notes is not None and any(
            text in n.get("message", "") and fingerprint(n) not in before for n in notes
        )

    # 300s, not 180s. Measured on a quiet Node1 (host load ~4, the settle gate
    # satisfied): the "Reactive correction cleared" edge landed 181.0s after
    # "active" -- one second past a 180s wait, so the scenario lost a race it was
    # always going to lose, on any host. The assertion is unchanged; only the
    # allowance is, and it is now well clear of the observed latency rather than
    # sitting exactly on it.
    context.notifications = poll_until(
        fetch,
        has_text,
        timeout=300,
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


def _asset_allocations(plan, asset_id):
    return [
        (slot.get("slot_index"), a.get("power_kw"))
        for slot in plan.get("slots", [])
        for a in slot.get("allocations", [])
        if a.get("asset_id") == asset_id and a.get("power_kw", 0.0) > 0.01
    ]


@then('the recomputed plan has no "{asset_id}" allocations')
def step_plan_has_no_asset_alloc(context, asset_id):
    # step_wait_plan_after_mode_session only guarantees a plan *stamped* after the
    # mode session was created — a cycle already in flight when the session lands
    # can still finish and get a fresh created_at without having read it (TOCTOU on
    # wall-clock freshness, not plan content; see comfort_steps.py's identical fix).
    # Re-poll /plan itself if the captured snapshot doesn't already satisfy this.
    if _asset_allocations(context.mode_plan, asset_id):
        def fetch():
            r = ven_get("/plan")
            return r.json() if r.ok else None

        context.mode_plan = poll_until(
            fetch,
            lambda plan: plan is not None and not _asset_allocations(plan, asset_id),
            timeout=30,
            interval=3,
            description=f"plan has no {asset_id} allocations",
        )
    offending = _asset_allocations(context.mode_plan, asset_id)
    assert not offending, f"expected no {asset_id} allocations, got {offending}"


@then("the mode EV session is deleted")
def step_delete_mode_session(context):
    request_id = context.mode_session["id"]
    r = ven_delete(f"/user-requests/{request_id}")
    assert r.status_code in (204, 404), f"unexpected status {r.status_code}"


@then("the sim inject state is reset")
def step_reset_inject(context):
    r = ven_post("/sim/inject/reset", json=None)
    assert r.ok, f"inject reset failed: {r.status_code}"
