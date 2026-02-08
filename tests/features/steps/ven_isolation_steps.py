"""Step definitions for VEN isolation tests.

These tests verify that a VEN can never see another VEN's data
through any VTN API call.
"""

from behave import given, when, then
from features.helpers.api_client import (
    get_token_value,
    vtn_get,
    vtn_post,
)


# ── token helpers ───────────────────────────────────────────────────────────

@given("I have a VEN-1 token")
def step_ven1_token(context):
    context.ven1_token = get_token_value("ven-1", "ven-1")


@given("I have a VEN-2 token")
def step_ven2_token(context):
    context.ven2_token = get_token_value("ven-2", "ven-2")


# ── event creation with name ────────────────────────────────────────────────

@given('I create an event for the saved program named "{name}"')
def step_create_named_event(context, name):
    r = vtn_post(
        "/events",
        context.vtn_token,
        json={
            "programID": context.saved_program_id,
            "eventName": name,
            "intervals": [
                {"id": 0, "payloads": [{"type": "SIMPLE", "values": [1.0]}]},
            ],
        },
    )
    r.raise_for_status()
    context.iso_event = r.json()


# ── report submission directly to VTN ───────────────────────────────────────

@when('VEN-1 submits a report to VTN for event "{name}" with clientName "{client}"')
def step_ven1_submit_report(context, name, client):
    r = vtn_post(
        "/reports",
        context.ven1_token,
        json={
            "programID": context.iso_event["programID"],
            "eventID": context.iso_event["id"],
            "clientName": client,
            "resources": [],
        },
    )
    assert r.status_code == 201, f"VEN-1 report creation failed: {r.status_code} {r.text[:200]}"
    context.ven1_report = r.json()


@when('VEN-2 submits a report to VTN for event "{name}" with clientName "{client}"')
def step_ven2_submit_report(context, name, client):
    r = vtn_post(
        "/reports",
        context.ven2_token,
        json={
            "programID": context.iso_event["programID"],
            "eventID": context.iso_event["id"],
            "clientName": client,
            "resources": [],
        },
    )
    assert r.status_code == 201, f"VEN-2 report creation failed: {r.status_code} {r.text[:200]}"
    context.ven2_report = r.json()


# ── report isolation assertions ─────────────────────────────────────────────

@then("VEN-1 querying VTN reports sees only its own reports")
def step_ven1_sees_own_reports(context):
    r = vtn_get("/reports", context.ven1_token)
    r.raise_for_status()
    reports = r.json()
    for report in reports:
        assert report["id"] != context.ven2_report["id"], (
            f"VEN-1 can see VEN-2's report {report['id']} — isolation violated!"
        )
    own = [rpt for rpt in reports if rpt["id"] == context.ven1_report["id"]]
    assert len(own) == 1, f"VEN-1 cannot see its own report"


@then("VEN-2 querying VTN reports sees only its own reports")
def step_ven2_sees_own_reports(context):
    r = vtn_get("/reports", context.ven2_token)
    r.raise_for_status()
    reports = r.json()
    for report in reports:
        assert report["id"] != context.ven1_report["id"], (
            f"VEN-2 can see VEN-1's report {report['id']} — isolation violated!"
        )
    own = [rpt for rpt in reports if rpt["id"] == context.ven2_report["id"]]
    assert len(own) == 1, f"VEN-2 cannot see its own report"


@then('business user querying VTN reports for event "{name}" sees both VEN reports')
def step_business_sees_both(context, name):
    r = vtn_get("/reports", context.vtn_token)
    r.raise_for_status()
    reports = r.json()
    event_id = context.iso_event["id"]
    event_reports = [rpt for rpt in reports if rpt.get("eventID") == event_id]
    ids = {rpt["id"] for rpt in event_reports}
    assert context.ven1_report["id"] in ids, "Business user cannot see VEN-1 report"
    assert context.ven2_report["id"] in ids, "Business user cannot see VEN-2 report"


# ── report by-ID isolation ──────────────────────────────────────────────────

@then("VEN-1 cannot retrieve VEN-2 report by ID")
def step_ven1_cannot_get_ven2_report(context):
    r = vtn_get(f"/reports/{context.ven2_report['id']}", context.ven1_token)
    assert r.status_code in (404, 403), (
        f"VEN-1 retrieved VEN-2's report by ID — expected 404/403, got {r.status_code}"
    )


@then("VEN-2 cannot retrieve VEN-1 report by ID")
def step_ven2_cannot_get_ven1_report(context):
    r = vtn_get(f"/reports/{context.ven1_report['id']}", context.ven2_token)
    assert r.status_code in (404, 403), (
        f"VEN-2 retrieved VEN-1's report by ID — expected 404/403, got {r.status_code}"
    )


# ── VEN record isolation ───────────────────────────────────────────────────

@when("VEN-1 queries VTN for VENs")
def step_ven1_query_vens(context):
    r = vtn_get("/vens", context.ven1_token)
    r.raise_for_status()
    context.ven1_vens = r.json()


@then("VEN-1 sees only its own VEN record")
def step_ven1_own_record(context):
    assert len(context.ven1_vens) == 1, (
        f"VEN-1 sees {len(context.ven1_vens)} VEN records, expected 1"
    )
    assert context.ven1_vens[0]["venName"] == "ven-1-name", (
        f"VEN-1 sees wrong VEN: {context.ven1_vens[0]['venName']}"
    )


@when("VEN-2 queries VTN for VENs")
def step_ven2_query_vens(context):
    r = vtn_get("/vens", context.ven2_token)
    r.raise_for_status()
    context.ven2_vens = r.json()


@then("VEN-2 sees only its own VEN record")
def step_ven2_own_record(context):
    assert len(context.ven2_vens) == 1, (
        f"VEN-2 sees {len(context.ven2_vens)} VEN records, expected 1"
    )
    assert context.ven2_vens[0]["venName"] == "ven-2", (
        f"VEN-2 sees wrong VEN: {context.ven2_vens[0]['venName']}"
    )
