"""behave environment hooks — run before/after the entire test suite."""

import os
from features.helpers.api_client import VTN_BASE_URL, VEN_BASE_URL, VEN2_BASE_URL, BFF_BASE_URL
from features.helpers.wait import wait_for_url

UI_BASE_URL = os.environ.get("UI_BASE_URL", "http://test-ui:80")


def before_all(context):
    """Wait for all services to be reachable before running any tests."""
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

    print("All services healthy — starting tests.")

    # Playwright browser — started once, shared across all @ui scenarios
    context._pw = None
    context._browser = None


def _is_ui(scenario):
    """Check for @ui tag on scenario or its parent feature."""
    return "ui" in scenario.tags or "ui" in scenario.feature.tags


def before_scenario(context, scenario):
    """Launch browser page for @ui scenarios."""
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


def after_scenario(context, scenario):
    """Close browser page after @ui scenarios; restart stopped services."""
    if _is_ui(scenario) and hasattr(context, "browser_page"):
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
