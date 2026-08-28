"""Step definitions for the forward site-headroom forecast (GET /flexibility/forecast).

Scope note: these steps assert the *invariant* — after dark the forecast never
advertises more export headroom than storage discharge plus the consumption the
plan itself has scheduled (both genuine export-direction flexibility; PV is not)
— and exercise the live wiring (dispatcher tick -> pre-lock weather resolution
-> per-slot frames -> endpoint). The multi-zone slot-alignment mechanism is covered
deterministically by the Rust regression test
`simulator::forecast::tests::pv_ceiling_is_evaluated_at_each_slots_own_timestamp_on_a_zoned_horizon`,
which pins a fixed `now` and a two-zone horizon; the E2E profiles are
single-zone, so this scenario cannot and does not stand in for it.
"""

from datetime import datetime, timezone

from behave import then, when
from features.helpers.api_client import ven_get
from features.helpers.wait import poll_until


def _parse_ts(raw: str) -> datetime:
    return datetime.fromisoformat(raw.replace("Z", "+00:00")).astimezone(timezone.utc)


def _storage_export_kw() -> float:
    """Total export power the site's storage can supply right now (kW, positive).

    Read from the live capability endpoints rather than hardcoded, so the
    bound stays correct if a profile's ratings change.
    """
    total = 0.0
    for asset in ("battery", "ev"):
        r = ven_get(f"/capability/{asset}")
        if r.status_code != 200:
            continue  # profile legitimately has no such asset
        total += abs(r.json().get("max_export_kw", 0.0) or 0.0)
    return total


@when("I wait for the VEN site headroom forecast to cover the planning horizon")
def step_wait_for_headroom_forecast(context):
    def _fetch():
        r = ven_get("/flexibility/forecast")
        return r.json() if r.status_code == 200 else None

    def _covers_horizon(slots):
        if not slots:
            return False
        span_h = (_parse_ts(slots[-1]["ts"]) - _parse_ts(slots[0]["ts"])).total_seconds() / 3600.0
        # The E2E "test" profile plans 24h; require most of it so the window is
        # guaranteed to include night slots whatever time the suite runs.
        return span_h >= 12.0

    context.headroom_forecast = poll_until(
        _fetch,
        _covers_horizon,
        timeout=180,
        interval=5,
        description="site headroom forecast spanning at least 12h",
    )


def _planned_sheddable_kw_by_ts() -> dict:
    """Per-slot consumption the plan intends to draw, which shedding could free.

    `up_kw` is the sum over assets of `planned_kw - cap_max_export_kw`, so
    turning off a heater that the plan has running is genuine export-direction
    headroom, exactly like discharging the battery. Read from `/plan` — an
    endpoint independent of the headroom computation — so this stays a
    cross-check rather than a restatement of the same arithmetic.
    """
    r = ven_get("/plan")
    if r.status_code != 200:
        return {}
    out = {}
    for slot in r.json().get("slots", []):
        planned = slot.get("planned_kw_by_asset") or {}
        out[slot["start"]] = sum(kw for kw in planned.values() if kw and kw > 0.0)
    return out


@then("no night slot in the site headroom forecast claims PV export capability")
def step_night_slots_have_no_pv_headroom(context):
    """After dark the only legitimate sources of `up_kw` are storage discharge
    and shedding whatever consumption the plan has scheduled — never PV.

    A PV ceiling misresolved from a daytime hour shows up as headroom in
    excess of both, which is what this bounds.
    """
    storage_kw = _storage_export_kw()
    sheddable_by_ts = _planned_sheddable_kw_by_ts()
    tolerance_kw = 0.5

    night = [
        s
        for s in context.headroom_forecast
        if not 6.0 <= (_parse_ts(s["ts"]).hour + _parse_ts(s["ts"]).minute / 60.0) <= 18.0
    ]
    assert night, (
        "fixture spans no night slots, so this scenario proved nothing — "
        f"forecast covers {context.headroom_forecast[0]['ts']} .. "
        f"{context.headroom_forecast[-1]['ts']}"
    )

    offenders = []
    for s in night:
        limit_kw = storage_kw + sheddable_by_ts.get(s["ts"], 0.0) + tolerance_kw
        if s["up_kw"] > limit_kw:
            offenders.append(
                (s["ts"], round(s["up_kw"], 2), round(limit_kw, 2))
            )

    assert not offenders, (
        "night slots claimed export headroom beyond storage discharge "
        f"({storage_kw:.2f} kW) plus the plan's own sheddable load — the "
        "signature of a PV ceiling resolved from the wrong slot. "
        f"(ts, up_kw, allowed): {offenders[:5]}"
    )
