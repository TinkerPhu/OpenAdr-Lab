"""Step definitions for Phase A physics and capability BDD tests.

Covers: capability() state-dependence (battery SoC bounds, EV unplugged, PV is_fixed)
and UserOverrides paths (pv_irradiance, ev_plugged).
"""

import time
from datetime import datetime, timezone
from behave import given, when, then, step
from features.helpers.api_client import ven_post, ven_get
from features.helpers.wait import poll_until


# ── Given: setup helpers ──────────────────────────────────────────────────────

@given("the battery SoC is reset to {soc:f}")
def step_given_battery_soc_reset(context, soc):
    """Force battery SoC to a specific value via POST /sim/reset/battery."""
    r = ven_post("/sim/reset/battery", json={"soc": soc})
    assert r.status_code == 204, (
        f"Expected 204 from /sim/reset/battery, got {r.status_code}: {r.text}"
    )


@given("the system is idle")
def step_given_system_idle(context):
    """Wait until the VEN has produced at least one plan, then pause briefly
    so the planner loop is idle before the next step injects state.

    Stores the current plan's created_at in context.idle_plan_ts so that
    the 'no plan cycle' assertion can detect any subsequent solve.
    """
    def fetch():
        resp = ven_get("/plan")
        if not resp.ok:
            return None
        body = resp.json()
        return body if (body and "created_at" in body) else None

    poll_until(
        fetch,
        lambda p: p is not None,
        timeout=150,
        description="VEN /plan returns a plan with created_at",
    )
    # Brief pause to ensure any in-flight replan (e.g. triggered by a previous
    # scenario's cleanup removing a rate event) has time to complete and be adopted.
    time.sleep(3)
    # Re-read plan AFTER the sleep so the baseline is the latest adopted plan.
    # Without this, a background replan that fires during the sleep would cause
    # the 'no plan cycle' assertion to fail spuriously.
    resp = ven_get("/plan")
    body = resp.json() if resp.ok else None
    assert body and "created_at" in body, "VEN /plan missing created_at after idle sleep"
    context.idle_plan_ts = body["created_at"]


# ── When: SoC reset and override helpers ─────────────────────────────────────

@given("I inject ev_soc {soc:f} via sim inject")
def step_given_inject_ev_soc(context, soc):
    r = ven_post("/sim/inject", json={"ev_soc": soc})
    r.raise_for_status()


@given("I inject heater_temp_c {temp:f} via sim inject")
def step_given_inject_heater_temp_c(context, temp):
    """One-shot reset of the heater's current temperature via POST /sim/inject.

    Blocks until GET /sim reflects the injected value: the inject is applied by
    the next 1 s sim tick, and a planner cycle that clones the sim inside that
    window would otherwise solve against the pre-inject tank state — making a
    subsequent "wait for recomputed plan" step accept a plan built without the
    injection (observed as a load-dependent flake on the near-T_max scenario).

    Records `context.plan_freshness_cutoff` *before* sending the request: the
    POST synchronously fires PlanTrigger::AssetStateChange, so the resulting
    plan's created_at can land before a cutoff captured only after this step
    returns (this step can block here for several seconds waiting for the sim
    tick) — the subsequent "wait for recomputed plan" step would then never see
    a "fresh" plan from this injection at all and fall back to waiting for an
    unrelated, non-deterministic later trigger (the actual root cause of the
    load-dependent flake this docstring already flagged, not just the
    stale-tank-state race the polling below fixes).
    """
    context.plan_freshness_cutoff = datetime.now(timezone.utc)
    r = ven_post("/sim/inject", json={"heater_temp_c": temp})
    r.raise_for_status()

    def heater_temp():
        resp = ven_get("/sim")
        if not resp.ok:
            return None
        return (resp.json().get("assets") or {}).get("heater", {}).get("temp_c")

    poll_until(
        heater_temp,
        lambda t: t is not None and abs(t - temp) < 0.2,
        timeout=15,
        interval=0.5,
        description=f"sim heater temp_c reflects injected {temp}",
    )


@given("I inject pv irradiance {irradiance:f} via sim inject")
def step_given_inject_pv_irradiance(context, irradiance):
    r = ven_post("/sim/inject", json={"pv_irradiance": irradiance})
    r.raise_for_status()


