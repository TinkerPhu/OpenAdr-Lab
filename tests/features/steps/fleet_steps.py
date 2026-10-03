"""Steps for the fleet telemetry feed (fleet-monitor phase 0 §6/§7)."""

from behave import then, when
from features.helpers.api_client import bff_get
from features.helpers.wait import poll_until


@then("the BFF health shows the fleet feed connected")
def step_bff_health_fleet_connected(context):
    body = context.response.json()
    fleet = body.get("fleet")
    assert fleet is not None, f"/api/health does not report the fleet feed: {body}"
    assert fleet.get("enabled"), (
        "the test stack configures FLEET_MQTT_HOST, so the feed must be enabled; "
        f"got {fleet}"
    )
    assert fleet.get("connected"), (
        f"the fleet feed is enabled but not connected to the broker: {fleet}"
    )


@when("I wait for the fleet feed to have at least {count:d} VEN")
def step_wait_for_fleet_vens(context, count):
    """Telemetry arrives on the VEN's tick cadence, so the first message may
    be a few seconds out. Waiting beats asserting against an empty feed and
    calling it a pass."""

    def fetch():
        context.response = bff_get("/api/fleet/power")
        return context.response.json()

    context.fleet = poll_until(
        fetch,
        lambda b: len(b.get("vens") or []) >= count,
        timeout=90,
        interval=3,
        description=f"at least {count} VEN on the fleet feed",
    )


@then("every listed VEN either reports a power value or none at all")
def step_ven_power_is_value_or_absent(context):
    for ven in context.fleet["vens"]:
        value = ven.get("netPowerW")
        assert value is None or isinstance(value, (int, float)), (
            f"{ven.get('venName')} reports a non-numeric power {value!r} — a VEN "
            "that has not spoken must be null, never a stand-in number"
        )


@then("the fleet sum equals the sum of the reporting VENs")
def step_fleet_sum_is_consistent(context):
    reporting = [
        v["netPowerW"] for v in context.fleet["vens"] if v.get("netPowerW") is not None
    ]
    fleet = context.fleet["fleet"]
    assert fleet["contributingVens"] == len(reporting), (
        f"the sum claims {fleet['contributingVens']} contributors but "
        f"{len(reporting)} VENs reported a value"
    )
    expected = sum(reporting)
    assert abs(fleet["netPowerW"] - expected) < 1e-6, (
        f"fleet sum {fleet['netPowerW']} != {expected} from its own VEN list"
    )


@when("I wait for the fleet history of the last {minutes:d} minutes to have samples")
def step_wait_for_fleet_history(context, minutes):
    """The store writes in batches, so a sample published a moment ago may not
    be queryable yet. Waiting is the honest way to say "eventually"."""
    from datetime import datetime, timedelta, timezone

    def fetch():
        start = datetime.now(timezone.utc) - timedelta(minutes=minutes)
        context.response = bff_get(
            "/api/fleet/power",
            params={"from": start.isoformat(), "stepSeconds": 5},
        )
        if context.response.status_code != 200:
            # 503 while the store is still connecting is a normal early state,
            # not a failure -- keep polling, and let the timeout message carry
            # the status if it never clears.
            return {"status": context.response.status_code,
                    "body": context.response.text[:200]}
        return context.response.json()

    context.fleet_history = poll_until(
        fetch,
        lambda b: len(b.get("fleet") or []) > 0,
        timeout=120,
        interval=5,
        description="stored fleet telemetry",
    )


@then("the fleet history says which resolution it was drawn at")
def step_history_declares_resolution(context):
    body = context.fleet_history
    assert body.get("source") in ("raw", "rollup"), (
        f"the history must say which table answered it, got {body.get('source')!r}"
    )
    assert body.get("stepSeconds"), "the history must state its bucket width"


@then("every history bucket counts the VENs it was summed from")
def step_history_buckets_count_contributors(context):
    for bucket in context.fleet_history["fleet"]:
        n = bucket.get("contributingVens")
        assert isinstance(n, int) and n >= 1, (
            f"bucket {bucket.get('ts')} carries no contributor count ({n!r}) — "
            "a total whose membership is unknown cannot be compared with another"
        )


