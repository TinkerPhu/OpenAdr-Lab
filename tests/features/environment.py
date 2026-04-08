"""behave environment hooks — run before/after the entire test suite."""

import os
from features.helpers.api_client import VTN_BASE_URL, VEN_BASE_URL, VEN2_BASE_URL, VEN_NO_PV_BASE_URL, BFF_BASE_URL
from features.helpers.wait import wait_for_url

UI_BASE_URL = os.environ.get("UI_BASE_URL", "http://test-ui:80")
VEN_UI_BASE_URL = os.environ.get("VEN_UI_BASE_URL", "http://test-ven-ui:80")


def _check_not_live():
    """Abort if URLs point at live instances (not test-* or localhost)."""
    from urllib.parse import urlparse
    safe_hosts = {"localhost", "127.0.0.1"}
    urls = {
        "VTN_BASE_URL": VTN_BASE_URL,
        "VEN_BASE_URL": VEN_BASE_URL,
        "VEN2_BASE_URL": VEN2_BASE_URL,
        "VEN_NO_PV_BASE_URL": VEN_NO_PV_BASE_URL,
        "BFF_BASE_URL": BFF_BASE_URL,
    }
    for name, url in urls.items():
        host = urlparse(url).hostname or ""
        if host not in safe_hosts and not host.startswith("test-"):
            if os.environ.get("ALLOW_LIVE_TESTS", "").lower() not in ("1", "true", "yes"):
                raise RuntimeError(
                    f"SAFETY: {name}={url} points at live host '{host}'. "
                    f"Tests should run against the test Docker compose stack (test-* hostnames). "
                    f"Set ALLOW_LIVE_TESTS=true to override."
                )


def _cleanup_all_programs():
    """Delete every program in the test VTN before each feature.

    Two-phase cleanup:
    1. API phase: delete programs the `any-business` credential can see
       (programs with a matching business_id).
    2. SQL phase: delete orphaned programs with business_id IS NULL — these
       are created via the BFF/UI layer and are invisible to the API credential,
       so they accumulate across runs and cause 409 Conflict errors.
       Only NULL-business_id rows are removed; API-created programs (and their
       ven_program enrollment records) are left intact.
    """
    import subprocess

    # Phase 1 — API cleanup (programs visible to any-business credential).
    try:
        from features.helpers.api_client import vtn_get, vtn_delete, get_token_value
        token = get_token_value("any-business", "any-business")
        skip = 0
        limit = 50
        deleted = 0
        while True:
            r = vtn_get(f"/programs?limit={limit}&skip={skip}", token)
            if not r.ok:
                break
            programs = r.json()
            if not programs:
                break
            for p in programs:
                try:
                    vtn_delete(f"/programs/{p['id']}", token)
                    deleted += 1
                except Exception:
                    pass
            if len(programs) < limit:
                break
            skip += limit
        if deleted:
            print(f"Pre-run cleanup: deleted {deleted} programs (API) from test VTN.")
    except Exception as exc:
        print(f"Warning: API program cleanup failed: {exc}")

    # Phase 2 — SQL cleanup (orphaned programs with business_id IS NULL).
    # Only removes programs/events/enrollments that have no owning business.
    # Enrollment records (ven_program) for API-created programs are unaffected.
    try:
        dsn = "postgres://openadr:openadr@test-db:5432/openadr"
        sql = (
            "DELETE FROM report"
            "  WHERE program_id IN (SELECT id FROM program WHERE business_id IS NULL);"
            "DELETE FROM event"
            "  WHERE program_id IN (SELECT id FROM program WHERE business_id IS NULL);"
            "DELETE FROM ven_program"
            "  WHERE program_id IN (SELECT id FROM program WHERE business_id IS NULL);"
            "DELETE FROM program WHERE business_id IS NULL;"
        )
        result = subprocess.run(
            ["psql", dsn, "-c", sql],
            capture_output=True, text=True, timeout=15,
        )
        if result.returncode != 0 and result.stderr:
            print(f"SQL cleanup warning: {result.stderr[:200]}")
    except Exception as exc:
        print(f"Warning: SQL fallback cleanup failed: {exc}")



def before_all(context):
    """Wait for all services to be reachable before running any tests."""
    _check_not_live()
    print(f"Waiting for VTN at {VTN_BASE_URL} ...")
    wait_for_url(f"{VTN_BASE_URL}/health", timeout=120)

    print(f"Waiting for VEN-1 at {VEN_BASE_URL} ...")
    wait_for_url(f"{VEN_BASE_URL}/health", timeout=120)

    print(f"Waiting for VEN-2 at {VEN2_BASE_URL} ...")
    wait_for_url(f"{VEN2_BASE_URL}/health", timeout=120)

    print(f"Waiting for VEN-no-pv at {VEN_NO_PV_BASE_URL} ...")
    wait_for_url(f"{VEN_NO_PV_BASE_URL}/health", timeout=120)

    print(f"Waiting for BFF at {BFF_BASE_URL} ...")
    wait_for_url(f"{BFF_BASE_URL}/api/health", timeout=120)

    if os.environ.get("UI_BASE_URL"):
        print(f"Waiting for UI at {UI_BASE_URL} ...")
        wait_for_url(UI_BASE_URL, timeout=120)

    if os.environ.get("VEN_UI_BASE_URL"):
        print(f"Waiting for VEN UI at {VEN_UI_BASE_URL} ...")
        wait_for_url(VEN_UI_BASE_URL, timeout=120)

    print("All services healthy — starting tests.")

    # Playwright browser — started once, shared across all @ui scenarios
    context._pw = None
    context._browser = None


