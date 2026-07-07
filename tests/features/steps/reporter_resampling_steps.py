"""Step definitions for reporter multi-interval resampling (RF-05e)."""

import requests
from behave import given, when, then
from features.helpers.api_client import vtn_post, VEN_BASE_URL, HTTP_TIMEOUT
from features.helpers.wait import poll_until


@given('I create an event for the saved program with a reportDescriptor frequency of {freq_s:d} seconds')
def step_create_event_with_report_descriptor(context, freq_s):
    r = vtn_post(
        "/events",
        context.vtn_token,
        json={
            "programID": context.saved_program_id,
            "eventName": "resample-event",
            "intervals": [
                {"id": 0, "payloads": [{"type": "PRICE", "values": [0.25]}]},
            ],
            "reportDescriptors": [
                {
                    "payloadType": "USAGE",
                    "readingType": "DIRECT_READ",
                    "frequency": freq_s,
                    "repeat": 1,
                }
            ],
        },
    )
    r.raise_for_status()
    context.saved_event_id = r.json()["id"]


@given("I create an event for the saved program without a reportDescriptor")
def step_create_event_without_report_descriptor(context):
    r = vtn_post(
        "/events",
        context.vtn_token,
        json={
            "programID": context.saved_program_id,
            "eventName": "no-descriptor-event",
            "intervals": [
                {"id": 0, "payloads": [{"type": "SIMPLE", "values": [1.0]}]},
            ],
        },
    )
    r.raise_for_status()
    context.saved_event_id = r.json()["id"]


@when("I wait for VEN-1 to accumulate at least {secs:d} seconds of history")
def step_wait_ven1_history(context, secs):
    import time
    time.sleep(secs)


@when("I wait for VEN-1 to submit an obligation-driven report for the event")
def step_wait_ven1_obligation_report_for_event(context):
    # Obligations recur (R6): `fulfilled` stays false permanently and is no longer a
    # one-shot "done" signal — due_at advances in place instead. The actual thing this
    # scenario cares about is that a report was submitted, so poll /reports directly
    # rather than the obligation's internal fulfilled flag.
    event_id = context.saved_event_id

    def fetch():
        reports = requests.get(f"{VEN_BASE_URL}/reports", timeout=HTTP_TIMEOUT).json()
        return [r for r in reports if r.get("eventID") == event_id]

    matching = poll_until(
        fetch,
        lambda rs: len(rs) >= 1,
        timeout=60,
        interval=3,
        description=f"VEN-1 has submitted a report for event {event_id}",
    )
    context.ven1_reports = matching


@when("I wait for VEN-1 to submit at least {count:d} timer-driven report for the event")
@when("I wait for VEN-1 to submit at least {count:d} timer-driven reports for the event")
def step_wait_ven1_timer_reports_for_event(context, count):
    event_id = context.saved_event_id

    def fetch():
        reports = requests.get(f"{VEN_BASE_URL}/reports", timeout=HTTP_TIMEOUT).json()
        return [r for r in reports if r.get("eventID") == event_id]

    matching = poll_until(
        fetch,
        lambda rs: len(rs) >= count,
        timeout=90,
        interval=3,
        description=f"VEN-1 has >= {count} reports for event {event_id}",
    )
    context.ven1_reports = matching


@then("the latest VEN-1 report for the event has multiple intervals")
def step_report_has_multiple_intervals(context):
    reports = requests.get(f"{VEN_BASE_URL}/reports", timeout=HTTP_TIMEOUT).json()
    event_id = context.saved_event_id

    matching = [r for r in reports if r.get("eventID") == event_id]
    assert matching, f"No reports found for event {event_id}"

    report = matching[-1]
    resources = report.get("resources", [])
    assert resources, "Report has no resources"

    intervals = resources[0].get("intervals", [])
    context.report_intervals = intervals
    assert len(intervals) > 1, (
        f"Expected multiple intervals, got {len(intervals)}: {intervals}"
    )


@then("the latest VEN-1 report for the event has exactly {count:d} interval")
@then("the latest VEN-1 report for the event has exactly {count:d} intervals")
def step_report_has_exact_intervals(context, count):
    reports = requests.get(f"{VEN_BASE_URL}/reports", timeout=HTTP_TIMEOUT).json()
    event_id = context.saved_event_id

    matching = [r for r in reports if r.get("eventID") == event_id]
    assert matching, f"No reports found for event {event_id}"

    report = matching[-1]
    resources = report.get("resources", [])
    assert resources, "Report has no resources"

    intervals = resources[0].get("intervals", [])
    context.report_intervals = intervals
    assert len(intervals) == count, (
        f"Expected {count} intervals, got {len(intervals)}"
    )


@then("each interval has sequential ids starting from 0")
def step_intervals_sequential_ids(context):
    intervals = context.report_intervals
    for i, iv in enumerate(intervals):
        assert iv.get("id") == i, (
            f"Interval {i} has id {iv.get('id')}, expected {i}"
        )


@then('each interval has an intervalPeriod with start and duration "{duration}"')
def step_intervals_have_period(context, duration):
    intervals = context.report_intervals
    for i, iv in enumerate(intervals):
        period = iv.get("intervalPeriod", {})
        assert "start" in period, f"Interval {i} missing intervalPeriod.start"
        assert period.get("duration") == duration, (
            f"Interval {i} duration={period.get('duration')}, expected {duration}"
        )


@then("each interval contains a USAGE payload")
def step_intervals_have_usage(context):
    intervals = context.report_intervals
    for i, iv in enumerate(intervals):
        payloads = iv.get("payloads", [])
        types = [p.get("type") for p in payloads]
        assert "USAGE" in types, (
            f"Interval {i} missing USAGE payload, has: {types}"
        )


@then('each interval contains an OPERATING_STATE payload with value "{value}"')
def step_intervals_have_operating_state(context, value):
    intervals = context.report_intervals
    for i, iv in enumerate(intervals):
        payloads = iv.get("payloads", [])
        os_payloads = [p for p in payloads if p.get("type") == "OPERATING_STATE"]
        assert os_payloads, f"Interval {i} missing OPERATING_STATE payload"
        actual = os_payloads[0].get("values", [None])[0]
        assert actual == value, (
            f"Interval {i} OPERATING_STATE={actual}, expected {value}"
        )
