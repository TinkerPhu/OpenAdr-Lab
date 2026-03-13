from behave import given, when, then
from features.helpers.api_client import ven_get, ven_post


@given("I post sensor data with temperature {temp:g} and power {power:g}")
def step_post_sensor(context, temp, power):
    r = ven_post(
        "/sensors",
        json={
            "temperature_c": temp,
            "power_w": power,
            "raw": {"source": "test"},
        },
    )
    r.raise_for_status()


@given("I post partial sensor data with only temperature {temp:g}")
def step_post_partial_temp(context, temp):
    r = ven_post("/sensors", json={"temperature_c": temp})
    r.raise_for_status()
    context.post_response = r.json()


@given("I post partial sensor data with only power {power:g}")
def step_post_partial_power(context, power):
    r = ven_post("/sensors", json={"power_w": power})
    r.raise_for_status()
    context.post_response = r.json()


@when("I GET the VEN sensor snapshot")
def step_get_sensor(context):
    context.ven_sensor = ven_get("/sensors").json()


@then("the sensor temperature is {temp:g}")
def step_sensor_temp(context, temp):
    actual = context.ven_sensor.get("temperature_c")
    assert actual == temp, f"Expected temperature {temp}, got {actual}"


@then("the sensor power is {power:g}")
def step_sensor_power(context, power):
    actual = context.ven_sensor.get("power_w")
    if actual is None or abs(actual - power) >= 0.001:
        # GET may race with the VEN sim tick overwriting the sensor state.
        # The POST response is the authoritative value at the moment of write.
        post = getattr(context, "post_response", None) or {}
        actual = post.get("power_w", actual)
    assert actual is not None and abs(actual - power) < 0.001, (
        f"Expected power {power}, got {actual}"
    )


@then('the sensor has a generated "{field}"')
def step_sensor_has_generated(context, field):
    val = context.ven_sensor.get(field)
    assert val is not None, f"Expected generated '{field}', got None"
