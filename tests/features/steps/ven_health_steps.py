from behave import when, then
from features.helpers.api_client import ven_get
from features.helpers.wait import poll_until


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


@then('the VEN health response field "{field}" is "{expected}"')
def step_ven_health_field_is(context, field, expected):
    body = context.ven_response.json()
    assert field in body, f"Missing field '{field}' in {body}"
    actual = body[field]
    expected_value = {"true": True, "false": False}.get(expected, expected)
    assert actual == expected_value, (
        f"Expected health field '{field}' to be {expected_value!r}, got {actual!r}: {body}"
    )


@then('the VEN health response field "{field}" becomes "{expected}" within {timeout:d} seconds')
def step_ven_health_field_becomes(context, field, expected, timeout):
    """Poll-based variant of the "is" check above — needed right after a VTN
    restart, since the poll loop may still be mid-sleep in a previously
    computed backoff delay (same class of latency the pre-existing
    "VEN backs off exponentially" scenario documents) — a same-instant GET
    can observe the stale pre-recovery value."""
    expected_value = {"true": True, "false": False}.get(expected, expected)

    def fetch():
        context.ven_response = ven_get("/health")
        return context.ven_response.json()

    body = poll_until(
        fetch,
        lambda b: b.get(field) == expected_value,
        timeout=timeout,
        interval=2,
        description=f"health field '{field}' == {expected_value!r}",
    )
    assert body[field] == expected_value
