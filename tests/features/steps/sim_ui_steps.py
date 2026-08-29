"""Step definitions for VEN simulation UI override scenarios."""

from behave import given
from features.helpers.api_client import ven_get, ven_post


# ── Helpers ───────────────────────────────────────────────────────────────────

def _ven1_trace():
    r = ven_get("/trace/events?limit=10")
    r.raise_for_status()
    return r.json()


# ── Step definitions ──────────────────────────────────────────────────────────

@given('the VEN-1 sim overrides are reset')
def step_reset_ven_overrides(context):
    """Release all active sim injects on VEN-1 to clear any persisted override values.

    Without this reset, environmental overrides set in scenario N bleed into
    scenario N+1, causing unexpected sim state in subsequent scenarios.
    """
    r = ven_post("/sim/inject/reset", json={})
    r.raise_for_status()
