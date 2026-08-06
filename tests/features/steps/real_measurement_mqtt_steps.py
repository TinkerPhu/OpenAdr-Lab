"""Step definitions for the real-measurement-mqtt BDD scenarios
(real_measurement_mqtt.feature). Requires the `mosquitto` test-broker service
in tests/docker-compose.test.yml and the `measurements` profile section in
VEN/profiles/test.yaml.
"""

import json
import time
from datetime import datetime, timezone

from behave import given, then
from features.helpers.api_client import ven_get, ven_post
from features.helpers.wait import poll_until


def _publish_mqtt(topic: str, payload: dict) -> None:
    import paho.mqtt.publish as publish

    publish.single(
        topic,
        payload=json.dumps(payload),
        hostname="mosquitto",
        port=1883,
        qos=1,
        retain=True,
    )


def _sample_measurement_message(power_kw: float) -> dict:
    return {
        "power_kw": power_kw,
        "ts": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
    }


PV_MEASURED_KW = 2.5
BASE_LOAD_MEASURED_KW = 0.75


@given("a PV measurement message is published to the test Mosquitto broker for VEN-1")
def step_publish_pv_measurement(context):
    _publish_mqtt(
        "openadr-lab/measurement/ven-1/pv",
        _sample_measurement_message(PV_MEASURED_KW),
    )


@given("a baseline-load measurement message is published to the test Mosquitto broker for VEN-1")
def step_publish_base_load_measurement(context):
    _publish_mqtt(
        "openadr-lab/measurement/ven-1/base_load",
        _sample_measurement_message(BASE_LOAD_MEASURED_KW),
    )


@given("no PV measurement has ever been published for VEN-1")
def step_no_pv_measurement_published(context):
    # Nothing to do — absence is the precondition. This relies on test-ven-1 and the
    # mosquitto broker being freshly created for this run (run_all_tests.sh tears the
    # stack down with `docker compose down -v` both before and after the E2E/resilience
    # suites specifically so a leftover retained MQTT reading from an interrupted prior
    # run can't get replayed to test-ven-1 on startup and make this precondition false).
    pass


@given("the VEN-1 pv irradiance offset is flushed to zero")
def step_flush_pv_irradiance_offset(context):
    # pv_irradiance_alpha=1.0 -> full decay in a single tick (see
    # PvSmoothingState::update); a plain /sim/inject/reset alone only stops
    # re-forcing the override, it does not accelerate the existing offset's
    # decay, which at the default alpha=0.1 can linger for many minutes.
    r = ven_post("/sim/inject", json={"pv_irradiance_alpha": 1.0})
    r.raise_for_status()
    time.sleep(3)  # >= a few ticks at the test profile's tick_s=1
    r = ven_post("/sim/inject/reset", json={})
    r.raise_for_status()


@then("/measurement reports the PV signal as ok with the published reading")
def step_measurement_pv_ok(context):
    poll_until(
        lambda: ven_get("/measurement"),
        lambda resp: resp.ok and resp.json()["pv"]["status"] == "ok",
        timeout=15,
        interval=1,
        description="VEN-1's /measurement reports pv.status == ok",
    )
    resp = ven_get("/measurement")
    assert resp.json()["pv"]["raw_kw"] == PV_MEASURED_KW


@then("/measurement reports the base_load signal as ok with the published reading")
def step_measurement_base_load_ok(context):
    poll_until(
        lambda: ven_get("/measurement"),
        lambda resp: resp.ok and resp.json()["base_load"]["status"] == "ok",
        timeout=15,
        interval=1,
        description="VEN-1's /measurement reports base_load.status == ok",
    )
    resp = ven_get("/measurement")
    assert resp.json()["base_load"]["raw_kw"] == BASE_LOAD_MEASURED_KW


@then("/measurement reports the PV signal as not_configured")
def step_measurement_pv_not_configured(context):
    resp = ven_get("/measurement")
    assert resp.ok
    assert resp.json()["pv"]["status"] == "not_configured"


@then("the live PV power on VEN-1 reflects the measured reading rather than the weather estimate")
def step_live_pv_matches_measured(context):
    poll_until(
        lambda: ven_get("/sim"),
        lambda resp: resp.ok and abs(resp.json()["assets"]["pv"]["power_kw"] + PV_MEASURED_KW) < 0.05,
        timeout=15,
        interval=1,
        description="VEN-1's live PV power reflects the measured reading",
    )
