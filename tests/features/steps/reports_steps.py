import requests
from behave import when, then
from features.helpers.api_client import bff_get, VEN_BASE_URL, HTTP_TIMEOUT
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
    # When a specific program was created in this scenario, wait for VEN-1 to
    # discover an event for *that* program.  This prevents stale cached events
    # from satisfying the condition before VEN-1 has polled the new program.
    program_id = getattr(context, "saved_program_id", None)

    def fetch():
        return requests.get(f"{VEN_BASE_URL}/events", timeout=HTTP_TIMEOUT).json()

    if program_id:
        predicate = lambda events: any(e.get("programID") == program_id for e in events)
        desc = f"VEN-1 has an event for program {program_id}"
    else:
        predicate = lambda events: len(events) >= count
        desc = f"VEN-1 has >= {count} events"

    context.ven1_events = poll_until(fetch, predicate, timeout=90, description=desc)


@when("I submit a report via VEN-1 for the first event")
def step_submit_report_ven1(context):
    events = requests.get(f"{VEN_BASE_URL}/events", timeout=HTTP_TIMEOUT).json()
    assert len(events) > 0, "VEN-1 has no events to report on"
    # Prefer the event that belongs to the program created in this scenario so
    # that stale cached events (from a previous run) are not accidentally used.
    program_id = getattr(context, "saved_program_id", None)
    if program_id:
        matching = [e for e in events if e.get("programID") == program_id]
        event = matching[0] if matching else events[0]
    else:
        event = events[0]
    payload = {
        "programID": event.get("programID", ""),
        "eventID": event["id"],
        "clientName": "ven-1",
        "reportName": "TELEMETRY_USAGE",
        "resources": [],
    }
    context.report_response = requests.post(
        f"{VEN_BASE_URL}/reports", json=payload, timeout=HTTP_TIMEOUT
    )
    context.submitted_report = payload


@then("the report appears in VEN-1 report list")
def step_report_in_ven1(context):
    def fetch():
        return requests.get(f"{VEN_BASE_URL}/reports", timeout=HTTP_TIMEOUT).json()

    reports = poll_until(
        fetch,
        lambda rs: any(
            r.get("clientName") == "ven-1" and r.get("eventID") == context.submitted_report["eventID"]
            for r in rs
        ),
        timeout=60,
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
        timeout=60,
        description="Report appears in BFF",
    )
    assert len(reports) > 0


@when("I POST to VEN-1 reports with a valid OadrReportBody")
def step_post_valid_report_body(context):
    payload = {
        "programID": "test-prog",
        "eventID": "test-evt",
        "clientName": "ven-1",
        "reportName": "TELEMETRY_USAGE",
        "resources": [],
    }
    context.report_response = requests.post(
        f"{VEN_BASE_URL}/reports", json=payload, timeout=HTTP_TIMEOUT
    )
    context.submitted_report = payload


@then("the response body echoes back the submitted report fields")
def step_response_echoes_report(context):
    body = context.report_response.json()
    submitted = context.submitted_report
    assert body.get("programID") == submitted["programID"], f"programID mismatch: {body}"
    assert body.get("clientName") == submitted["clientName"], f"clientName mismatch: {body}"
    assert body.get("reportName") == submitted["reportName"], f"reportName mismatch: {body}"


@when("I POST to VEN-1 reports with a body missing programID")
def step_post_missing_program_id(context):
    payload = {
        "clientName": "ven-1",
        "reportName": "TELEMETRY_USAGE",
        "resources": [],
    }
    context.report_response = requests.post(
        f"{VEN_BASE_URL}/reports", json=payload, timeout=HTTP_TIMEOUT
    )


@then("the VEN report submission response status is {status:d}")
def step_report_status(context, status):
    assert context.report_response.status_code == status, (
        f"Expected {status}, got {context.report_response.status_code}: "
        f"{context.report_response.text[:200]}"
    )
