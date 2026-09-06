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


@when("I wait for the VEN site headroom forecast to reflect a full battery")
def step_wait_for_headroom_to_reflect_full_battery(context):
    def _fetch():
        r = ven_get("/flexibility/forecast")
        return r.json() if r.status_code == 200 else None

    def _all_slots_zero_down(slots):
        # down_kw is the SUM of every asset's absolute import headroom, so
        # this only proves the invariant once at least one slot is present
        # to check (an empty forecast would vacuously satisfy `all()`).
        return bool(slots) and all(s["down_kw"] <= 0.01 for s in slots)

    context.headroom_forecast = poll_until(
        _fetch,
        _all_slots_zero_down,
        timeout=120,
        interval=3,
        description="site headroom forecast reflecting a fully-charged battery (down_kw settles to ~0)",
    )


@then("no slot in the site headroom forecast credits the battery with import headroom")
def step_no_slot_credits_battery_import_headroom(context):
    # The wait step above already polled for this; re-assert on the stored
    # result for a clear, independent failure message if it somehow regresses
    # between the wait and this step (e.g. a stray replan).
    offenders = [s for s in context.headroom_forecast if s["down_kw"] > 0.01]
    assert not offenders, (
        "a fully-charged battery must contribute 0.0 absolute import headroom "
        f"(down_kw) at every slot, got: {offenders[:5]}"
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
