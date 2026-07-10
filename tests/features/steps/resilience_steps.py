"""Step definitions for failure-recovery / resilience scenarios."""

import json
import os
from datetime import datetime, timezone
from behave import given, when, then
from features.helpers.api_client import ven_get, ven2_get, get_token_value
from features.helpers.wait import poll_until
from features.helpers import docker_ctl

VEN_BASE_URL = os.environ.get("VEN_BASE_URL", "http://test-ven-1:8080")
VEN2_BASE_URL = os.environ.get("VEN2_BASE_URL", "http://test-ven-2:8080")
VTN_BASE_URL = os.environ.get("VTN_BASE_URL", "http://test-vtn:3000")

# Map service names to their health URLs
HEALTH_URLS = {
    "test-vtn": f"{VTN_BASE_URL}/health",
    "test-ven-1": f"{VEN_BASE_URL}/health",
    "test-ven-2": f"{VEN2_BASE_URL}/health",
}

# NOTE: "I wait for VEN-1/VEN-2 to show event" steps are defined in
# use_case_steps.py (with both @given and @when decorators).


@when('the "{service}" service is stopped')
def step_stop_service(context, service):
    docker_ctl.stop_service(service)
    # Track stopped services for cleanup in after_scenario
    try:
        stopped = context._stopped_services
    except (AttributeError, KeyError):
        stopped = []
        context._stopped_services = stopped
    stopped.append(service)


@when('the "{service}" service is restarted')
def step_restart_service(context, service):
    docker_ctl.restart_service(service)


@when('I wait for the "{service}" service to be healthy')
@given('I wait for the "{service}" service to be healthy')
def step_wait_healthy(context, service):
    url = HEALTH_URLS.get(service)
    if not url:
        raise ValueError(f"No health URL configured for service '{service}'")
    docker_ctl.wait_for_healthy(url, timeout=60)


@when('I refresh my VTN token as "{user}"')
@given('I refresh my VTN token as "{user}"')
def step_refresh_token(context, user):
    context.vtn_token = get_token_value(user, user)


@then('VEN-1 still serves cached event "{name}"')
def step_ven1_cached_event(context, name):
    events = ven_get("/events").json()
    names = [e.get("eventName") for e in events]
    assert name in names, f"Expected '{name}' in VEN-1 cached events, got: {names}"


@then('VEN-1 picks up event "{name}" within {seconds:d} seconds')
def step_ven1_picks_up(context, name, seconds):
    poll_until(
        lambda: ven_get("/events").json(),
        lambda events: any(e.get("eventName") == name for e in events),
        timeout=seconds,        description=f"VEN-1 picks up event '{name}'",
    )


@then('VEN-2 picks up event "{name}" within {seconds:d} seconds')
def step_ven2_picks_up(context, name, seconds):
    poll_until(
        lambda: ven2_get("/events").json(),
        lambda events: any(e.get("eventName") == name for e in events),
        timeout=seconds,        description=f"VEN-2 picks up event '{name}'",
    )


# ── WP2.1 (BL-03): exponential backoff with jitter ─────────────────────────────

@given("I mark the current time as the outage start")
def step_mark_outage_start(context):
    context.outage_start = datetime.now(timezone.utc)


def _parse_log_ts(raw):
    return datetime.fromisoformat(raw.replace("Z", "+00:00"))


@then("VEN-1's events-poll failure log shows growing intervals since the outage start")
def step_backoff_growing_intervals(context):
    since = context.outage_start.strftime("%Y-%m-%dT%H:%M:%SZ")
    lines = docker_ctl.get_logs("test-ven-1", since=since)

    failure_ts = []
    for line in lines:
        try:
            entry = json.loads(line)
        except (json.JSONDecodeError, TypeError):
            continue
        if entry.get("level") != "ERROR":
            continue
        fields = entry.get("fields", {})
        if fields.get("resource") != "events":
            continue
        if "poll failed" not in fields.get("message", ""):
            continue
        failure_ts.append(_parse_log_ts(entry["timestamp"]))

    assert len(failure_ts) >= 3, (
        f"expected >= 3 events-poll failures logged during the outage, "
        f"got {len(failure_ts)} (last 20 log lines: {lines[-20:]})"
    )

    gaps = [
        (failure_ts[i + 1] - failure_ts[i]).total_seconds()
        for i in range(len(failure_ts) - 1)
    ]
    # First gap is ~base interval (30s, ±10% jitter); a later gap has doubled
    # at least once (>=60s, ±10%) — 1.3x leaves clear margin over jitter alone.
    assert gaps[-1] > gaps[0] * 1.3, (
        f"expected backoff intervals to grow during the outage, got gaps={gaps}"
    )
