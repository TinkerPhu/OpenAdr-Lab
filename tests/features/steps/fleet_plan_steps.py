"""Step definitions for the fleet plan feed (fleet_plan_feed.feature)."""

from datetime import datetime, timedelta, timezone

from behave import given, when, then
from features.helpers.api_client import bff_get, ven_get, vtn_post
from features.helpers.tariff import parse_ts
from features.helpers.wait import poll_until


def _ven_plan_id():
    """The id of the plan the VEN is working to, or None before its first adoption."""
    resp = ven_get("/plan")
    if not resp.ok:
        return None
    plan = resp.json()
    return plan["id"] if isinstance(plan, dict) else None


def _fleet_plan(at=None):
    resp = bff_get("/api/fleet/plan", params={"at": at} if at else None)
    return resp.json() if resp.ok else None


def _carries(fleet_plan, plan_id):
    return fleet_plan is not None and any(p["planId"] == plan_id for p in fleet_plan["plans"])


@given("I wait for the fleet plan to carry the plan the VEN is working to")
@when("I wait for the fleet plan to carry the plan the VEN is working to")
def step_wait_fleet_plan_carries_ven_plan(context):
    """Both halves move, so each poll reads the VEN's current plan and asks the BFF for it."""
    def fetch():
        plan_id = _ven_plan_id()
        fleet = _fleet_plan()
        return (plan_id, fleet) if plan_id and _carries(fleet, plan_id) else None

    context.carried_plan_id, context.fleet_plan = poll_until(
        fetch,
        lambda found: found is not None,
        timeout=120,
        interval=3,
        description="the BFF's fleet plan carries the plan the VEN is working to",
    )


@then("the fleet plan states import and export in kW, never negative")
def step_fleet_plan_nonnegative(context):
    points = context.fleet_plan["points"]
    assert points, "the fleet plan has no grid points"
    for p in points:
        assert p["netImportKw"] >= 0 and p["netExportKw"] >= 0, p


@then("every grid point of the fleet plan says how many VENs it is built from")
def step_fleet_plan_counts_contributors(context):
    for p in context.fleet_plan["points"]:
        assert isinstance(p["contributingVens"], int) and p["contributingVens"] >= 1, p


@given("I remember the plan the VEN has and the instant it took effect")
def step_remember_instant(context):
    """The instant is the plan's own `createdAt`, not the wall clock: a VEN replans on its own
    schedule, so "now" may already belong to a successor of the plan being remembered."""
    context.earlier_plan_id = context.carried_plan_id
    context.remembered_at = next(
        p["createdAt"] for p in context.fleet_plan["plans"] if p["planId"] == context.carried_plan_id
    )


@when("I create a high-priced PRICE event for the saved program")
def step_create_high_price_event(context):
    start = (datetime.now(timezone.utc) + timedelta(hours=1)).replace(minute=0, second=0, microsecond=0)
    r = vtn_post(
        "/events",
        context.vtn_token,
        json={
            "programID": context.saved_program_id,
            "eventName": "fleet-plan-feed-price",
            "priority": 0,
            "payloadDescriptors": [{"payloadType": "PRICE", "units": "KWH", "currency": "EUR"}],
            "intervals": [
                {
                    "id": 0,
                    "intervalPeriod": {"start": start.strftime("%Y-%m-%dT%H:%M:%SZ"), "duration": "PT2H"},
                    "payloads": [{"type": "PRICE", "values": [0.9]}],
                }
            ],
        },
    )
    r.raise_for_status()
    context.rate_event_id = r.json()["id"]  # removed by the scenario cleanup


@when("I wait for the VEN to adopt a different plan")
def step_wait_ven_adopts_different_plan(context):
    poll_until(
        _ven_plan_id,
        lambda plan_id: plan_id is not None and plan_id != context.earlier_plan_id,
        timeout=120,
        interval=3,
        description="the VEN adopts a plan other than the one it had",
    )


@then("the fleet plan as of the remembered instant still carries the earlier plan")
def step_fleet_plan_earlier(context):
    then = _fleet_plan(context.remembered_at)
    listed = [(p["venName"], p["planId"], p["createdAt"]) for p in (then or {"plans": []})["plans"]]
    assert _carries(then, context.earlier_plan_id), (
        f"as of {context.remembered_at} the fleet plan lists {listed}, not {context.earlier_plan_id}"
    )


@then("the fleet plan now carries the later plan")
def step_fleet_plan_later(context):
    assert context.carried_plan_id != context.earlier_plan_id
    assert _carries(_fleet_plan(), context.carried_plan_id)


@then("the fleet plan holds a plan adopted after the editor's publish that the plan of a second before it did not")
def step_fleet_plan_answers_publish(context):
    """The question the tariff editor's comparison card asks, asked of the plan feed directly: the
    instant is the published event's own `createdDateTime` less one second (`instantBefore`), and
    a plan counts only if the VEN adopted it after the publish, so an earlier replan cannot pass."""
    created = parse_ts(context.published_event["createdDateTime"])
    before = _fleet_plan((created - timedelta(seconds=1)).strftime("%Y-%m-%dT%H:%M:%S.%fZ"))
    assert before is not None, "the plan feed answers nothing for the instant before the publish"
    had = {(p["venName"], p["planId"]) for p in before["plans"]}

    def newer_plans():
        now = _fleet_plan()
        return [
            p for p in (now or {"plans": []})["plans"]
            if (p["venName"], p["planId"]) not in had and parse_ts(p["createdAt"]) > created
        ]

    poll_until(
        newer_plans,
        bool,
        timeout=120,
        interval=3,
        description=f"a VEN adopts a plan after the publish at {created}",
    )

