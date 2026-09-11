"""Step definitions for the unified capacity/envelope engine's absolute-quantity
behavior (`unified-capacity-envelope-engine`, Spec E) — GET /flexibility/forecast
and GET /flexibility/capacity, exercised end-to-end (dispatcher tick ->
controller::capacity_headroom -> HTTP), not just the Rust unit tests already
covering the same logic in isolation.
"""

from behave import then, when
from features.helpers.api_client import ven_get
from features.helpers.wait import poll_until


def _live_assets():
    """The full live `/sim` asset map, `{}` on failure.

    Reads EVERY asset actually present, not a hardcoded fixed-name list
    (`battery`/`ev`/`heater`/`base_load`) — found via a full-suite run: a
    dynamically-named `ShiftableLoadAsset` (e.g. `wm-2`) left over from a
    PRECEDING `@isolated` scenario in the same VEN instance is a real asset
    with real import capability, and `/capability/{asset}` only exists for
    the fixed-name kinds, so a fixed-list sum silently misses it. `/sim`
    itself has no such blind spot — every asset instance appears there,
    dynamic or not, each carrying its own `cap_max_import_kw`.
    """
    r = ven_get("/sim")
    return r.json().get("assets", {}) if r.status_code == 200 else {}


def _import_capability_kw(assets: dict, *, exclude_types: set[str] = frozenset()) -> float:
    """Sum of `cap_max_import_kw` across every asset in `assets` whose
    `asset_type` is not in `exclude_types`.
    """
    return sum(
        abs(a.get("cap_max_import_kw", 0.0) or 0.0)
        for a in assets.values()
        if a.get("asset_type") not in exclude_types
    )


# ── Site Headroom: absolute, not plan-relative ──────────────────────────────


@when("I wait for the VEN site headroom forecast to be available")
def step_wait_for_headroom_forecast_available(context):
    # base_load's own max_import_kw is its LIVE, noise-profile-driven current
    # draw, not a fixed rating -- it can genuinely move between two separate
    # HTTP round-trips. Fetch the forecast and the bounding asset snapshot
    # back-to-back, in the SAME retry attempt, and store both together, so
    # the `then` step compares two readings taken close in time (found via a
    # full-suite run under contention: a reproducible multi-kW gap between
    # separately-timed reads, not a logic bug).
    def _fetch_forecast_and_assets():
        r = ven_get("/flexibility/forecast")
        if r.status_code != 200:
            return None
        slots = r.json()
        if not slots:
            return None
        return slots, _live_assets()

    context.headroom_forecast, context.headroom_live_assets = poll_until(
        _fetch_forecast_and_assets,
        lambda result: result is not None,
        timeout=60,
        interval=3,
        description="site headroom forecast and live asset snapshot captured together",
    )


@then(
    "the site headroom forecast's first slot does not exceed the site's controllable assets' own combined live import capability"
)
def step_headroom_first_slot_bounded_by_capability(context):
    first_slot = context.headroom_forecast[0]
    # PV never contributes to Import (design.md D1 -- max_effort_setpoint is
    # a trivial constant 0.0), so it's excluded from the bound the same way
    # compute_site_headroom_forecast itself excludes it.
    bound_kw = _import_capability_kw(context.headroom_live_assets, exclude_types={"pv"})
    tolerance_kw = 0.5
    assert first_slot["down_kw"] <= bound_kw + tolerance_kw, (
        f"site headroom forecast's first slot (down_kw={first_slot['down_kw']:.2f} kW) "
        f"exceeds the sum of every live asset's own import capability "
        f"({bound_kw:.2f} kW) -- the signature of a plan-relative-delta computation "
        "(or a double-counting bug) reappearing instead of each asset's own absolute "
        f"max_effort_setpoint. Full slot: {first_slot}"
    )


# ── Capacity Forecast: PV-Import bug fixed by construction ──────────────────


@when("I wait for the VEN capacity forecast to reflect the injected irradiance")
def step_wait_for_capacity_forecast_to_reflect_irradiance(context):
    # Confirm the inject has actually propagated through a dispatcher tick
    # (PV genuinely generating) before reading the capacity forecast --
    # otherwise a stale pre-inject snapshot could pass this scenario for the
    # wrong reason.
    def _pv_generating():
        assets = _live_assets()
        pv = assets.get("pv")
        return pv if pv and pv.get("power_kw", 0.0) < -0.5 else None

    poll_until(
        _pv_generating,
        lambda pv: pv is not None,
        timeout=30,
        interval=2,
        description="PV genuinely generating after the irradiance inject",
    )

    # Fetch the capacity curve and the bounding asset snapshot back-to-back,
    # in the same retry attempt, and store both together -- same reasoning
    # as the sibling headroom scenario's own comment above (base_load's live
    # draw, and any other asset present, can move between separate reads).
    def _fetch_capacity_and_assets():
        r = ven_get("/flexibility/capacity")
        if r.status_code != 200:
            return None
        curves = r.json()
        if not curves:
            return None
        return curves, _live_assets()

    context.capacity_curves, context.capacity_live_assets = poll_until(
        _fetch_capacity_and_assets,
        lambda result: result is not None,
        timeout=30,
        interval=2,
        description="capacity forecast and live asset snapshot captured together after the irradiance inject",
    )