@step("I set pv plan forecast to {kw:f} kW")
def step_given_set_pv_plan_forecast(context, kw):
    """Set the MILP planning-horizon PV forecast to a fixed value via POST /sim/inject.
    This overrides all 24h forecast slots; does NOT trigger a replan.
    """
    r = ven_post("/sim/inject", json={"pv_plan_kw": kw})
    assert r.status_code == 204, (
        f"Expected 204 from POST /sim/inject with pv_plan_kw={kw}, got {r.status_code}: {r.text}"
    )


@when("I POST a sim override setting pv_irradiance to {irradiance:f}")
def step_sim_override_pv_irradiance(context, irradiance):
    r = ven_post("/sim/inject", json={"pv_irradiance": irradiance})
    r.raise_for_status()
    context.last_response = r


@when("I POST a sim override setting grid_export_limit_kw to {kw:f}")
def step_sim_override_grid_export_limit_kw(context, kw):
    """Set the VTN/sim-injected grid export capacity limit (kW, positive magnitude).

    Regression: PvInverter.export_limit_kw — the field step_inner actually clamps
    against — was never written by any live tick-pipeline code path, only by unit
    tests. This exercises the real path (OadrCapacityState -> resolve_pv_export_limit_kw
    -> SimState::tick's pv_export_limit_override) end to end.
    """
    r = ven_post("/sim/inject", json={"grid_export_limit_kw": kw})
    r.raise_for_status()
    context.last_response = r


@when("I wait {seconds:d} seconds for the sim to tick")
def step_wait_sim_tick(context, seconds):
    time.sleep(seconds)


# ── Then: no-replan assertion ─────────────────────────────────────────────────

@then("no plan cycle is triggered within {sec:d} seconds")
def step_no_plan_cycle(context, sec):
    """Assert the inject does not trigger a solve: for `sec` seconds, no newly adopted plan
    carries the `ASSET_STATE_CHANGE` trigger an inject would send.

    Polls GET /plan every 500 ms. A new plan for any *other* reason (the periodic grid, a
    previous scenario's request still being planned) is not this scenario's business and
    does not fail it: asserting "no plan at all" failed on 2026-10-05 when an unrelated plan
    landed 4 s after the idle baseline. The trigger is the claim the scenario makes.
    """
    baseline_ts = getattr(context, "idle_plan_ts", None)
    assert baseline_ts is not None, (
        "'Given the system is idle' must precede this step to record baseline plan timestamp"
    )

    deadline = time.time() + sec
    while time.time() < deadline:
        resp = ven_get("/plan")
        if resp.ok:
            body = resp.json() or {}
            current_ts = body.get("created_at")
            if current_ts and current_ts != baseline_ts and body.get("trigger") == "ASSET_STATE_CHANGE":
                raise AssertionError(
                    f"A plan triggered by ASSET_STATE_CHANGE was adopted within {sec}s of the "
                    f"pv_plan_kw inject (baseline {baseline_ts!r}, new {current_ts!r})"
                )
        time.sleep(0.5)


# ── Then: capability assertions ───────────────────────────────────────────────
# Uses context.last_response_json set by the shared "I GET {path} from the VEN" step.

@then("the capability max_import_kw is {expected:f}")
def step_capability_max_import(context, expected):
    data = context.last_response_json
    assert data is not None, "No capability JSON in context (request failed?)"
    actual = data.get("max_import_kw")
    assert actual is not None, f"'max_import_kw' missing from response: {data}"
    assert abs(actual - expected) < 1e-6, (
        f"Expected max_import_kw={expected}, got {actual}"
    )


@then("the capability max_export_kw is {expected:f}")
def step_capability_max_export(context, expected):
    data = context.last_response_json
    assert data is not None, "No capability JSON in context (request failed?)"
    actual = data.get("max_export_kw")
    assert actual is not None, f"'max_export_kw' missing from response: {data}"
    assert abs(actual - expected) < 1e-6, (
        f"Expected max_export_kw={expected}, got {actual}"
    )


