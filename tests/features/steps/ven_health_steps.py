from behave import when, then
from features.helpers.api_client import ven_get


@when('I GET the VEN "{path}" endpoint')
def step_get_ven_endpoint(context, path):
    context.ven_response = ven_get(path)


@then('the VEN health response status is "{expected}"')
def step_ven_health_status_is(context, expected):
    body = context.ven_response.json()
    actual = body["status"]
    assert actual == expected, f"Expected status '{expected}', got '{actual}': {body}"


@then("the VEN health response has components {names}")
def step_ven_health_has_components(context, names):
    body = context.ven_response.json()
    components = body["components"]
    for name in [n.strip() for n in names.split(",")]:
        assert name in components, f"Missing component '{name}' in {components}"
        assert components[name]["status"] in ("ok", "degraded"), (
            f"Component '{name}' has unexpected status shape: {components[name]}"
        )
