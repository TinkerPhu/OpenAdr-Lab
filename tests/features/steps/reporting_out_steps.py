"""Step definitions for outbound flexibility/forecast reports (WP3.6 — BL-10, §8.8)."""

import requests
from behave import given, then
from features.helpers.api_client import vtn_post, ven_get, VEN_BASE_URL, HTTP_TIMEOUT
from features.helpers.wait import poll_until


@given('I create an event for the saved program with a reportDescriptor of type "{ptype}" reporting every {secs:d} seconds')
def step_create_event_with_typed_descriptor(context, ptype, secs):
    """Ask for a report every `secs` seconds, the way 3.1 expresses it.

    Three things have to line up, and getting any one wrong makes the report
    never arrive:

    * `frequency` counts *intervals*, not seconds (spec: "number of intervals
      that elapse between reports"), so the cadence is an interval `secs` long
      with `frequency: 1`. Passing seconds straight into `frequency` only ever
      worked because the VEN read the field in the wrong unit too.
    * the interval needs a real `intervalPeriod`; without one there is no
      interval length to count and the VEN applies its one-hour default.
    * the *event* has to outlive the interval. The VEN polls
      `/events?active=true`, so an event whose window is also `secs` long has
      already ended by the time it is fetched -- `event.duration` keeps it
      alive (and repeats the short interval across it, per User Guide 647).
    """
    from datetime import datetime, timedelta, timezone

    start = (datetime.now(timezone.utc) - timedelta(seconds=secs)).strftime(
        "%Y-%m-%dT%H:%M:%SZ"
    )
    r = vtn_post(
        "/events",
        context.vtn_token,
        json={
            "programID": context.saved_program_id,
            "eventName": f"descriptor-{ptype.lower().replace('_', '-')}",
            # Keeps the event active for an hour while its intervals stay
            # `secs` long; 3.1 repeats the interval sequence across the
            # event-level duration (User Guide 647).
            "duration": "PT1H",
            "intervalPeriod": {"start": start},
            "intervals": [
                {
                    "id": 0,
                    "intervalPeriod": {"start": start, "duration": f"PT{secs}S"},
                    "payloads": [{"type": "PRICE", "values": [0.25]}],
                },
            ],
            "reportDescriptors": [
                {
                    "payloadType": ptype,
                    "readingType": "DIRECT_READ",
                    "frequency": 1,
                    "repeat": 1,
                }
            ],
        },
    )
    r.raise_for_status()
    context.saved_event_id = r.json()["id"]


def _latest_report_intervals(context):
    reports = requests.get(f"{VEN_BASE_URL}/reports", timeout=HTTP_TIMEOUT).json()
    matching = [r for r in reports if r.get("eventID") == context.saved_event_id]
    assert matching, f"No reports found for event {context.saved_event_id}"
    resources = matching[-1].get("resources", [])
    assert resources, "Report has no resources"
    intervals = resources[0].get("intervals", [])
    assert intervals, "Report has no intervals"
    return intervals


@then('the latest VEN-1 report for the event has a "{ptype}" payload with a non-negative number value')
def step_report_payload_non_negative(context, ptype):
    intervals = _latest_report_intervals(context)
    payloads = [p for iv in intervals for p in iv.get("payloads", []) if p.get("type") == ptype]
    assert payloads, f"No '{ptype}' payload in report intervals: {intervals}"
    for p in payloads:
        value = p["values"][0]
        assert isinstance(value, (int, float)), f"'{ptype}' value not numeric: {value!r}"
        assert value >= 0, f"'{ptype}' value negative: {value}"


@then('every interval of the latest report has a "{ptype}" payload with a number value')
def step_every_interval_has_numeric_payload(context, ptype):
    intervals = _latest_report_intervals(context)
    for iv in intervals:
        matches = [p for p in iv.get("payloads", []) if p.get("type") == ptype]
        assert matches, f"Interval {iv.get('id')} lacks a '{ptype}' payload: {iv}"
        value = matches[0]["values"][0]
        assert isinstance(value, (int, float)), (
            f"Interval {iv.get('id')} '{ptype}' value not numeric: {value!r}"
        )


