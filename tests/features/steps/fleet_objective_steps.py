"""Step definitions for fleet objective control (openspec `opportunistic-objective` D10, D11).

Commands go through the BFF as the VTN UI sends them; what each VEN made of them is read from
its own `GET /plan/objective`, and what the BFF knows from `GET /api/fleet/power`. Every command
a scenario sends is released when it ends, so a retained command cannot outlive its scenario
(a later VEN restart would apply it again).
"""

from datetime import datetime, timedelta, timezone

from behave import given, then, when
from features.helpers.api_client import (
    VEN2_BASE_URL,
    VEN_BASE_URL,
    bff_get,
    bff_put,
    vtn_get,
    vtn_post,
)
from features.helpers.ui import tid
from features.helpers.wait import poll_until
import re

import requests

VEN_URLS = {"VEN-1": VEN_BASE_URL, "VEN-2": VEN2_BASE_URL}


def _release(ven):
    r = bff_put(f"/api/fleet/vens/{ven}/objective", json={"objective": None})
    if r.status_code != 200:
        print(f"[cleanup] fleet release of {ven}: {r.status_code} {r.text[:200]}")


def _commanded(context, ven):
    if ven not in getattr(context, "fleet_commanded", set()):
        context.fleet_commanded = getattr(context, "fleet_commanded", set()) | {ven}
        context.add_cleanup(_release, ven)


@given('the fleet commands "{ven}" to run "{objective}"')
@when('the fleet commands "{ven}" to run "{objective}"')
def step_fleet_commands(context, ven, objective):
    # Every change a scenario asserts happens after this instant, not the release an earlier
    # scenario's cleanup sent a moment before.
    context.commanded_at = datetime.now(timezone.utc)
    context.response = bff_put(f"/api/fleet/vens/{ven}/objective", json={"objective": objective})
    if context.response.status_code == 200:
        _commanded(context, ven)


@when('the fleet releases "{ven}"')
def step_fleet_releases(context, ven):
    r = bff_put(f"/api/fleet/vens/{ven}/objective", json={"objective": None})
    assert r.status_code == 200, f"release {ven}: {r.status_code} {r.text}"


@when('the fleet switches every VEN to "{objective}"')
def step_fleet_switches_all(context, objective):
    context.response = bff_put("/api/fleet/objective", json={"objective": objective})
    assert context.response.status_code == 202, context.response.text
    for ven in context.response.json()["vens"]:
        _commanded(context, ven)


@then('the fleet switch goes to "{first}" and "{second}", spaced apart')
def step_fleet_switch_targets(context, first, second):
    body = context.response.json()
    assert {first, second} <= set(body["vens"]), body
    assert body["spacingSeconds"] > 0, body


@then('the BFF refuses the command with {status:d} naming "{ven}" and "{value}"')
def step_bff_refuses(context, status, ven, value):
    assert context.response.status_code == status, context.response.text
    error = context.response.json()["error"]
    assert ven in error and value in error, error


def _objective(ven_label):
    r = requests.get(f"{VEN_URLS[ven_label]}/plan/objective", timeout=30)
    return r.json() if r.ok else None


@then('within {seconds:d} seconds {ven_label} reports "{objective}" set by "{source}"')
@given('within {seconds:d} seconds {ven_label} reports "{objective}" set by "{source}"')
def step_ven_reports(context, seconds, ven_label, objective, source):
    poll_until(
        lambda: _objective(ven_label),
        lambda o: o and o["objective"] == objective and o["source"] == source,
        timeout=seconds,
        interval=2,
        description=f"{ven_label} reporting {objective} set by {source}",
    )


@then('within {seconds:d} seconds {ven_label} reports its profile objective set by "{source}"')
def step_ven_reports_profile(context, seconds, ven_label, source):
    poll_until(
        lambda: _objective(ven_label),
        lambda o: o and o["objective"] == o["profile_objective"] and o["source"] == source,
        timeout=seconds,
        interval=2,
        description=f"{ven_label} back on its profile objective, set by {source}",
    )


