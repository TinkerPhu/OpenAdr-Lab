from behave import when, then
from features.helpers.api_client import ven_get
from features.helpers.wait import poll_until


@when('I wait for the VEN to show program "{name}"')
def step_wait_ven_program(context, name):
    def fetch():
        return ven_get("/programs").json()

    context.ven_programs = poll_until(
        fetch,
        lambda progs: any(p.get("name") == name for p in progs),
        timeout=30,
        interval=3,
        description=f"VEN shows program '{name}'",
    )


@then('the VEN program list contains "{name}"')
def step_ven_program_list_contains(context, name):
    names = [p.get("name") for p in context.ven_programs]
    assert name in names, f"'{name}' not in VEN program names: {names}"


@when("I wait for the VEN to have at least {count:d} event")
def step_wait_ven_events(context, count):
    def fetch():
        return ven_get("/events").json()

    context.ven_events = poll_until(
        fetch,
        lambda events: len(events) >= count,
        timeout=30,
        interval=3,
        description=f"VEN has >= {count} events",
    )


@then("the VEN event list is not empty")
def step_ven_events_not_empty(context):
    assert len(context.ven_events) > 0, "VEN event list is empty"


@when("I wait for the VEN sensor to have power data")
def step_wait_ven_sensor_power(context):
    def fetch():
        return ven_get("/sensors").json()

    context.ven_sensor = poll_until(
        fetch,
        lambda s: s.get("power_w") is not None,
        timeout=30,
        interval=3,
        description="VEN sensor has power_w",
    )


@then('the VEN sensor snapshot has a "{field}" value')
def step_ven_sensor_has_field(context, field):
    assert context.ven_sensor.get(field) is not None, (
        f"Sensor field '{field}' is None in {context.ven_sensor}"
    )
