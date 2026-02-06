from behave import given, then
from features.helpers.api_client import get_token


@given('I request a token with client_id "{client_id}" and client_secret "{client_secret}"')
def step_request_token(context, client_id, client_secret):
    context.response = get_token(client_id, client_secret)


@then("the response status is {status:d}")
def step_check_status(context, status):
    assert context.response.status_code == status, (
        f"Expected {status}, got {context.response.status_code}: "
        f"{context.response.text[:200]}"
    )


@then("the response status is not {status:d}")
def step_check_status_not(context, status):
    assert context.response.status_code != status, (
        f"Expected status != {status}, but got {context.response.status_code}"
    )


@then('the response contains an "{field}"')
def step_response_contains_field(context, field):
    body = context.response.json()
    assert field in body, f"Field '{field}' not found in {list(body.keys())}"
    assert body[field], f"Field '{field}' is empty"
