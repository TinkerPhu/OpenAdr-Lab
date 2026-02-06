from behave import when, then
from features.helpers.api_client import ven_get


@when('I GET the VEN "{path}" endpoint')
def step_get_ven_endpoint(context, path):
    context.ven_response = ven_get(path)


@then('the VEN response body is "{expected}"')
def step_ven_body_is(context, expected):
    actual = context.ven_response.text
    assert actual == expected, f"Expected '{expected}', got '{actual}'"
