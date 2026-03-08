"""Step definitions for VEN Rate System (Stage 2) BDD tests."""

import uuid
from behave import given, when, then
from features.helpers.api_client import get_token_value, vtn_post, ven_get
from features.helpers.wait import poll_until


# ---------------------------------------------------------------------------
# Given: program + event setup on VTN
# ---------------------------------------------------------------------------

@given("I create a rate-system program and save its ID")
def step_create_rate_program(context):
    # Use a unique name per scenario run to avoid 409 conflicts
    unique_name = f"rate-test-{uuid.uuid4().hex[:8]}"
    r = vtn_post(
        "/programs",
        context.vtn_token,
        json={
            "programName": unique_name,
            "intervalPeriod": None,
            "programDescriptions": None,
        },
    )
    r.raise_for_status()
    context.saved_program_id = r.json()["id"]


@given("I create a 3-interval PRICE event for the saved program")
def step_create_3interval_price_event(context):
    r = vtn_post(
        "/events",
        context.vtn_token,
        json={
            "programID": context.saved_program_id,
            "eventName": "rate-price-3interval",
            "priority": 1,
            "intervals": [
                {
                    "id": 0,
                    "intervalPeriod": {
                        "start": "2025-01-01T14:00:00Z",
                        "duration": "PT1H",
                    },
                    "payloads": [{"type": "PRICE", "values": [0.25]}],
                },
                {
                    "id": 1,
                    "intervalPeriod": {
                        "start": "2025-01-01T15:00:00Z",
                        "duration": "PT1H",
                    },
                    "payloads": [{"type": "PRICE", "values": [0.30]}],
                },
                {
                    "id": 2,
                    "intervalPeriod": {
                        "start": "2025-01-01T16:00:00Z",
                        "duration": "PT1H",
                    },
                    "payloads": [{"type": "PRICE", "values": [0.35]}],
                },
            ],
        },
    )
    r.raise_for_status()
    context.rate_event_id = r.json().get("id")


@given("I create a GHG event for the saved program")
def step_create_ghg_event(context):
    r = vtn_post(
        "/events",
        context.vtn_token,
        json={
            "programID": context.saved_program_id,
            "eventName": "rate-ghg-event",
            "priority": 1,
            "intervals": [
                {
                    "id": 0,
                    "intervalPeriod": {
                        "start": "2025-01-01T10:00:00Z",
                        "duration": "PT1H",
                    },
                    "payloads": [{"type": "GHG", "values": [210.5]}],
                }
            ],
        },
    )
    r.raise_for_status()
    context.rate_event_id = r.json().get("id")


@given("I create an EXPORT_PRICE event for the saved program")
def step_create_export_price_event(context):
    r = vtn_post(
        "/events",
        context.vtn_token,
        json={
            "programID": context.saved_program_id,
            "eventName": "rate-export-price-event",
            "priority": 1,
            "intervals": [
                {
                    "id": 0,
                    "intervalPeriod": {
                        "start": "2025-01-01T11:00:00Z",
                        "duration": "PT1H",
                    },
                    "payloads": [{"type": "EXPORT_PRICE", "values": [0.10]}],
                }
            ],
        },
    )
    r.raise_for_status()
    context.rate_event_id = r.json().get("id")


@given("I create an IMPORT_CAPACITY_LIMIT event with limit {limit:f} kW for the saved program")
def step_create_capacity_limit_event(context, limit):
    r = vtn_post(
        "/events",
        context.vtn_token,
        json={
            "programID": context.saved_program_id,
            "eventName": "rate-cap-limit-event",
            "priority": 1,
            "intervals": [
                {
                    "id": 0,
                    "intervalPeriod": {
                        "start": "2025-01-01T12:00:00Z",
                        "duration": "PT1H",
                    },
                    "payloads": [{"type": "IMPORT_CAPACITY_LIMIT", "values": [limit]}],
                }
            ],
        },
    )
    r.raise_for_status()
    context.capacity_limit = limit
    context.rate_event_id = r.json().get("id")


@given("I create a PRICE event with no reportDescriptors for the saved program")
def step_create_price_event_no_descriptors(context):
    r = vtn_post(
        "/events",
        context.vtn_token,
        json={
            "programID": context.saved_program_id,
            "eventName": "rate-price-no-descriptors",
            "priority": 1,
            "intervals": [
                {
                    "id": 0,
                    "intervalPeriod": {
                        "start": "2025-01-01T09:00:00Z",
                        "duration": "PT1H",
                    },
                    "payloads": [{"type": "PRICE", "values": [0.20]}],
                }
            ],
            # No reportDescriptors field
        },
    )
    r.raise_for_status()
    context.rate_event_id = r.json().get("id")


