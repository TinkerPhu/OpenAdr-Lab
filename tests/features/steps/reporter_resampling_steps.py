"""Step definitions for reporter multi-interval resampling (RF-05e)."""

import time
import uuid

import requests
from behave import given, when, then
from features.helpers.api_client import vtn_post, VEN_BASE_URL, HTTP_TIMEOUT
from features.helpers.wait import poll_until


@given('I create an event for the saved program reporting every {secs:d} seconds')
def step_create_event_with_report_descriptor(context, secs):
    """See `reporting_out_steps.step_create_event_with_typed_descriptor` for
    why cadence is expressed this way, and why the event needs its own
    `duration` to outlive its intervals."""
    from datetime import datetime, timedelta, timezone

    start = (datetime.now(timezone.utc) - timedelta(seconds=secs)).strftime(
        "%Y-%m-%dT%H:%M:%SZ"
    )
    r = vtn_post(
        "/events",
        context.vtn_token,
        json={
            "programID": context.saved_program_id,
            # Unique per event, because the name is the VTN's uniqueness
            # constraint and nothing here reads it back. A fixed name means
            # this step 409s against any test VTN that still holds an event
            # from an earlier run -- which the before_all cleanup usually but
            # not always prevents, and which makes re-running a single feature
            # against a live stack fail for a reason that has nothing to do
            # with what is being tested.
            "eventName": f"resample-event-{uuid.uuid4().hex[:8]}",
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
                    "payloadType": "USAGE",
                    "readingType": "DIRECT_READ",
                    "frequency": 1,
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


@then("VEN-1 submits no report for the event within {seconds:d} seconds")
def step_no_report_for_event(context, seconds):
    """D-5: an event that asked for nothing gets nothing.

    Waits the full window rather than checking once — "no report yet" and "no
    report ever" look identical at t=0, and only the second is the contract.
    """
    event_id = context.saved_event_id
    deadline = time.time() + seconds
    while time.time() < deadline:
        reports = requests.get(f"{VEN_BASE_URL}/reports", timeout=HTTP_TIMEOUT).json()
        matching = [r for r in reports if r.get("eventID") == event_id]
        assert not matching, (
            f"VEN-1 reported for event {event_id}, which carries no reportDescriptors — "
            "the timer-driven path was removed in D-5/F-9 and nothing else should report "
            f"unprompted: {matching[:1]}"
        )
        time.sleep(5)


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
