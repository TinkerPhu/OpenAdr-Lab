import requests
from behave import given, when, then
from features.helpers.api_client import (
    get_token_value,
    vtn_get,
    vtn_post,
    VTN_BASE_URL,
)


@given('I have a VTN token as "{user}"')
def step_get_vtn_token(context, user):
    # client_id and client_secret are the same as the user name for fixtures
    context.vtn_token = get_token_value(user, user)


@when('I create a program named "{name}"')
@given('I create a program named "{name}"')
def step_create_program(context, name):
    context.response = vtn_post(
        "/programs",
        context.vtn_token,
        json={"programName": name, "intervalPeriod": None, "programDescriptions": None},
    )


@when("I list programs")
def step_list_programs(context):
    context.response = vtn_get("/programs", context.vtn_token)
    context.response.raise_for_status()
    context.program_list = context.response.json()


@then('the program list contains "{name}"')
def step_program_list_contains(context, name):
    names = [p.get("programName") for p in context.program_list]
    assert name in names, f"'{name}' not in program names: {names}"


@then('the response contains "{field}" equal to "{value}"')
def step_response_field_equals(context, field, value):
    body = context.response.json()
    assert body.get(field) == value, (
        f"Expected {field}={value!r}, got {body.get(field)!r}"
    )


@when('I GET "{path}" without authentication')
def step_get_without_auth(context, path):
    context.response = requests.get(f"{VTN_BASE_URL}{path}", timeout=10)
