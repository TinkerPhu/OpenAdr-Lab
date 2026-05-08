"""behave environment hooks — run before/after the entire test suite."""

import os
import time
import json
from datetime import datetime
from features.helpers.api_client import VTN_BASE_URL, VEN_BASE_URL, VEN2_BASE_URL, VEN_NO_PV_BASE_URL, BFF_BASE_URL
from features.helpers.wait import wait_for_url

# Timing instrumentation
TIMING_LOG = "/tmp/test-timing.jsonl"

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

    # Phase 2 — SQL cleanup.
    # Two passes:
    # a) Programs with business_id IS NULL (created via BFF/UI layer, invisible to API credentials).
    # b) Programs whose names match the ui_use_cases.feature hardcoded names — belt-and-suspenders
    #    for runs where the UI credential gave the programs a non-null business_id.
    try:
        dsn = "postgres://openadr:openadr@test-db:5432/openadr"
        sql = (
            "DELETE FROM report"
            "  WHERE program_id IN (SELECT id FROM program WHERE business_id IS NULL"
            "    OR program_name LIKE 'ui-uc%');"
            "DELETE FROM event"
            "  WHERE program_id IN (SELECT id FROM program WHERE business_id IS NULL"
            "    OR program_name LIKE 'ui-uc%');"
            "DELETE FROM ven_program"
            "  WHERE program_id IN (SELECT id FROM program WHERE business_id IS NULL"
            "    OR program_name LIKE 'ui-uc%');"
            "DELETE FROM program WHERE business_id IS NULL OR program_name LIKE 'ui-uc%';"
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
    t0 = time.time()
    _cleanup_all_programs()
    elapsed = round(time.time() - t0, 2)
    entry = {"type": "feature_cleanup", "feature": feature.name, "elapsed_s": elapsed}
    line = json.dumps(entry)
    print(f"[TIMING] {line}", flush=True)
    try:
        with open(TIMING_LOG, "a") as f:
            f.write(line + "\n")
    except Exception:
        pass


def _is_ui(scenario):
    """Check for @ui tag on scenario or its parent feature."""
    return "ui" in scenario.tags or "ui" in scenario.feature.tags


def _is_ven_ui(scenario):
    """Check for @ven-ui tag (VEN simulation UI scenarios)."""
    return "ven-ui" in scenario.tags or "ven-ui" in scenario.feature.tags


def _log_timing(scenario, steps_s, cleanup_s, start_iso=""):
    """Write one timing entry to stdout and to TIMING_LOG.

    steps_s   — time from start of before_scenario to end of last step
    cleanup_s — time for after_scenario cleanup (VTN, sim reset, browser close)
    Both values survive container exit because they are printed to stdout,
    which is captured by the external `tee` command.
    """
    tags = list(set(list(scenario.tags) + list(scenario.feature.tags)))
    entry = {
        "type": "scenario",
        "feature": scenario.feature.name,
        "scenario": scenario.name,
        "status": str(scenario.status),
        "start_iso": start_iso,
        "steps_s": steps_s,
        "cleanup_s": cleanup_s,
        "total_s": round(steps_s + cleanup_s, 2),
        "tags": tags,
    }
    line = json.dumps(entry)
    # Print to stdout so it survives the ephemeral --rm container
    print(f"[TIMING] {line}", flush=True)
    try:
        with open(TIMING_LOG, "a") as f:
            f.write(line + "\n")
    except Exception:
        pass


def before_scenario(context, scenario):
    """Record scenario start time and launch browser page for @ui and @ven-ui scenarios."""
    context._scenario_start_time = time.time()
    context._scenario_start_iso = datetime.utcnow().isoformat() + "Z"

    if _is_ui(scenario):
        if context._pw is None:
            from playwright.sync_api import sync_playwright
            context._pw = sync_playwright().start()
            context._browser = context._pw.chromium.launch(headless=True)
        context.browser_page = context._browser.new_page()
        # Capture browser console errors and network failures for diagnosis
        context.browser_page.on(
            "console",
            lambda msg: print(f"[BROWSER:{msg.type}] {msg.text}") if msg.type in ("error", "warning") else None,
        )
        context.browser_page.on(
            "pageerror",
            lambda exc: print(f"[PAGE ERROR] {exc}"),
        )
        context.browser_page.on(
            "requestfailed",
            lambda req: print(f"[REQUEST FAILED] {req.method} {req.url} — {req.failure}"),
        )
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


def _reset_device_sessions():
    """Clear all device sessions on VEN-1 between scenarios.

    Prevents ev-sessions, heater-targets, shiftable-loads, and user-requests
    posted in one scenario from leaking into subsequent scenarios.
    """
    try:
        from features.helpers.api_client import ven_get, ven_delete
        ven_delete("/ev-session")
        ven_delete("/heater-target")
        r = ven_get("/shiftable-loads")
        if r.ok:
            for load in (r.json() or []):
                ven_delete(f"/shiftable-loads/{load['id']}")
        r = ven_get("/user-requests")
        if r.ok:
            for req in (r.json() or []):
                ven_delete(f"/user-requests/{req['id']}")
    except Exception:
        pass


def after_scenario(context, scenario):
    """Record scenario timing, close browser page, and clean up."""
    # Capture scenario-steps time (before cleanup)
    steps_end = time.time()
    steps_s = round(steps_end - context._scenario_start_time, 2) if hasattr(context, '_scenario_start_time') else 0.0

    start_iso = context._scenario_start_iso if hasattr(context, '_scenario_start_iso') else ""
    if scenario.status == 'skipped':
        _log_timing(scenario, steps_s, 0.0, start_iso)
        return

    import features.helpers.api_client as api_client
    api_client.VEN_BASE_URL = api_client._DEFAULT_VEN_BASE_URL
    _cleanup_vtn_resources(context)
    _reset_ven_sim_overrides()
    _reset_device_sessions()

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

    # Capture cleanup time separately
    cleanup_s = round(time.time() - steps_end, 2)
    _log_timing(scenario, steps_s, cleanup_s, start_iso)


def after_all(context):
    """Print timing summary and shut down Playwright."""
    try:
        timings = []
        cleanups = []
        if os.path.exists(TIMING_LOG):
            with open(TIMING_LOG, "r") as f:
                for line in f:
                    try:
                        entry = json.loads(line)
                        if entry.get("type") == "scenario":
                            timings.append(entry)
                        elif entry.get("type") == "feature_cleanup":
                            cleanups.append(entry)
                    except json.JSONDecodeError:
                        pass

        timings.sort(key=lambda x: x.get("total_s", 0), reverse=True)

        print("\n" + "=" * 100)
        print("TEST TIMING SUMMARY — ALL SCENARIOS (slowest first)")
        print("=" * 100)
        print(f"{'#':>3}  {'steps_s':>8}  {'cleanup_s':>9}  {'total_s':>8}  {'st':>2}  {'feature :: scenario'}")
        print("-" * 100)
        for i, t in enumerate(timings, 1):
            st = "✓" if t['status'] == 'passed' else ("✗" if t['status'] == 'failed' else "⊘")
            tags_str = " [" + ",".join(t['tags']) + "]" if t['tags'] else ""
            label = f"{t['feature']} :: {t['scenario']}{tags_str}"
            print(f"{i:>3}  {t['steps_s']:>8.1f}  {t['cleanup_s']:>9.1f}  {t['total_s']:>8.1f}  {st:>2}  {label}")

        if timings:
            total_steps = sum(t.get("steps_s", 0) for t in timings)
            total_cleanup = sum(t.get("cleanup_s", 0) for t in timings)
            total_feature_cleanup = sum(c.get("elapsed_s", 0) for c in cleanups)
            total_accounted = total_steps + total_cleanup + total_feature_cleanup
            avg_total = (total_steps + total_cleanup) / len(timings)
            print("-" * 100)
            print(f"\nSCENARIO STEPS:         {total_steps:7.1f}s")
            print(f"AFTER-SCENARIO CLEANUP: {total_cleanup:7.1f}s")
            print(f"BEFORE-FEATURE CLEANUP: {total_feature_cleanup:7.1f}s  (across {len(cleanups)} features)")
            print(f"TOTAL ACCOUNTED:        {total_accounted:7.1f}s")
            print(f"Scenarios: {len(timings)} | Avg total per scenario: {avg_total:.1f}s")
    except Exception as e:
        print(f"Warning: failed to print timing summary: {e}")

    # Shut down Playwright
    if context._browser:
        context._browser.close()
    if context._pw:
        context._pw.stop()
