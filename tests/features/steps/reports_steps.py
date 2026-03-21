import requests
from behave import when, then
from features.helpers.api_client import bff_get, VEN_BASE_URL
from features.helpers.wait import poll_until


@when("I list reports via BFF")
def step_list_bff_reports(context):
    context.response = bff_get("/api/reports")


@then("the response is a JSON array")
def step_response_is_array(context):
    data = context.response.json()
    assert isinstance(data, list), f"Expected array, got {type(data).__name__}: {str(data)[:200]}"


@when("I wait for VEN-1 to have at least {count:d} event")
def step_wait_ven1_events(context, count):
    def fetch():
        return requests.get(f"{VEN_BASE_URL}/events", timeout=10).json()

    context.ven1_events = poll_until(
        fetch,
        lambda events: len(events) >= count,
        timeout=30,
        description=f"VEN-1 has >= {count} events",
    )


@when("I submit a report via VEN-1 for the first event")
def step_submit_report_ven1(context):
    events = requests.get(f"{VEN_BASE_URL}/events", timeout=10).json()
    assert len(events) > 0, "VEN-1 has no events to report on"
    event = events[0]
    payload = {
        "programID": event.get("programID", ""),
        "eventID": event["id"],
        "clientName": "ven-1",
        "resources": [],
    }
    context.report_response = requests.post(
        f"{VEN_BASE_URL}/reports", json=payload, timeout=10
    )
    context.submitted_report = payload


@then("the VEN report submission response status is {status:d}")
def step_report_status(context, status):
    assert context.report_response.status_code == status, (
        f"Expected {status}, got {context.report_response.status_code}: "
        f"{context.report_response.text[:200]}"
    )


@then("the report appears in VEN-1 report list")
def step_report_in_ven1(context):
    def fetch():
        return requests.get(f"{VEN_BASE_URL}/reports", timeout=10).json()

    reports = poll_until(
        fetch,
        lambda rs: any(
            r.get("clientName") == "ven-1" and r.get("eventID") == context.submitted_report["eventID"]
            for r in rs
        ),
        timeout=30,
        description="Report appears in VEN-1",
    )
    assert len(reports) > 0


@then("the report appears in BFF report list")
def step_report_in_bff(context):
    def fetch():
        r = bff_get("/api/reports")
        r.raise_for_status()
        return r.json()

    reports = poll_until(
        fetch,
        lambda rs: any(
            r.get("clientName") == "ven-1" and r.get("eventID") == context.submitted_report["eventID"]
            for r in rs
        ),
        timeout=30,
        description="Report appears in BFF",
    )
    assert len(reports) > 0
