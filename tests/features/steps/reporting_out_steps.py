"""Step definitions for outbound flexibility/forecast reports (WP3.6 — BL-10, §8.8)."""

import requests
from behave import given, then
from features.helpers.api_client import vtn_post, VEN_BASE_URL, HTTP_TIMEOUT


@given('I create an event for the saved program with a reportDescriptor of type "{ptype}" and frequency {freq_s:d} seconds')
def step_create_event_with_typed_descriptor(context, ptype, freq_s):
    r = vtn_post(
        "/events",
        context.vtn_token,
        json={
            "programID": context.saved_program_id,
            "eventName": f"descriptor-{ptype.lower().replace('_', '-')}",
            "intervals": [
                {"id": 0, "payloads": [{"type": "PRICE", "values": [0.25]}]},
            ],
            "reportDescriptors": [
                {
                    "payloadType": ptype,
                    "readingType": "DIRECT_READ",
                    "frequency": freq_s,
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


@then("every interval of the latest report has an intervalPeriod start")
def step_every_interval_has_period_start(context):
    intervals = _latest_report_intervals(context)
    for iv in intervals:
        period = iv.get("intervalPeriod") or {}
        assert period.get("start"), (
            f"Interval {iv.get('id')} lacks intervalPeriod.start: {iv}"
        )