@given("within {seconds:d} seconds {ven_label} runs its profile objective and no fleet command")
def step_ven_on_profile_objective(context, seconds, ven_label):
    # Who set it does not matter (an earlier feature may have set it from the VEN UI); a fleet
    # command an earlier scenario's cleanup released must have reached the VEN.
    poll_until(
        lambda: _objective(ven_label),
        lambda o: o and o["objective"] == o["profile_objective"] and o["source"] != "fleet",
        timeout=seconds,
        interval=2,
        description=f"{ven_label} on its profile objective, no fleet command in force",
    )


@then('within {seconds:d} seconds the BFF shows "{ven}" running "{objective}" with nothing pending')
def step_bff_shows(context, seconds, ven, objective):
    def listed():
        vens = bff_get("/api/fleet/power").json()["vens"]
        return next((v for v in vens if v["venName"] == ven), None)

    poll_until(
        listed,
        lambda v: v
        and (v.get("objective") or {}).get("objective") == objective
        and (v.get("command") or {}).get("objective") == objective
        and v.get("commandPending") is False,
        timeout=seconds,
        interval=2,
        description=f"the BFF showing {ven} on {objective}, nothing pending",
    )


@given("I create a standing event for the saved program asking for a usage forecast")
def step_create_forecast_event(context):
    # As the seeded fleet-monitoring event (scripts/seed_vtn.py): a long event whose one interval
    # cannot be the report grid, a USAGE_FORECAST descriptor at frequency 0 (ad hoc: the VEN
    # reports at once when its plan's objective changes, else every 15 minutes) on the VEN's own
    # intervals.
    start = (datetime.now(timezone.utc) - timedelta(minutes=1)).strftime("%Y-%m-%dT%H:%M:%SZ")
    r = vtn_post(
        "/events",
        context.vtn_token,
        json={
            "programID": context.saved_program_id,
            "eventName": "fleet-forecast",
            "duration": "PT2H",
            "intervalPeriod": {"start": start, "duration": "PT2H"},
            "intervals": [{"id": 0, "payloads": [{"type": "SIMPLE", "values": [0]}]}],
            "payloadDescriptors": [{"payloadType": "SIMPLE"}],
            "reportDescriptors": [
                {
                    "payloadType": "USAGE_FORECAST",
                    "readingType": "FORECAST",
                    "frequency": 0,
                    "reportIntervals": "OPEN_INTERVALS",
                    "historical": False,
                }
            ],
        },
    )
    r.raise_for_status()
    context.saved_event_id = r.json()["id"]


def _usage_forecast_reports(token, event_id):
    """VEN-1's USAGE_FORECAST reports for the event, as {id: modificationDateTime}, read from the
    VTN. The VEN upserts one report per obligation, so a newer forecast is the same id with a later
    modification time, not a new id."""
    r = vtn_get("/reports", token, params={"eventID": event_id})
    if not r.ok:
        return {}
    # The event is untargeted, so every VEN of the stack answers it under its own client.
    return {
        rep["id"]: rep.get("modificationDateTime")
        for rep in r.json()
        if rep.get("eventID") == event_id
        and rep.get("clientName") == "ven-1"
        and any(
            p.get("type") == "USAGE_FORECAST"
            for res in rep.get("resources", [])
            for iv in res.get("intervals", [])
            for p in iv.get("payloads", [])
        )
    }


@given("within {seconds:d} seconds VEN-1 has reported a usage forecast for the event")
def step_usage_forecast_reported(context, seconds):
    context.usage_forecast_reports = poll_until(
        lambda: _usage_forecast_reports(context.vtn_token, context.saved_event_id),
        lambda reports: len(reports) > 0,
        timeout=seconds,
        interval=5,
        description="VEN-1 reporting a usage forecast for the event",
    )


@then("within {seconds:d} seconds VEN-1 reports a newer usage forecast for the event")
def step_newer_usage_forecast(context, seconds):
    poll_until(
        lambda: _usage_forecast_reports(context.vtn_token, context.saved_event_id),
        lambda reports: any(
            context.usage_forecast_reports.get(rid) != modified for rid, modified in reports.items()
        ),
        timeout=seconds,
        interval=5,
        description="VEN-1 reporting a usage forecast after the switch, before its 15-minute cadence",
    )


