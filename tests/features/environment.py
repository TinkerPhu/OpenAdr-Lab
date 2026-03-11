"""behave environment hooks — run before/after the entire test suite."""

import os
from features.helpers.api_client import VTN_BASE_URL, VEN_BASE_URL, VEN2_BASE_URL, BFF_BASE_URL
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


def before_all(context):
    """Wait for all services to be reachable before running any tests."""
    _check_not_live()
    print(f"Waiting for VTN at {VTN_BASE_URL} ...")
    wait_for_url(f"{VTN_BASE_URL}/health", timeout=120)

    print(f"Waiting for VEN-1 at {VEN_BASE_URL} ...")
    wait_for_url(f"{VEN_BASE_URL}/health", timeout=120)

    print(f"Waiting for VEN-2 at {VEN2_BASE_URL} ...")
    wait_for_url(f"{VEN2_BASE_URL}/health", timeout=120)

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
        from features.helpers.ui import VenUi
        context.ven_ui = VenUi(context.browser_page)
        from features.helpers.api_client import get_token_value
        context.vtn_token = get_token_value("any-business", "any-business")


def _cleanup_vtn_events(context):
    """Delete all VTN events created during this scenario.

    Prevents capacity-limit events (and other persistent events) from leaking
    across scenarios and poisoning parse_capacity_state's global-minimum logic.
    """
    try:
        from features.helpers.api_client import vtn_delete, get_token_value
        token = get_token_value("any-business", "any-business")

        ids: set = set()
        for attr in ("rate_event_id", "planner_event_id"):
            val = getattr(context, attr, None)
            if val:
                ids.add(val)
        created = getattr(context, "created_event", None)
        if isinstance(created, dict) and created.get("id"):
            ids.add(created["id"])
        for evt in getattr(context, "uc_events", {}).values():
            if isinstance(evt, dict) and evt.get("id"):
                ids.add(evt["id"])

        for eid in ids:
            try:
                vtn_delete(f"/events/{eid}", token)
            except Exception:
                pass
    except Exception as exc:
        print(f"Warning: event cleanup failed: {exc}")


def after_scenario(context, scenario):
    """Close browser page after @ui/@ven-ui scenarios; restart stopped services."""
    _cleanup_vtn_events(context)

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
