"""Step definitions for VEN Dispatcher (Stage 4) BDD tests."""

from behave import given, when, then
from features.helpers.api_client import ven_get, ven_post, ven_put
from features.helpers.wait import poll_until


# ---------------------------------------------------------------------------
# When: poll for ledger state
# ---------------------------------------------------------------------------

@when('I poll VEN /ledger until field "{field}" is greater than {threshold:f}')
def step_poll_ledger_field(context, field, threshold):
    """Poll GET /ledger until a dotted field exceeds a threshold."""
    def fetch():
        r = ven_get("/ledger")
        r.raise_for_status()
        return r.json()

    def resolve(data, path):
        for part in path.split("."):
            if not isinstance(data, dict):
                return None
            data = data.get(part)
        return data

    def check(data):
        val = resolve(data, field)
        return isinstance(val, (int, float)) and val > threshold

    context.last_response_json = poll_until(
        fetch, check, timeout=15,
        description=f"VEN /ledger field '{field}' > {threshold}",
    )


# ---------------------------------------------------------------------------
# Then: packet status assertions
# ---------------------------------------------------------------------------

@then('the response JSON field "{field_path}" is the string "{expected}"')
def step_response_json_field_is_string(context, field_path, expected):
    """Assert a nested JSON field equals a specific string value."""
    data = context.last_response_json
    assert data is not None, "Response was not JSON"

    def resolve(d, path):
        parts = path.split(".")
        for part in parts:
            if not isinstance(d, dict):
                return None
            d = d.get(part)
        return d

    val = resolve(data, field_path)
    assert val == expected, (
        f"Field '{field_path}' = {val!r}, expected string '{expected}'"
    )


# ---------------------------------------------------------------------------
# Layer 1 — reactive battery correction
# ---------------------------------------------------------------------------

@when("I inject base_load_kw {kw:f} with alpha {alpha:f} via sim inject")
def step_inject_base_load(context, kw, alpha):
    """Inject a persistent base-load offset into the VEN sim."""
    r = ven_post("/sim/inject", json={"base_load_kw": kw, "base_load_alpha": alpha})
    r.raise_for_status()


@when("I clear the base_load_kw inject")
def step_clear_base_load_inject(context):
    """Clear the base-load override so the sim returns to its natural base load.

    `null`, not `0.0`. The route takes these as double options
    (`Option<Option<f64>>`): `null` clears the override, while `0.0` *sets* it to
    zero, and `SimState::next_offset_kw` then returns `forced_kw -
    natural_base_kw` every tick -- pinning base load at exactly 0 kW rather than
    releasing it. The docstring here claimed it reset the override; it did not.
    """
    r = ven_post("/sim/inject", json={"base_load_kw": None, "base_load_alpha": None})
    r.raise_for_status()


@given("the deviation arbiter is enabled")
def step_deviation_arbiter_enabled(context):
    """BL-37: flip the arbiter rollout gate on (PUT /arbiter-settings)."""
    r = ven_put("/arbiter-settings", json={"deviation_arbiter_enabled": True})
    r.raise_for_status()


@when("the deviation arbiter is disabled")
def step_deviation_arbiter_disabled(context):
    """BL-37: reset the arbiter rollout gate to its default so later
    scenarios start clean."""
    r = ven_put("/arbiter-settings", json={"deviation_arbiter_enabled": False})
    r.raise_for_status()


@given("the arbiter has no deviation it would keep if released")
def step_arbiter_has_no_held_deviation(context):
    """R-88: a correction is released once the deviation with battery/EV back at plan is
    inside the 0.1 kW dead band. A scenario asserting that release first needs a site where
    that is true *without* its own disturbance; otherwise the correction it provokes is held
    by something else and "cleared" can never come. Fails naming that gap rather than
    letting a later wait time out with nothing to go on.

    A replan is forced first: resetting the sim overrides does not replan, so the plan in
    force can still be one an earlier scenario's override shaped (on 2026-10-05 a
    `pv_plan_kw` of 0 from the main pass left a plan expecting no PV, a standing 2.9 kW gap
    the periodic replan, every 300 s, had not yet corrected). `/plan/trigger` is a replan the
    VEN cannot ignore, so the check runs against a plan made from the reset state.
    """
    from datetime import datetime, timezone

    cutoff = datetime.now(timezone.utc)
    ven_post("/plan/trigger").raise_for_status()

    def created_at(plan):
        # RFC 3339 with up to nanoseconds; fromisoformat takes at most microseconds.
        raw = (plan or {}).get("created_at")
        if not raw:
            return None
        head, _, frac = raw.rstrip("Z").partition(".")
        return datetime.fromisoformat(f"{head}.{(frac + '000000')[:6]}+00:00")

    def fetch_plan():
        r = ven_get("/plan")
        return r.json() if r.ok else None

    poll_until(
        fetch_plan,
        lambda plan: (created_at(plan) or cutoff) > cutoff,
        timeout=180,
        interval=2,
        description="a plan made after the overrides were reset",
    )

    def fetch():
        r = ven_get("/arbiter-diagnostics")
        return r.json() if r.ok else None

    def no_held_gap(diag):
        kw = (diag or {}).get("dev_without_correction_kw")
        return kw is not None and abs(kw) < 0.1

    try:
        poll_until(fetch, no_held_gap, timeout=120, interval=2,
                   description="no deviation the arbiter would keep if released")
    except TimeoutError:
        diag = fetch()
        raise AssertionError(
            "the site already differs from plan without any disturbance from this scenario, "
            f"so a release cannot be observed: {diag}"
        )

