"""Step definitions for VEN Rate System (Stage 2) BDD tests."""

import uuid
from datetime import datetime, timedelta, timezone
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
                    "id": i,
                    "intervalPeriod": {
                        "start": (datetime.now(timezone.utc) + timedelta(hours=1 + i)).strftime("%Y-%m-%dT%H:%M:%SZ"),
                        "duration": "PT1H",
                    },
                    "payloads": [{"type": "PRICE", "values": [0.25 + i * 0.05]}],
                }
                for i in range(3)
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
                        "start": (datetime.now(timezone.utc) + timedelta(hours=1)).strftime("%Y-%m-%dT%H:%M:%SZ"),
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
                        "start": (datetime.now(timezone.utc) + timedelta(hours=1)).strftime("%Y-%m-%dT%H:%M:%SZ"),
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
                        "start": (datetime.now(timezone.utc) + timedelta(hours=1)).strftime("%Y-%m-%dT%H:%M:%SZ"),
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
                        "start": (datetime.now(timezone.utc) + timedelta(hours=1)).strftime("%Y-%m-%dT%H:%M:%SZ"),
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


def _post_single_price_event(context, name, priority, start, duration, price):
    r = vtn_post(
        "/events",
        context.vtn_token,
        json={
            "programID": context.saved_program_id,
            "eventName": name,
            "priority": priority,
            "intervals": [{
                "id": 0,
                "intervalPeriod": {"start": start.strftime("%Y-%m-%dT%H:%M:%SZ"), "duration": duration},
                "payloads": [{"type": "PRICE", "values": [price]}],
            }],
        },
    )
    r.raise_for_status()


def _parse_ts(s):
    return datetime.fromisoformat(s.replace("Z", "+00:00"))


def _price_at(rates, at):
    for s in rates:
        if _parse_ts(s["interval_start"]) <= at < _parse_ts(s["interval_end"]):
            return s.get("import_tariff_eur_kwh")
    return None


@given("I create a priority-5 day-ahead PRICE event of 0.09 for one hour 30 hours from now")
def step_create_day_ahead_hour(context):
    # 30 h out: clear of every other rate scenario's +1..+4 h events in this feature.
    hour = (datetime.now(timezone.utc) + timedelta(hours=30)).replace(minute=0, second=0, microsecond=0)
    context.day_ahead_hour_start = hour
    _post_single_price_event(context, "gb45-day-ahead", 5, hour, "PT1H", 0.09)


@given("I create a priority-1 PRICE event of 0.45 for 10 minutes starting 20 minutes into that hour")
def step_create_intra_hour_dr(context):
    start = context.day_ahead_hour_start + timedelta(minutes=20)
    _post_single_price_event(context, "gb45-intra-hour-dr", 1, start, "PT10M", 0.45)


@when("I wait for the VEN /tariffs endpoint to show 0.45 inside that hour")
def step_wait_intra_hour_price(context):
    at = context.day_ahead_hour_start + timedelta(minutes=25)

    def fetch():
        resp = ven_get("/tariffs")
        return resp.json() if resp.ok else []

    context.ven_rates = poll_until(
        fetch,
        lambda rates: isinstance(rates, list) and _price_at(rates, at) == 0.45,
        timeout=60,
        description="VEN /tariffs resolves the intra-hour DR price",
    )


@then("the VEN /tariffs price that hour at 0.09 before, 0.45 during and 0.09 after the 10-minute window")
def step_assert_intra_hour_resolution(context):
    h = context.day_ahead_hour_start
    got = {m: _price_at(context.ven_rates, h + timedelta(minutes=m)) for m in (5, 19, 20, 25, 29, 30, 45)}
    assert got == {5: 0.09, 19: 0.09, 20: 0.45, 25: 0.45, 29: 0.45, 30: 0.09, 45: 0.09}, got


@then("no two VEN /tariffs snapshots overlap")
def step_assert_tariffs_non_overlapping(context):
    spans = sorted((_parse_ts(s["interval_start"]), _parse_ts(s["interval_end"])) for s in context.ven_rates)
    for (s1, e1), (s2, e2) in zip(spans, spans[1:]):
        assert e1 <= s2, f"overlap: [{s1}, {e1}) and [{s2}, {e2})"


# ---------------------------------------------------------------------------
# When: poll VEN endpoints
# ---------------------------------------------------------------------------

@when("I wait for the VEN /tariffs endpoint to have at least {count:d} snapshot")
@when("I wait for the VEN /tariffs endpoint to have at least {count:d} snapshots")
def step_wait_ven_rates(context, count):
    def fetch():
        resp = ven_get("/tariffs")
        if not resp.ok:
            return []
        return resp.json()

    context.ven_rates = poll_until(
        fetch,
        lambda rates: isinstance(rates, list) and len(rates) >= count,
        timeout=60,
        description=f"VEN /tariffs has >= {count} snapshot(s)",
    )


@when("I wait for the VEN /tariffs endpoint to have a snapshot with co2_g_kwh")
def step_wait_ven_rates_co2(context):
    def fetch():
        resp = ven_get("/tariffs")
        if not resp.ok:
            return []
        return resp.json()

    context.ven_rates = poll_until(
        fetch,
        lambda rates: isinstance(rates, list) and any(s.get("co2_g_kwh") is not None for s in rates),
        timeout=60,
        description="VEN /tariffs has a snapshot with co2_g_kwh",
    )


@when("I wait for the VEN /tariffs endpoint to have a snapshot with export_tariff_eur_kwh")
def step_wait_ven_rates_export_price(context):
    def fetch():
        resp = ven_get("/tariffs")
        if not resp.ok:
            return []
        return resp.json()

    context.ven_rates = poll_until(
        fetch,
        lambda rates: isinstance(rates, list) and any(s.get("export_tariff_eur_kwh") is not None for s in rates),
        timeout=60,
        description="VEN /tariffs has a snapshot with export_tariff_eur_kwh",
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
        timeout=120,
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

@then("all rate snapshots have an import_tariff_eur_kwh value")
def step_all_snapshots_have_import_price(context):
    rates = context.ven_rates
    assert rates, "No rate snapshots returned"
    for snap in rates:
        assert snap.get("import_tariff_eur_kwh") is not None, (
            f"Rate snapshot missing import_tariff_eur_kwh: {snap}"
        )


@then("at least one rate snapshot has a co2_g_kwh value")
def step_at_least_one_snapshot_has_co2(context):
    rates = context.ven_rates
    assert any(s.get("co2_g_kwh") is not None for s in rates), (
        f"No rate snapshot has co2_g_kwh. Snapshots: {rates}"
    )


@then("at least one rate snapshot has an export_tariff_eur_kwh value")
def step_at_least_one_snapshot_has_export_price(context):
    rates = context.ven_rates
    assert any(s.get("export_tariff_eur_kwh") is not None for s in rates), (
        f"No rate snapshot has export_tariff_eur_kwh. Snapshots: {rates}"
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
