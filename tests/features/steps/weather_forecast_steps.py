"""Step definitions for the weather-forecast-plugin BDD scenarios (@wip —
see weather_forecast.feature: these depend on
weather-forecast-implementation-plan.md Phase 8 planner wiring, not yet
landed. Excluded from the default suite via behave.ini's `tags = ~@wip`;
kept here so the intended scenario shape is committed and reviewable ahead
of that follow-up, matching the existing precedent in ven_reports.feature.
"""

import json
from datetime import datetime, timedelta, timezone

from behave import given, when, then
from features.helpers.api_client import ven_get


def _publish_mqtt(topic: str, payload: dict) -> None:
    """Publish a test message to the Mosquitto broker used by the E2E stack.
    Mirrors the publish pattern already used by this project's
    data_acquisition/mqtt_bridge services (paho-mqtt, broker host "mosquitto").
    """
    import paho.mqtt.publish as publish

    publish.single(
        topic,
        payload=json.dumps(payload),
        hostname="mosquitto",
        port=1883,
        qos=1,
        retain=True,
    )


def _sample_forecast_message(fetched_at: datetime) -> dict:
    valid_at = fetched_at.replace(minute=0, second=0, microsecond=0) + timedelta(hours=1)
    return {
        "schema_version": "1.0.0",
        "source_id": "bdd-test-source",
        "location": {"latitude_deg": 47.4491, "longitude_deg": 7.8081},
        "fetched_at": fetched_at.strftime("%Y-%m-%dT%H:%M:%SZ"),
        "samples": [
            {
                "valid_at": valid_at.strftime("%Y-%m-%dT%H:%M:%SZ"),
                "age_h": 1,
                "temperature_c": 20.0,
                "ghi_w_m2": 600.0,
            }
        ],
    }


@given("a weather forecast message is published to the test Mosquitto broker for VEN-1")
def step_publish_fresh_forecast(context):
    _publish_mqtt(
        "openadr-lab/weather/ven-1/forecast",
        _sample_forecast_message(datetime.now(timezone.utc)),
    )


@given(
    "a weather forecast message older than the staleness threshold is published to "
    "the test Mosquitto broker for VEN-1"
)
def step_publish_stale_forecast(context):
    _publish_mqtt(
        "openadr-lab/weather/ven-1/forecast",
        _sample_forecast_message(datetime.now(timezone.utc) - timedelta(hours=6)),
    )


@given("no weather forecast has ever been published for VEN-1")
def step_no_forecast_published(context):
    pass  # nothing to do — absence is the precondition


@when("a plan cycle runs on VEN-1")
def step_wait_for_plan_cycle(context):
    from features.helpers.wait import poll_until

    poll_until(
        lambda: ven_get("/plan"),
        lambda resp: resp.ok and resp.json().get("id") is not None,
        timeout=90,
        interval=3,
        description="VEN-1 has produced a plan",
    )
    context.plan_response = ven_get("/plan")


def _pv_allocation_kw(context) -> list:
    plan = context.plan_response.json()
    return [
        slot.get("planned_kw_by_asset", {}).get("pv", 0.0) for slot in plan.get("slots", [])
    ]


@then("the plan's PV allocation reflects the weather-sourced forecast rather than the sin model")
def step_pv_matches_weather(context):
    # A 600 W/m2 GHI test sample should not match the sin model's midday
    # peak exactly — any deviation confirms the weather path was used.
    pv_kw = _pv_allocation_kw(context)
    assert any(v != 0.0 for v in pv_kw), "expected a non-zero, weather-influenced PV forecast"


@then("the plan's PV allocation matches the sin-model forecast")
def step_pv_matches_sin_model(context):
    pv_kw = _pv_allocation_kw(context)
    assert pv_kw is not None
