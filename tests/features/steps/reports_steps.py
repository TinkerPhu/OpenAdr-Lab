import uuid

import requests
from behave import when, then
from features.helpers.api_client import bff_get, VEN_BASE_URL, HTTP_TIMEOUT
from features.helpers.wait import poll_until


def _unique_report_name(client_name, base="TELEMETRY-REPORT"):
    return f"{client_name}-{base}-{uuid.uuid4().hex[:6]}".upper()


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
        "reportName": _unique_report_name("ven-1"),
        "resources": [],
    }
    context.report_response = requests.post(
        f"{VEN_BASE_URL}/reports", json=payload, timeout=HTTP_TIMEOUT
    )
    context.submitted_report = payload


@when("the report appears in VEN-1 report list")
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


@then("the response body echoes back the submitted report fields")
def step_response_echoes_report(context):
    """`eventID`, not `programID`: 3.1 removed the latter from a report."""
    body = context.report_response.json()
    submitted = context.submitted_report
    for field in ("eventID", "clientName", "reportName"):
        assert body.get(field) == submitted[field], f"{field} mismatch: {body}"


def _submitted_report_in(reports, context):
    name = context.submitted_report["reportName"]
    return next((r for r in reports if r.get("reportName") == name), None)


@when('I update the submitted report via VEN-1 with a resource named "{resource_name}"')
def step_update_report_ven1(context, resource_name):
    """PUT the whole report under the id the VTN gave it, as the Reports page's edit form does."""
    reports = requests.get(f"{VEN_BASE_URL}/reports", timeout=HTTP_TIMEOUT).json()
    report = _submitted_report_in(reports, context)
    assert report is not None, f"VEN-1 does not list {context.submitted_report['reportName']}"
    payload = {
        key: context.submitted_report[key] for key in ("eventID", "clientName", "reportName")
    }
    payload["resources"] = [{"resourceName": resource_name, "intervals": []}]
    context.report_response = requests.put(
        f"{VEN_BASE_URL}/reports/{report['id']}", json=payload, timeout=HTTP_TIMEOUT
    )


@then('the BFF report list shows the submitted report with a resource named "{resource_name}"')
def step_bff_report_has_resource(context, resource_name):
    def fetch():
        r = bff_get("/api/reports")
        r.raise_for_status()
        return _submitted_report_in(r.json(), context)

    poll_until(
        fetch,
        lambda report: report is not None and any(
            res.get("resourceName") == resource_name for res in report.get("resources", [])
        ),
        timeout=60,
        description=f"BFF shows the report with resource {resource_name}",
    )


@when("I POST to VEN-1 reports with a body missing eventID")
def step_post_missing_event_id(context):
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


@then("the VEN report submission response is a client error")
def step_report_is_client_error(context):
    """The VEN forwards to the VTN, which refuses the body, and the VEN surfaces
    that as a 4xx or a 502 carrying the VTN's reason. Either is a refusal; what
    matters is that an unidentifiable report does not quietly succeed."""
    status = context.report_response.status_code
    body = context.report_response.text[:300]
    assert status >= 400, f"expected a refusal, got {status}: {body}"
    assert "eventID" in body or status == 422, (
        f"the refusal should say which field was missing, got {status}: {body}"
    )