@when("I wait for the fleet to report a reaction to the saved event")
def step_wait_for_reaction(context):
    """The chain needs three things to line up: the VEN polls the event, it
    publishes the decision, and the BFF writes it. Each is seconds, so this
    waits rather than asserts on the first try."""
    event_id = context.rate_event_id

    def fetch():
        context.response = bff_get("/api/fleet/reactions", params={"eventID": event_id})
        if context.response.status_code != 200:
            return {"status": context.response.status_code,
                    "body": context.response.text[:200]}
        return context.response.json()

    context.reactions = poll_until(
        fetch,
        lambda b: (b.get("vensSeen") or 0) >= 1,
        timeout=120,
        interval=5,
        description=f"a VEN reaction to event {event_id}",
    )


@then("each reacting VEN names the event version it saw")
def step_reaction_names_the_version(context):
    for ven in context.reactions["vens"]:
        assert ven.get("seenAt"), f"{ven.get('venName')} reports no time it saw the event"
        assert ven.get("modificationDateTime"), (
            f"{ven.get('venName')} did not say which version of the event it acted on — "
            "an edited event keeps its id, so without this the trace is ambiguous"
        )


@when("I navigate to the Fleet page")
def step_navigate_fleet(context):
    context.ui.page.click('[data-testid="nav-fleet"]')
    context.ui.page.wait_for_selector('[data-testid="fleet-total-card"]', timeout=30000)


@when('I select the fleet "{label}" window')
def step_select_fleet_window(context, label):
    """Drive the page's own window selector (R-99).

    The Fleet page opens on `WINDOWS[3]` — 24 h at 900 s steps — and the
    chart renders `fleet-chart-empty` *instead of itself* until that query
    returns rows. A store holding only a few minutes of telemetry resamples
    to zero 900 s buckets, so on a young stack the chart cannot appear, and
    appears later purely because time passed. That is what made this
    scenario fail early in a run and pass minutes later with no code change.

    Narrowing to a window the store fills in seconds removes the dependency
    on elapsed time rather than waiting it out. It is also a real user
    action the page already supports, not a test-only hook.
    """
    page = context.ui.page
    # MUI Select: click the wrapping div (parent of the hidden input) to open
    # the dropdown, then pick the item by its label — same pattern as
    # `helpers/ui.py::create_event`'s program select.
    page.locator('[data-testid="fleet-window-select"]').locator("..").click()
    page.locator(f'li[role="option"]:has-text("{label}")').click()
    # The selector closing is not the query returning; the chart re-renders
    # from a fresh fetch, so let the caller's own wait cover that.
    page.wait_for_selector('li[role="option"]', state="detached", timeout=10000)


def _fleet_chart_diagnosis(page, minutes=1440, step_seconds=900):
    """Why the chart is not there, asked of the browser rather than guessed.

    R-99: this scenario failed three times with nothing but "selector not
    found", and each failure cost a full E2E run to dismiss. The two facts
    that separate every candidate explanation are both cheap to get, and
    neither was being collected:

    - whether `fleet-chart-empty` is rendered. `FleetPowerChart` renders it
      *instead of* the chart while `rows.length === 0`, so its presence means
      "no data" and its absence means the component never rendered at all.
    - what the page's own `/api/fleet/power` fetch returns, asked with the
      page's own parameters. `Fleet.tsx` opens on `WINDOWS[3]`
      (`useState(3)`) = **24 h at 900 s steps** -- not the 15 min at 5 s that
      R-99's text records. The probe step waits on 10 min at 5 s, so it and
      the chart ask questions 144x apart in window and 180x apart in step,
      which is why a passing probe never implied a rendered chart. The probe
      also talks to the BFF directly while the browser goes through nginx;
      running the fetch *in the page* settles both differences at once.
    """
    empty = page.locator('[data-testid="fleet-chart-empty"]').count() > 0
    card = page.locator('[data-testid="fleet-total-card"]').count() > 0
    try:
        seen = page.evaluate(
            """async ([minutes, stepSeconds]) => {
                const from = new Date(Date.now() - minutes * 60000).toISOString();
                const r = await fetch(
                    `/api/fleet/power?from=${encodeURIComponent(from)}&stepSeconds=${stepSeconds}`
                );
                if (!r.ok) return { status: r.status };
                const b = await r.json();
                return {
                    status: r.status,
                    fleet_points: (b.fleet || []).length,
                    vens: (b.vens || []).length,
                    ven_samples: (b.vens || []).reduce(
                        (n, v) => n + (v.samples || []).length, 0),
                };
            }""",
            [minutes, step_seconds],
        )
    except Exception as e:  # the page itself may be broken; say so rather than mask it
        seen = {"evaluate_failed": str(e)[:200]}
    return (
        f"fleet-chart-empty rendered: {empty}; fleet-total-card present: {card}; "
        f"the page's own /api/fleet/power ({minutes} min, step {step_seconds}s) "
        f"returned: {seen}"
    )