@then('within {seconds:d} seconds the BFF notes when the objective of "{ven}" changed')
def step_bff_notes_change(context, seconds, ven):
    def changed_at():
        vens = bff_get("/api/fleet/power").json()["vens"]
        return next((v.get("objectiveChangedAt") for v in vens if v["venName"] == ven), None)

    context.objective_changed_at = poll_until(
        changed_at,
        lambda t: bool(t) and _parse(t) >= context.commanded_at,
        timeout=seconds,
        interval=2,
        description=f"the BFF noting when the objective of {ven} changed",
    )


def _parse(iso):
    return datetime.fromisoformat(iso.replace("Z", "+00:00"))


@then('the fleet plan as of just before that change lists a plan of "{ven}" adopted before it')
def step_plan_before_change(context, ven):
    # The Fleet page's instant (utils/fleetPlan.ts `instantBefore`): one second before the change.
    before = (_parse(context.objective_changed_at) - timedelta(seconds=1)).isoformat()
    r = bff_get("/api/fleet/plan", params={"at": before})
    assert r.ok, f"GET /api/fleet/plan?at={before}: {r.status_code} {r.text[:200]}"
    mine = [p for p in r.json()["plans"] if p["venName"] == ven]
    assert mine, f"as of {before} the fleet plan lists no plan of {ven}: {r.json()['plans']}"
    assert _parse(mine[0]["createdAt"]) <= _parse(before), mine[0]
    context.plan_before_change = mine[0]["planId"]


@then('within {seconds:d} seconds the BFF serves the plan of "{ven}" as an "{objective}" plan with a kW per asset')
def step_bff_serves_ven_plan(context, seconds, ven, objective):
    def plan():
        r = bff_get(f"/api/fleet/vens/{ven}/plan")
        return r.json() if r.ok else None

    context.served_plan = poll_until(
        plan,
        lambda p: p
        and p.get("objective") == objective
        and _parse(p["created_at"]) > _parse(context.objective_changed_at)
        and p.get("solve_status")
        and p.get("slots")
        and all("planned_kw_by_asset" in s for s in p["slots"]),
        timeout=seconds,
        interval=3,
        description=f"the BFF serving {ven}'s plan under {objective}, with each asset's kW",
    )


@then("that plan is not the one the fleet plan listed from before the change")
def step_served_plan_is_new(context):
    assert context.served_plan["id"] != context.plan_before_change, context.served_plan["id"]


@when("I open the Fleet page")
def step_open_fleet_page(context):
    page = context.ui.page
    page.click(tid("nav-fleet"))
    page.wait_for_selector(tid("fleet-total-card"), timeout=60000)


def _replan_progress(page):
    """(replanned, total) from the comparison card's label, or None until the card shows."""
    label = page.locator(tid("plan-replan-progress"))
    if label.count() == 0:
        return None
    m = re.search(r"(\d+) of (\d+) VENs replanned", label.first.text_content())
    return (int(m.group(1)), int(m.group(2))) if m else None


@then("within {seconds:d} seconds the Fleet page reports every VEN replanned since the switch")
def step_fleet_page_replanned(context, seconds):
    page = context.ui.page
    # The page polls its feeds; a reload per attempt would also lose nothing, but the poll is enough.
    poll_until(
        lambda: _replan_progress(page),
        lambda p: p is not None and p[1] > 0 and p[0] == p[1],
        timeout=seconds,
        interval=5,
        description="the Fleet page's replan progress reading 'n of n VENs replanned'",
    )


@then("the Fleet plan chart opens on accumulated energy")
def step_fleet_chart_accumulated(context):
    pressed = context.ui.page.locator(f'{tid("fleet-plan-quantity")} button[aria-pressed="true"]')
    assert pressed.text_content() == "Accumulated energy", pressed.text_content()


@when("I choose the power view of the Fleet plan chart")
def step_fleet_chart_choose_power(context):
    context.ui.page.click(f'{tid("fleet-plan-quantity")} button:has-text("Power")')


@then("the Fleet plan chart shows power")
def step_fleet_chart_power(context):
    pressed = context.ui.page.locator(f'{tid("fleet-plan-quantity")} button[aria-pressed="true"]')
    assert pressed.text_content() == "Power", pressed.text_content()
