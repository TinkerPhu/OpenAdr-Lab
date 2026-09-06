"""Step definitions for the unified capacity/envelope engine's absolute-quantity
behavior (`unified-capacity-envelope-engine`, Spec E) — GET /flexibility/forecast
and GET /flexibility/capacity, exercised end-to-end (dispatcher tick ->
controller::capacity_envelope -> HTTP), not just the Rust unit tests already
covering the same logic in isolation.
"""

from behave import then, when
from features.helpers.api_client import ven_get
from features.helpers.wait import poll_until


# ── Site Headroom: absolute, not plan-relative ──────────────────────────────


def _all_assets_import_capability_kw() -> float:
    """Sum of every controllable asset's own live max_import_kw (kW), read
    fresh from `/capability/*` — mirrors `_non_pv_import_capability_kw`'s own
    pattern below (and `site_headroom_steps.py::_storage_export_kw`'s).
    """
    total = 0.0
    for asset in ("battery", "ev", "heater", "base_load"):
        r = ven_get(f"/capability/{asset}")
        if r.status_code != 200:
            continue  # profile legitimately has no such asset
        total += abs(r.json().get("max_import_kw", 0.0) or 0.0)
    return total


@when("I wait for the VEN site headroom forecast to be available")
def step_wait_for_headroom_forecast_available(context):
    def _fetch():
        r = ven_get("/flexibility/forecast")
        return r.json() if r.status_code == 200 else None

    context.headroom_forecast = poll_until(
        _fetch,
        lambda slots: bool(slots),
        timeout=60,
        interval=3,
        description="site headroom forecast available",
    )


@then(
    "the site headroom forecast's first slot does not exceed the site's controllable assets' own combined live import capability"
)
def step_headroom_first_slot_bounded_by_capability(context):
    first_slot = context.headroom_forecast[0]
    bound_kw = _all_assets_import_capability_kw()
    tolerance_kw = 0.5
    assert first_slot["down_kw"] <= bound_kw + tolerance_kw, (
        f"site headroom forecast's first slot (down_kw={first_slot['down_kw']:.2f} kW) "
        f"exceeds the sum of every controllable asset's own live import capability "
        f"({bound_kw:.2f} kW) -- the signature of a plan-relative-delta computation "
        "(or a double-counting bug) reappearing instead of each asset's own absolute "
        f"max_effort_setpoint. Full slot: {first_slot}"
    )


# ── Capacity Forecast: PV-Import bug fixed by construction ──────────────────


def _non_pv_import_capability_kw() -> float:
    """Sum of every non-PV controllable asset's own live max_import_kw (kW).

    Read from the live capability endpoints rather than hardcoded, so the
    bound stays correct if a profile's ratings change — mirrors
    `site_headroom_steps.py::_storage_export_kw`'s own pattern.
    """
    total = 0.0
    for asset in ("battery", "ev", "heater", "base_load"):
        r = ven_get(f"/capability/{asset}")
        if r.status_code != 200:
            continue  # profile legitimately has no such asset
        total += abs(r.json().get("max_import_kw", 0.0) or 0.0)
    return total


@when("I wait for the VEN capacity forecast to reflect the injected irradiance")
def step_wait_for_capacity_forecast_to_reflect_irradiance(context):
    # Confirm the inject has actually propagated through a dispatcher tick
    # (PV genuinely generating) before reading the capacity forecast --
    # otherwise a stale pre-inject snapshot could pass this scenario for the
    # wrong reason.
    def _pv_generating():
        r = ven_get("/sim")
        if r.status_code != 200:
            return None
        pv = r.json().get("assets", {}).get("pv")
        return pv if pv and pv.get("power_kw", 0.0) < -0.5 else None

    poll_until(
        _pv_generating,
        lambda pv: pv is not None,
        timeout=30,
        interval=2,
        description="PV genuinely generating after the irradiance inject",
    )

    def _fetch_capacity():
        r = ven_get("/flexibility/capacity")
        return r.json() if r.status_code == 200 else None

    context.capacity_curves = poll_until(
        _fetch_capacity,
        lambda curves: curves is not None,
        timeout=30,
        interval=2,
        description="capacity forecast available after the irradiance inject",
    )


@then(
    "the import capacity curve's first step does not exceed the site's non-PV controllable assets' own import capability"
)
def step_import_curve_bounded_by_non_pv_capability(context):
    import_curve = context.capacity_curves["import"]
    first_step_kw = import_curve["steps"][0]["power_kw"]
    bound_kw = _non_pv_import_capability_kw()
    tolerance_kw = 0.5

    assert first_step_kw <= bound_kw + tolerance_kw, (
        f"import capacity curve's first step ({first_step_kw:.2f} kW) exceeds "
        f"the site's non-PV controllable assets' own import capability "
        f"({bound_kw:.2f} kW) while PV is actively generating -- the "
        "signature of the confirmed PV-Import bug (crediting PV's own "
        "generation as import headroom) reappearing."
    )
