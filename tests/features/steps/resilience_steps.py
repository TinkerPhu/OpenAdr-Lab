"""Step definitions for failure-recovery / resilience scenarios."""

import os
from behave import given, when, then
from features.helpers.api_client import ven_get, ven2_get
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


@given('I wait for VEN-1 to show event "{name}"')
@when('I wait for VEN-1 to show event "{name}"')
def step_wait_ven1_event(context, name):
    poll_until(
        lambda: ven_get("/events").json(),
        lambda events: any(e.get("eventName") == name for e in events),
        timeout=30, interval=3,
        description=f"VEN-1 shows event '{name}'",
    )


@given('I wait for VEN-2 to show event "{name}"')
@when('I wait for VEN-2 to show event "{name}"')
def step_wait_ven2_event(context, name):
    poll_until(
        lambda: ven2_get("/events").json(),
        lambda events: any(e.get("eventName") == name for e in events),
        timeout=30, interval=3,
        description=f"VEN-2 shows event '{name}'",
    )


@when('the "{service}" service is stopped')
def step_stop_service(context, service):
    docker_ctl.stop_service(service)
    # Track stopped services for cleanup in after_scenario
    if not hasattr(context, "_stopped_services"):
        context._stopped_services = []
    context._stopped_services.append(service)


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
        timeout=seconds, interval=3,
        description=f"VEN-1 picks up event '{name}'",
    )


@then('VEN-2 picks up event "{name}" within {seconds:d} seconds')
def step_ven2_picks_up(context, name, seconds):
    poll_until(
        lambda: ven2_get("/events").json(),
        lambda events: any(e.get("eventName") == name for e in events),
        timeout=seconds, interval=3,
        description=f"VEN-2 picks up event '{name}'",
    )