@then(
    "the import capacity curve's first step does not exceed the site's non-PV controllable assets' own import capability"
)
def step_import_curve_bounded_by_non_pv_capability(context):
    import_curve = context.capacity_curves["import"]
    first_step_kw = import_curve["steps"][0]["power_kw"]
    bound_kw = _import_capability_kw(context.capacity_live_assets, exclude_types={"pv"})
    tolerance_kw = 0.5

    assert first_step_kw <= bound_kw + tolerance_kw, (
        f"import capacity curve's first step ({first_step_kw:.2f} kW) exceeds "
        f"the site's non-PV controllable assets' own import capability "
        f"({bound_kw:.2f} kW) while PV is actively generating -- the "
        "signature of the confirmed PV-Import bug (crediting PV's own "
        "generation as import headroom) reappearing."
    )


# ── Site Headroom "now": PV neither inflates nor reduces the import side ────

# PV-induced error would be ~5 kW (test profile PV rated 5 kW at irradiance
# 1.0); 1 kW absorbs base load's live jitter between the paired reads.
_TWO_SIDED_TOLERANCE_KW = 1.0


@when("I wait for PV to generate and capture the live site headroom with the live asset snapshot")
def step_capture_live_headroom_while_pv_generates(context):
    def _capture():
        assets = _live_assets()
        pv = assets.get("pv")
        if not pv or pv.get("power_kw", 0.0) > -0.5:
            return None
        headroom = ven_get("/flexibility")
        curves = ven_get("/flexibility/capacity")
        if headroom.status_code != 200 or curves.status_code != 200 or not curves.json():
            return None
        return headroom.json(), curves.json(), _live_assets()

    context.live_headroom, context.capacity_curves, context.capacity_live_assets = poll_until(
        _capture,
        lambda result: result is not None,
        timeout=30,
        interval=2,
        description="live site headroom, capacity curves and asset snapshot captured while PV generates",
    )


def _assert_matches_non_pv_import_capability(actual_kw: float, what: str, context):
    expected_kw = _import_capability_kw(context.capacity_live_assets, exclude_types={"pv"})
    assert abs(actual_kw - expected_kw) <= _TWO_SIDED_TOLERANCE_KW, (
        f"{what} ({actual_kw:.2f} kW) differs from the site's non-PV controllable "
        f"assets' own import capability ({expected_kw:.2f} kW) while PV generates "
        f"{context.capacity_live_assets['pv']['power_kw']:.2f} kW -- PV can be "
        "curtailed to 0, so it must neither add to nor subtract from max import."
    )


@then("the live site headroom's import side equals the site's non-PV controllable assets' own import capability")
def step_live_headroom_import_matches_non_pv_capability(context):
    _assert_matches_non_pv_import_capability(
        context.live_headroom["down_kw"], "live site headroom down_kw", context
    )


@then("the live site headroom's import side includes the EV's own live import capability")
def step_live_headroom_import_includes_ev_capability(context):
    def _capture():
        r = ven_get("/flexibility")
        return (r.json(), _live_assets()) if r.status_code == 200 else None

    headroom, assets = poll_until(
        _capture,
        lambda result: result is not None,
        timeout=30,
        interval=2,
        description="live site headroom and asset snapshot captured together",
    )
    ev_cap_kw = assets.get("ev", {}).get("cap_max_import_kw", 0.0)
    assert ev_cap_kw > 0.0, (
        f"precondition: the scheduled EV session should leave the EV able to charge "
        f"(plugged, below target), but its import capability is {ev_cap_kw:.2f} kW: {assets.get('ev')}"
    )
    assert headroom["down_kw"] >= ev_cap_kw - 0.5, (
        f"site headroom down_kw ({headroom['down_kw']:.2f} kW) is below the EV's own "
        f"import capability ({ev_cap_kw:.2f} kW) -- the EV's charge ceiling is missing."
    )


@then(
    "the captured import capacity curve's first step equals the site's non-PV controllable assets' own import capability"
)
def step_import_curve_first_step_matches_non_pv_capability(context):
    _assert_matches_non_pv_import_capability(
        context.capacity_curves["import"]["steps"][0]["power_kw"],
        "import capacity curve's first step",
        context,
    )