@then("the fleet chart has a line for every reporting VEN")
def step_fleet_chart_has_a_line_per_ven(context):
    page = context.ui.page
    # The chart draws from the stored history, which the store writes a little
    # after the live feed shows a VEN -- so wait for the chart rather than
    # asserting against whatever happens to be rendered first.
    try:
        page.wait_for_selector('[data-testid="fleet-power-chart"]', timeout=60000)
    except Exception as e:
        raise AssertionError(
            f"the fleet chart never rendered. {_fleet_chart_diagnosis(page)}"
        ) from e

    reporting = [
        v["venName"]
        for v in bff_get("/api/fleet/power").json()["vens"]
        if v.get("netPowerW") is not None
    ]
    assert reporting, "no VEN is reporting, so this scenario would assert nothing"

    legend = page.locator('[data-testid^="legend-entry-"]')
    labels = {legend.nth(i).inner_text().strip() for i in range(legend.count())}
    context.fleet_legend_labels = labels
    missing = [name for name in reporting if name not in labels]
    assert not missing, f"reporting VENs missing from the chart legend: {missing} (legend: {labels})"
    assert "fleet total" in labels, f"the fleet total is not drawn: {labels}"


@then("hiding a VEN in the legend removes its line")
def step_hiding_a_ven_removes_its_line(context):
    page = context.ui.page
    name = sorted(n for n in context.fleet_legend_labels if n != "fleet total")[0]
    before = page.locator('[data-testid="fleet-power-chart"] .recharts-line').count()
    page.click(f'[data-testid="legend-toggle-{name}"]')
    page.wait_for_timeout(500)
    after = page.locator('[data-testid="fleet-power-chart"] .recharts-line').count()
    assert after == before - 1, (
        f"toggling {name} off left {after} lines, expected {before - 1} — the legend "
        "is decorative if it does not change what is drawn"
    )


@when("I wait for the fleet signals of the last {minutes:d} minutes to include the saved event")
def step_wait_for_signal_band(context, minutes):
    """The VEN has to have published at least once for the BFF to know it
    exists, so the band appears a moment after the event does."""
    from datetime import datetime, timedelta, timezone

    event_id = context.rate_event_id

    def fetch():
        start = datetime.now(timezone.utc) - timedelta(minutes=minutes)
        context.response = bff_get("/api/fleet/signals", params={"from": start.isoformat()})
        if context.response.status_code != 200:
            return {"status": context.response.status_code,
                    "body": context.response.text[:200]}
        return context.response.json()

    def has_band(body):
        return any(
            b.get("eventID") == event_id
            for ven in body.get("vens", [])
            for b in ven.get("bands", [])
        )

    context.fleet_signals = poll_until(
        fetch, has_band, timeout=120, interval=5,
        description=f"a signal band for event {event_id}",
    )


@then("the signal band names its payload type and value")
def step_band_names_its_payload(context):
    event_id = context.rate_event_id
    bands = [
        b
        for ven in context.fleet_signals["vens"]
        for b in ven["bands"]
        if b["eventID"] == event_id
    ]
    assert bands, "no band for the saved event"
    band = bands[0]
    assert band["payloadType"] == "IMPORT_CAPACITY_LIMIT", (
        f"the band must say what it carries, got {band['payloadType']!r}"
    )
    assert band["value"] == 2.5, f"the band must carry the limit's value, got {band['value']!r}"
    assert band["from"] < band["to"], "a band must span forwards in time"


@then("no event was left unread")
def step_no_event_left_unread(context):
    rejected = context.fleet_signals.get("rejectedEvents")
    assert rejected == 0, (
        f"{rejected} event(s) could not be parsed — the overlay is incomplete and "
        "a shorter list of bands would look like a quieter grid"
    )

