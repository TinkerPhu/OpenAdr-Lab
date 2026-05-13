"""Step definitions for Phase A physics and capability BDD tests.

Covers: capability() state-dependence (battery SoC bounds, EV unplugged, PV is_fixed)
and UserOverrides paths (pv_irradiance, ev_plugged).
"""

import time
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

    plan = poll_until(
        fetch,
        lambda p: p is not None,
        timeout=150,
        description="VEN /plan returns a plan with created_at",
    )
    # Record the solve timestamp before the inject so the Then step can detect change.
    context.idle_plan_ts = plan["created_at"]
    # Brief pause to ensure the planner loop has fully settled.
    time.sleep(3)


# ── When: SoC reset and override helpers ─────────────────────────────────────

@when("the battery SoC is reset to {soc:f}")
def step_when_battery_soc_reset(context, soc):
    """Force battery SoC to a specific value via POST /sim/reset/battery."""
    r = ven_post("/sim/reset/battery", json={"soc": soc})
    assert r.status_code == 204, (
        f"Expected 204 from /sim/reset/battery, got {r.status_code}: {r.text}"
    )


@given("I inject ev_soc {soc:f} via sim inject")
def step_given_inject_ev_soc(context, soc):
    r = ven_post("/sim/inject", json={"ev_soc": soc})
    r.raise_for_status()


@given("I inject heater_temp_c {temp:f} via sim inject")
def step_given_inject_heater_temp_c(context, temp):
    """One-shot reset of the heater's current temperature via POST /sim/inject."""
    r = ven_post("/sim/inject", json={"heater_temp_c": temp})
    r.raise_for_status()


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


@when("I wait {seconds:d} seconds for the sim to tick")
def step_wait_sim_tick(context, seconds):
    time.sleep(seconds)


# ── Then: no-replan assertion ─────────────────────────────────────────────────

@then("no plan cycle is triggered within {sec:d} seconds")
def step_no_plan_cycle(context, sec):
    """Assert the MILP planner does not start a new solve for `sec` seconds.

    Polls GET /plan every 500 ms for `sec` seconds.  If the plan's created_at
    advances beyond the value captured in context.idle_plan_ts (set by 'Given
    the system is idle'), a new solve fired and the assertion fails.

    Uses 500 ms poll interval so a spurious solve firing within the window is
    reliably detected even on Pi4 ARM64.
    """
    baseline_ts = getattr(context, "idle_plan_ts", None)
    assert baseline_ts is not None, (
        "'Given the system is idle' must precede this step to record baseline plan timestamp"
    )

    deadline = time.time() + sec
    while time.time() < deadline:
        resp = ven_get("/plan")
        if resp.ok:
            body = resp.json()
            current_ts = body.get("created_at") if body else None
            if current_ts and current_ts != baseline_ts:
                raise AssertionError(
                    f"Unexpected plan cycle fired within {sec}s of pv_plan_kw inject. "
                    f"Baseline created_at={baseline_ts!r}, new created_at={current_ts!r}"
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


@then("the capability is_fixed is true")
def step_capability_is_fixed(context):
    data = context.last_response_json
    assert data is not None, "No capability JSON in context (request failed?)"
    assert data.get("is_fixed") is True, (
        f"Expected is_fixed=true, got is_fixed={data.get('is_fixed')}. "
        f"Full response: {data}"
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