@then("the capability max_import_kw is less than {threshold:f}")
def step_capability_max_import_lt(context, threshold):
    data = context.last_response_json
    assert data is not None, "No capability JSON in context (request failed?)"
    actual = data.get("max_import_kw")
    assert actual is not None, f"'max_import_kw' missing from response: {data}"
    # IEEE 754: -0.0 < 0.0 is False. Treat values within 1e-6 of threshold as passing.
    assert actual < threshold + 1e-6, (
        f"Expected max_import_kw < {threshold}, got {actual}"
    )


@when("I remember max_export_kw as the pre-override baseline")
def step_remember_export_baseline(context):
    """How much PV there is to silence, measured rather than assumed.

    The scenario sets this with a full-irradiance override first, so it holds at any
    hour. The residual after the zero override is a fraction of the output, so the
    assertion has to be a fraction too - see
    `step_capability_max_export_is_fraction_of_baseline`.
    """
    data = context.last_response_json
    assert data is not None, "No capability JSON in context (request failed?)"
    context.pv_export_baseline = abs(data.get("max_export_kw") or 0.0)
    assert context.pv_export_baseline > 0.1, (
        "there is no PV output to silence: the full-irradiance override left export at "
        f"{context.pv_export_baseline} kW"
    )


@then("the capability max_export_kw magnitude is at most {pct:g} percent of the baseline")
def step_capability_max_export_is_fraction_of_baseline(context, pct):
    """Scale-free, because the quantity being bounded is not scale-free.

    A one-shot pv_irradiance override auto-clears after one tick and the offset then
    EMA-decays back toward the natural sin model over a ~300 s window, so a few
    seconds later there is always a small residual - and its SIZE is proportional to
    how much sun there is at that moment. An absolute bound therefore passes in the
    morning and fails near solar noon: observed 2026-10-04, the same scenario passed
    at 10:06Z with ~2.5 kW of natural export and failed at 10:58Z with 3.34 kW, where
    the residual was 0.0125 kW against a fixed 0.01 kW threshold. Both readings are
    the same ~0.4 % of natural output, which is what the bound should have been
    measuring all along.
    """
    data = context.last_response_json
    assert data is not None, "No capability JSON in context (request failed?)"
    actual = abs(data.get("max_export_kw") or 0.0)
    baseline = getattr(context, "pv_export_baseline", None)
    assert baseline, "no pre-override baseline was remembered"
    limit = baseline * pct / 100.0
    assert actual <= limit, (
        f"expected |max_export_kw| <= {pct}% of the {baseline:.3f} kW baseline "
        f"({limit:.4f} kW), got {actual:.4f} kW"
    )


@then("the capability max_export_kw magnitude is less than {threshold:f}")
def step_capability_max_export_magnitude_lt(context, threshold):
    """A one-shot pv_irradiance override auto-clears after 1 tick and its offset
    then EMA-decays back toward the natural (non-zero) sin model — deliberately
    slowly, tuned for slider-drag smoothness over a 300 s reference window (see
    PvSmoothingState). So a strict is_fixed=true (exact zero) check is never
    satisfiable a few seconds after a single override post; a magnitude bound
    is the correct assertion here, matching max_import_kw's sibling check.
    """
    data = context.last_response_json
    assert data is not None, "No capability JSON in context (request failed?)"
    actual = data.get("max_export_kw")
    assert actual is not None, f"'max_export_kw' missing from response: {data}"
    assert abs(actual) < threshold + 1e-6, (
        f"Expected |max_export_kw| < {threshold}, got {actual}"
    )


# ── Polling capability steps ──────────────────────────────────────────────────

@when('I wait for the VEN /capability/{asset} {field} to equal {expected:f}')
def step_poll_capability(context, asset, field, expected):
    """Poll GET /capability/{asset} until `field` matches `expected` (±1e-6)."""
    def fetch():
        r = ven_get(f"/capability/{asset}")
        r.raise_for_status()
        return r.json()

    result = poll_until(
        fetch,
        lambda data: abs(data.get(field, float('inf')) - expected) < 1e-6,
        timeout=120,
        interval=2,
        description=f"/capability/{asset} {field}=={expected}",
    )
    context.polled_capability = result


@then("the polled capability matched")
def step_polled_capability_matched(context):
    assert context.polled_capability is not None, "No polled capability result"

