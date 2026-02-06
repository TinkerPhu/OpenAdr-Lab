import uuid
from behave import given, when, then
from features.helpers.api_client import ven_get, ven_post


@given("I post sensor data with temperature {temp:g} and power {power:g}")
def step_post_sensor(context, temp, power):
    r = ven_post(
        "/sensors",
        json={
            "id": str(uuid.uuid4()),
            "ts": "2025-01-01T00:00:00Z",
            "temperature_c": temp,
            "power_w": power,
            "voltage_v": None,
            "raw": {"source": "test"},
        },
    )
    r.raise_for_status()


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
    assert actual == power, f"Expected power {power}, got {actual}"
