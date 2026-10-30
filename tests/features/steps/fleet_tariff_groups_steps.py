"""Steps for the per-VEN-group tariff display (fleet_tariff_groups.feature, GB-64)."""

from datetime import datetime, timedelta, timezone

from behave import given, then, when
from features.helpers.api_client import bff_get, vtn_post
from features.helpers.wait import poll_until


def _first_reporting_ven(context):
    return sorted(v["venName"] for v in context.fleet["vens"])[0]


def _post_price_event(context, name, priority, targets, price):
    """A one-hour PRICE event that started five minutes ago, so it overlaps the 10-minute window."""
    start = datetime.now(timezone.utc) - timedelta(minutes=5)
    body = {
        "programID": context.saved_program_id,
        "eventName": name,
        "targets": targets,
        "payloadDescriptors": [{"payloadType": "PRICE", "units": "KWH", "currency": "EUR"}],
        "intervals": [
            {
                "id": 0,
                "intervalPeriod": {"start": start.strftime("%Y-%m-%dT%H:%M:%SZ"), "duration": "PT1H"},
                "payloads": [{"type": "PRICE", "values": [price]}],
            }
        ],
    }
    if priority is not None:
        body["priority"] = priority
    r = vtn_post("/events", context.vtn_token, json=body)
    r.raise_for_status()


@given('a PRICE event "{name}" of {price:g} with no priority for every VEN')
def step_open_price_event(context, name, price):
    _post_price_event(context, name, None, None, price)


@given('a priority-1 PRICE event "{name}" of {price:g} for the first reporting VEN')
def step_priority_price_event(context, name, price):
    _post_price_event(context, name, 1, [_first_reporting_ven(context)], price)


def _event_names(ven):
    return {b.get("eventName") for b in ven["bands"] if b["payloadType"] == "PRICE"}


@when("I wait for the fleet signals of the last {minutes:d} minutes to show both PRICE events")
def step_wait_for_both_price_events(context, minutes):
    target = _first_reporting_ven(context)

    def fetch():
        start = datetime.now(timezone.utc) - timedelta(minutes=minutes)
        return bff_get("/api/fleet/signals", params={"from": start.isoformat()}).json()

    def both_seen(body):
        vens = {v["venName"]: v for v in body.get("vens", [])}
        others = [v for name, v in vens.items() if name != target]
        return "tg-override" in _event_names(vens.get(target, {"bands": []})) and any(
            "tg-standard" in _event_names(v) for v in others
        )

    context.fleet_signals = poll_until(
        fetch, both_seen, timeout=120, interval=5, description="both tg- PRICE events in the feed"
    )


@then('the first reporting VEN\'s price bands come only from "{name}"')
def step_target_bands_only_from(context, name):
    ven = next(v for v in context.fleet_signals["vens"] if v["venName"] == _first_reporting_ven(context))
    names = {b.get("eventName") for b in ven["bands"] if b["payloadType"] == "PRICE"}
    tg = {n for n in names if n and n.startswith("tg-")}
    assert tg == {name}, (
        f"{ven['venName']} was sent {sorted(tg)}; the feed must list only the event that wins "
        f"where events overlap ({name}), or the chart draws a price the VEN did not act on"
    )


@then('another VEN\'s price bands include "{keep}" and not "{gone}"')
def step_other_bands_include(context, keep, gone):
    target = _first_reporting_ven(context)
    others = [v for v in context.fleet_signals["vens"] if v["venName"] != target]
    seen = [n for v in others for n in _event_names(v) if n]
    assert keep in seen, f"no other VEN carries {keep}: {sorted(set(seen))}"
    assert gone not in seen, f"{gone} is targeted at {target} only but reached another VEN"


@then('the Fleet tariff card lists a tariff of "{name}" for the first reporting VEN')
def step_card_lists_tariff(context, name):
    page = context.ui.page
    selector = f'[data-testid="fleet-tariff-group"][title*="{name}"]'
    page.wait_for_selector(selector, timeout=90_000)
    title = page.locator(selector).first.get_attribute("title")
    ven = _first_reporting_ven(context)
    assert ven in title, f"the tariff of {name} should list {ven} on hover, got {title!r}"


@then("the Fleet tariff card says it shows the VTN's events as of now")
def step_card_states_basis(context):
    text = context.ui.page.locator('[data-testid="fleet-tariff-source"]').first.inner_text()
    assert "VTN events as of now" in text, text
