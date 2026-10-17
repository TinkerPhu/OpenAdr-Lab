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
    vtn_post,
)
from features.helpers.wait import poll_until
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


def _forecast():
    r = bff_get("/api/fleet/forecast")
    return r.json() if r.ok else None


@when("within {seconds:d} seconds the fleet forecast has slots")
def step_forecast_has_slots(context, seconds):
    poll_until(
        _forecast,
        lambda f: f and len(f["slots"]) > 0 and all(s["vens"] >= 1 for s in f["slots"]),
        timeout=seconds,
        interval=5,
        description="the fleet forecast summing at least one VEN's report",
    )


@then("within {seconds:d} seconds the fleet forecast has a before series")
def step_forecast_has_before(context, seconds):
    poll_until(
        _forecast,
        lambda f: f and f.get("before") and len(f["before"]["slots"]) > 0 and f["before"]["changed_at"],
        timeout=seconds,
        interval=5,
        description="the fleet forecast keeping the sum from before the switch",
    )
