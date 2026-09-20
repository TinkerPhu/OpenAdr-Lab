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


@when("I wait for the fleet to have reported")
def step_wait_for_any_report(context):
    """Reports arrive on the VEN's own cadence, so the contract assertions wait
    for something to assert against rather than pass vacuously on an empty list."""

    def fetch():
        context.response = bff_get("/api/reports")
        return context.response.json()

    poll_until(
        fetch,
        lambda body: isinstance(body, list) and len(body) > 0,
        timeout=120,
        interval=3,
        description="at least one report on the VTN",
    )


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
        "reportName": _unique_report_name("ven-1"),
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


def _reports(context):
    """The reports asserted against by the wire-contract steps.

    Deliberately not "skip if empty": a scenario that silently passes when
    there is nothing to check is the same failure it exists to catch. The
    fleet reports continuously, so the list being empty means we asked too
    early, which is what the waiting step is for.
    """
    data = context.response.json()
    assert isinstance(data, list), f"expected array, got {type(data).__name__}"
    assert data, (
        "no reports on the VTN — the wire contract cannot be checked against "
        "nothing; use the waiting step before these assertions"
    )
    return data


def _payloads_of(report, payload_type):
    """Every (interval, payload) pair of the given type, across all resources."""
    for resource in report.get("resources") or []:
        for interval in resource.get("intervals") or []:
            for payload in interval.get("payloads") or []:
                if payload.get("type") == payload_type:
                    yield interval, payload


@then('every report omits "{field}"')
def step_reports_omit_field(context, field):
    for r in _reports(context):
        assert field not in r, (
            f"report {r.get('id')} still carries '{field}'; 3.1 removed it "
            f"({sorted(r.keys())})"
        )


@then("every report names its event")
def step_reports_name_their_event(context):
    for r in _reports(context):
        assert r.get("eventID"), (
            f"report {r.get('id')} has no eventID — in 3.1 that is a report's "
            "only link to the object it reports on"
        )


@then('every USAGE payload is declared as "{units}"')
def step_usage_declared(context, units):
    for r in _reports(context):
        if not any(True for _ in _payloads_of(r, "USAGE")):
            continue
        descriptors = r.get("payloadDescriptors") or []
        usage = next((d for d in descriptors if d.get("payloadType") == "USAGE"), None)
        assert usage is not None, (
            f"report {r.get('id')} sends USAGE with no payloadDescriptor — its "
            "unit would live only in the reader's head (GB-50)"
        )
        assert usage.get("units") == units, (
            f"report {r.get('id')} declares USAGE as {usage.get('units')!r}, "
            f"expected {units!r}"
        )


@then("every USAGE interval states the window it covers")
def step_usage_interval_has_window(context):
    for r in _reports(context):
        for interval, _payload in _payloads_of(r, "USAGE"):
            period = interval.get("intervalPeriod") or {}
            assert period.get("start") and period.get("duration"), (
                f"report {r.get('id')} interval {interval.get('id')} carries "
                "USAGE without an intervalPeriod — USAGE is energy *over an "
                f"interval*, so it cannot be read without one (got {period!r})"
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