# ---------------------------------------------------------------------------
# When: poll VEN endpoints
# ---------------------------------------------------------------------------

@when("I wait for the VEN /rates endpoint to have at least {count:d} snapshot")
@when("I wait for the VEN /rates endpoint to have at least {count:d} snapshots")
def step_wait_ven_rates(context, count):
    def fetch():
        resp = ven_get("/rates")
        if not resp.ok:
            return []
        return resp.json()

    context.ven_rates = poll_until(
        fetch,
        lambda rates: isinstance(rates, list) and len(rates) >= count,
        timeout=30,
        interval=3,
        description=f"VEN /rates has >= {count} snapshot(s)",
    )


@when("I wait for the VEN /rates endpoint to have a snapshot with co2_g_kwh")
def step_wait_ven_rates_co2(context):
    def fetch():
        resp = ven_get("/rates")
        if not resp.ok:
            return []
        return resp.json()

    context.ven_rates = poll_until(
        fetch,
        lambda rates: isinstance(rates, list) and any(s.get("co2_g_kwh") is not None for s in rates),
        timeout=30,
        interval=3,
        description="VEN /rates has a snapshot with co2_g_kwh",
    )


@when("I wait for the VEN /rates endpoint to have a snapshot with export_price_eur_kwh")
def step_wait_ven_rates_export_price(context):
    def fetch():
        resp = ven_get("/rates")
        if not resp.ok:
            return []
        return resp.json()

    context.ven_rates = poll_until(
        fetch,
        lambda rates: isinstance(rates, list) and any(s.get("export_price_eur_kwh") is not None for s in rates),
        timeout=30,
        interval=3,
        description="VEN /rates has a snapshot with export_price_eur_kwh",
    )


@when("I wait for the VEN /capacity import_limit_kw to be {expected:f}")
def step_wait_ven_capacity_limit(context, expected):
    def fetch():
        resp = ven_get("/capacity")
        if not resp.ok:
            return {}
        return resp.json()

    context.ven_capacity = poll_until(
        fetch,
        lambda cap: isinstance(cap, dict) and cap.get("import_limit_kw") == expected,
        timeout=30,
        interval=3,
        description=f"VEN /capacity import_limit_kw == {expected}",
    )


@when("I request GET /capacity from the VEN")
def step_get_capacity(context):
    resp = ven_get("/capacity")
    resp.raise_for_status()
    context.ven_capacity = resp.json()


# ---------------------------------------------------------------------------
# Then: assertions
# ---------------------------------------------------------------------------

@then("all rate snapshots have an import_price_eur_kwh value")
def step_all_snapshots_have_import_price(context):
    rates = context.ven_rates
    assert rates, "No rate snapshots returned"
    for snap in rates:
        assert snap.get("import_price_eur_kwh") is not None, (
            f"Rate snapshot missing import_price_eur_kwh: {snap}"
        )


@then("at least one rate snapshot has a co2_g_kwh value")
def step_at_least_one_snapshot_has_co2(context):
    rates = context.ven_rates
    assert any(s.get("co2_g_kwh") is not None for s in rates), (
        f"No rate snapshot has co2_g_kwh. Snapshots: {rates}"
    )


@then("at least one rate snapshot has an export_price_eur_kwh value")
def step_at_least_one_snapshot_has_export_price(context):
    rates = context.ven_rates
    assert any(s.get("export_price_eur_kwh") is not None for s in rates), (
        f"No rate snapshot has export_price_eur_kwh. Snapshots: {rates}"
    )


@then("the VEN /capacity response has import_limit_kw equal to {expected:f}")
def step_capacity_import_limit_equals(context, expected):
    cap = context.ven_capacity
    actual = cap.get("import_limit_kw")
    assert actual == expected, (
        f"Expected import_limit_kw={expected}, got {actual}. Full response: {cap}"
    )


@then("the VEN /obligations response is a JSON array")
def step_obligations_is_array(context):
    resp = ven_get("/obligations")
    resp.raise_for_status()
    body = resp.json()
    assert isinstance(body, list), f"Expected a JSON array, got: {type(body)} — {body}"


@then("the response is a JSON object")
def step_response_is_json_object(context):
    cap = context.ven_capacity
    assert isinstance(cap, dict), f"Expected a JSON object, got: {type(cap)} — {cap}"


@then('the response contains the field "{field}"')
def step_response_contains_field(context, field):
    cap = context.ven_capacity
    assert field in cap, f"Field '{field}' not in capacity response: {list(cap.keys())}"