def before_feature(context, feature):
    """Wipe all programs (cascading to events) before each feature.

    Running cleanup per-feature rather than once at before_all means:
    - Each feature starts with an empty program list, preventing 409 conflicts
      from hardcoded names used by earlier features in the same run.
    - Deletions are small (a handful from the previous feature), so VTN is
      never hit with a 100-item bulk delete that could cause BFF 502 errors.
    """
    _cleanup_all_programs()


def _is_ui(scenario):
    """Check for @ui tag on scenario or its parent feature."""
    return "ui" in scenario.tags or "ui" in scenario.feature.tags


def _is_ven_ui(scenario):
    """Check for @ven-ui tag (VEN simulation UI scenarios)."""
    return "ven-ui" in scenario.tags or "ven-ui" in scenario.feature.tags


def before_scenario(context, scenario):
    """Launch browser page for @ui and @ven-ui scenarios."""
    if _is_ui(scenario):
        if context._pw is None:
            from playwright.sync_api import sync_playwright
            context._pw = sync_playwright().start()
            context._browser = context._pw.chromium.launch(headless=True)
        context.browser_page = context._browser.new_page()
        from features.helpers.ui import VtnUi
        context.ui = VtnUi(context.browser_page)
        context.ui.open()
        # UI scenarios reuse API verification steps that need a VTN token
        from features.helpers.api_client import get_token_value
        context.vtn_token = get_token_value("any-business", "any-business")

    if _is_ven_ui(scenario):
        if context._pw is None:
            from playwright.sync_api import sync_playwright
            context._pw = sync_playwright().start()
            context._browser = context._pw.chromium.launch(headless=True)
        context.browser_page = context._browser.new_page()
        # Capture JavaScript console errors and page-level exceptions for debugging
        context.browser_page.on(
            "console",
            lambda msg: print(f"[BROWSER:{msg.type}] {msg.text}") if msg.type in ("error", "warning") else None,
        )
        context.browser_page.on(
            "pageerror",
            lambda exc: print(f"[PAGE ERROR] {exc}"),
        )
        from features.helpers.ui import VenUi
        context.ven_ui = VenUi(context.browser_page)
        from features.helpers.api_client import get_token_value
        context.vtn_token = get_token_value("any-business", "any-business")


def _cleanup_vtn_resources(context):
    """Delete all VTN events and programs created during this scenario.

    Prevents capacity-limit events (and other persistent events) from leaking
    across scenarios and poisoning parse_capacity_state's global-minimum logic.
    Also deletes programs so UI scenarios can re-run without 409 conflicts.
    """
    try:
        from features.helpers.api_client import vtn_delete, get_token_value
        token = get_token_value("any-business", "any-business")

        event_ids: set = set()
        for attr in ("rate_event_id", "planner_event_id"):
            val = getattr(context, attr, None)
            if val:
                event_ids.add(val)
        created = getattr(context, "created_event", None)
        if isinstance(created, dict) and created.get("id"):
            event_ids.add(created["id"])
        for evt in getattr(context, "uc_events", {}).values():
            if isinstance(evt, dict) and evt.get("id"):
                event_ids.add(evt["id"])

        for eid in event_ids:
            try:
                vtn_delete(f"/events/{eid}", token)
            except Exception:
                pass

    except Exception as exc:
        print(f"Warning: event cleanup failed: {exc}")


def _reset_ven_sim_overrides():
    """Reset all sim injects on VEN-1 to clear any override bleed.

    Prevents overrides set in one scenario (e.g. ev_plugged=false) from
    leaking into subsequent scenarios that don't belong to the same feature.
    """
    try:
        from features.helpers.api_client import ven_post
        ven_post("/sim/inject/reset", json={})
    except Exception:
        pass


def _reset_ven_packets():
    """Drop all non-terminal EV packets on VEN-1 between scenarios.

    Prevents packets posted in one scenario (e.g. plan_reasons FIRM_OBLIGATION
    test) from leaking into subsequent scenarios and polluting the planner state.
    """
    try:
        from features.helpers.api_client import ven_delete
        ven_delete("/packets")
    except Exception:
        pass


def after_scenario(context, scenario):
    """Close browser page after @ui/@ven-ui scenarios; restart stopped services."""
    import features.helpers.api_client as api_client
    api_client.VEN_BASE_URL = api_client._DEFAULT_VEN_BASE_URL
    _cleanup_vtn_resources(context)
    _reset_ven_sim_overrides()
    _reset_ven_packets()

    if (_is_ui(scenario) or _is_ven_ui(scenario)) and hasattr(context, "browser_page"):
        context.browser_page.close()

    # Resilience cleanup: restart any services stopped during the scenario
    try:
        stopped = context._stopped_services
    except (AttributeError, KeyError):
        stopped = None
    if stopped:
        from features.helpers import docker_ctl
        for svc in stopped:
            try:
                docker_ctl.start_service(svc)
            except Exception as exc:
                print(f"Warning: failed to restart {svc}: {exc}")
        context._stopped_services = []
        # Wait for VTN to be healthy before next scenario
        from features.helpers.wait import wait_for_url
        wait_for_url(f"{VTN_BASE_URL}/health", timeout=60)


def after_all(context):
    """Shut down Playwright."""
    if context._browser:
        context._browser.close()
    if context._pw:
        context._pw.stop()
