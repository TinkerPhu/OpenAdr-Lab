"""Step definitions for the real-measurement-mqtt BDD scenarios
(real_measurement_mqtt.feature). Requires the `mosquitto` test-broker service
in tests/docker-compose.test.yml and the `measurements` profile section in
VEN/profiles/test.yaml.
"""

import json
from datetime import datetime, timezone

from behave import given, then
from features.helpers.api_client import ven_get
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
    pass  # nothing to do — absence is the precondition


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
    def pv_power_kw():
        resp = ven_get("/sim")
        assert resp.ok
        return resp.json()["assets"]["pv"]["power_kw"]

    poll_until(
        lambda: ven_get("/sim"),
        lambda resp: resp.ok and abs(resp.json()["assets"]["pv"]["power_kw"] + PV_MEASURED_KW) < 0.05,
        timeout=15,
        interval=1,
        description="VEN-1's live PV power reflects the measured reading",
    )