@then('every interval of the latest report has a "{ptype}" payload with value "{expected}"')
def step_every_interval_has_string_payload(context, ptype, expected):
    intervals = _latest_report_intervals(context)
    for iv in intervals:
        matches = [p for p in iv.get("payloads", []) if p.get("type") == ptype]
        assert matches, f"Interval {iv.get('id')} lacks a '{ptype}' payload: {iv}"
        value = matches[0]["values"][0]
        assert value == expected, (
            f"Interval {iv.get('id')} '{ptype}' value {value!r} != {expected!r}"
        )


@then("every interval of the latest report has an intervalPeriod start")
def step_every_interval_has_period_start(context):
    intervals = _latest_report_intervals(context)
    for iv in intervals:
        period = iv.get("intervalPeriod") or {}
        assert period.get("start"), (
            f"Interval {iv.get('id')} lacks intervalPeriod.start: {iv}"
        )


# ── R-43: GET /history/reports reflects real submissions ───────────────────

@then("VEN-1's report history includes an entry for the event")
def step_history_reports_includes_event(context):
    def fetch():
        r = ven_get("/history/reports")
        r.raise_for_status()
        # /history/reports is paginated (`history_page_route!` -> HistoryPage),
        # so the body is {"rows": [...], "total": N}, not a bare list. Iterating
        # the object directly would silently walk its *keys* (strings).
        return r.json()["rows"]

    rows = poll_until(
        fetch,
        lambda rs: any(r.get("event_id") == context.saved_event_id for r in rs),
        timeout=30,
        interval=2,
        description=f"GET /history/reports includes event {context.saved_event_id}",
    )
    assert any(r.get("event_id") == context.saved_event_id for r in rows), (
        f"No /history/reports row for event {context.saved_event_id}: {rows}"
    )


def _report_for_event(context):
    """The report this scenario's own event produced."""
    reports = requests.get(f"{VEN_BASE_URL}/reports", timeout=HTTP_TIMEOUT).json()
    matching = [r for r in reports if r.get("eventID") == context.saved_event_id]
    assert matching, f"no report for event {context.saved_event_id}"
    return matching[-1]


def _usage_payloads(report):
    """Every (interval, payload) pair of type USAGE, across all resources."""
    for resource in report.get("resources") or []:
        for interval in resource.get("intervals") or []:
            for payload in interval.get("payloads") or []:
                if payload.get("type") == "USAGE":
                    yield interval, payload


@then('the report for the event omits "{field}"')
def step_report_omits_field(context, field):
    report = _report_for_event(context)
    assert field not in report, (
        f"report still carries '{field}'; 3.1 removed it ({sorted(report.keys())})"
    )


@then("the report for the event names its event")
def step_report_names_its_event(context):
    report = _report_for_event(context)
    assert report.get("eventID"), (
        "report has no eventID — in 3.1 that is its only link to the object "
        "it reports on"
    )


@then('every USAGE payload in the report is declared as "{units}"')
def step_usage_declared_as(context, units):
    report = _report_for_event(context)
    assert any(True for _ in _usage_payloads(report)), "report carries no USAGE payload"
    descriptors = report.get("payloadDescriptors") or []
    usage = next((d for d in descriptors if d.get("payloadType") == "USAGE"), None)
    assert usage is not None, (
        "report sends USAGE with no payloadDescriptor — its unit would live "
        "only in the reader's head (GB-50)"
    )
    assert usage.get("units") == units, (
        f"USAGE declared as {usage.get('units')!r}, expected {units!r}"
    )


@then("every USAGE interval in the report states the window it covers")
def step_usage_interval_states_window(context):
    report = _report_for_event(context)
    for interval, _payload in _usage_payloads(report):
        period = interval.get("intervalPeriod") or {}
        assert period.get("start") and period.get("duration"), (
            f"interval {interval.get('id')} carries USAGE without an "
            "intervalPeriod — USAGE is energy *over an interval*, so it cannot "
            f"be read without one (got {period!r})"
        )
