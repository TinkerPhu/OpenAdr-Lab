"""Step definitions for VEN Controller V2 Dashboard UI scenarios."""

from behave import given, when, then
from features.helpers.api_client import ven_get, ven_post
from features.helpers.ui import tid
from features.helpers.wait import poll_until


# ── Given ─────────────────────────────────────────────────────────────────────

@given("I open the VEN-1 controller V2 UI")
def step_open_controller_v2(context):
    context.ven_ui.open()
    context.ven_ui.go_controller()


# ── Layout: 01_layout.feature ─────────────────────────────────────────────────

@then("the grid tariff cell is visible")
def step_grid_tariff_visible(context):
    el = context.browser_page.wait_for_selector(
        tid("grid-tariff-cell"), timeout=45000
    )
    assert el is not None and el.is_visible(), "grid-tariff-cell not visible"


@then("the grid accumulated cell is visible")
def step_grid_accumulated_visible(context):
    el = context.browser_page.wait_for_selector(
        tid("grid-accumulated-cell"), timeout=45000
    )
    assert el is not None and el.is_visible(), "grid-accumulated-cell not visible"


@then("the grid cells appear above the asset cells")
def step_grid_above_asset(context):
    tariff = context.browser_page.query_selector(tid("grid-tariff-cell"))
    accumulated = context.browser_page.query_selector(tid("grid-accumulated-cell"))
    # Find any asset cell
    asset_cells = context.browser_page.query_selector_all("[data-testid^='asset-cell-']")
    assert tariff is not None, "grid-tariff-cell not found"
    assert accumulated is not None, "grid-accumulated-cell not found"
    assert len(asset_cells) > 0, "No asset cells found"

    tariff_box = tariff.bounding_box()
    accumulated_box = accumulated.bounding_box()
    first_asset_box = asset_cells[0].bounding_box()

    # Both grid cells must have a smaller top-Y than the first asset cell
    assert tariff_box["y"] < first_asset_box["y"], (
        f"grid-tariff-cell (y={tariff_box['y']}) is not above first asset cell (y={first_asset_box['y']})"
    )
    assert accumulated_box["y"] < first_asset_box["y"], (
        f"grid-accumulated-cell (y={accumulated_box['y']}) is not above first asset cell (y={first_asset_box['y']})"
    )


@then("the controller V2 scrollable content area is visible")
def step_scrollable_content_visible(context):
    el = context.browser_page.wait_for_selector(
        tid("scrollable-content"), timeout=45000
    )
    assert el is not None and el.is_visible(), "scrollable-content not visible"


@then("the grid tariff cell is not sticky by default")
def step_grid_tariff_not_sticky(context):
    # Verify that without pinning, the grid tariff cell is NOT inside the pinned-zone
    pinned_zone = context.browser_page.query_selector(tid("pinned-zone"))
    if pinned_zone is None:
        return  # No pinned zone rendered = cells are not pinned
    # Check if grid-tariff-cell is inside the pinned zone
    tariff_in_pinned = pinned_zone.query_selector(tid("grid-tariff-cell"))
    assert tariff_in_pinned is None, (
        "grid-tariff-cell found inside pinned-zone but should not be pinned by default"
    )


@then("at least one asset cell is visible")
def step_at_least_one_asset_cell(context):
    # Wait for at least one asset cell to appear (React Query fetch may be async)
    context.browser_page.wait_for_selector(
        "[data-testid^='asset-cell-']", timeout=45000
    )
    cells = context.browser_page.query_selector_all("[data-testid^='asset-cell-']")
    assert len(cells) > 0, "No asset cells found on controller V2 page"
    assert cells[0].is_visible(), "First asset cell is not visible"


# ── Asset cells: 02_asset_cells.feature ───────────────────────────────────────

@then("the EV asset cell shows a power value")
def step_ev_power_visible(context):
    el = context.browser_page.wait_for_selector(
        tid("asset-power-ev"), timeout=45000
    )
    assert el is not None and el.is_visible(), "asset-power-ev not visible"
    text = el.inner_text().strip()
    assert text, "asset-power-ev has no text content"


@then("the EV asset cell shows a cost rate value")
def step_ev_cost_rate_visible(context):
    el = context.browser_page.wait_for_selector(
        tid("asset-cost-rate-ev"), timeout=45000
    )
    assert el is not None and el.is_visible(), "asset-cost-rate-ev not visible"


@then("the EV asset cell shows a CO2eq rate value")
def step_ev_co2_rate_visible(context):
    el = context.browser_page.wait_for_selector(
        tid("asset-co2-rate-ev"), timeout=45000
    )
    assert el is not None and el.is_visible(), "asset-co2-rate-ev not visible"


@then("the EV asset timeline chart is visible")
def step_ev_timeline_visible(context):
    el = context.browser_page.wait_for_selector(
        tid("asset-timeline-chart-ev"), timeout=45000
    )
    assert el is not None and el.is_visible(), "asset-timeline-chart-ev not visible"


@then("the NOW reference line is visible on the EV timeline chart")
def step_now_line_visible(context):
    # recharts ReferenceLine renders as a <line> inside the chart SVG
    chart = context.browser_page.wait_for_selector(
        tid("asset-timeline-chart-ev"), timeout=45000
    )
    assert chart is not None, "asset-timeline-chart-ev not found"
    # Wait for the recharts reference line element (has class recharts-reference-line).
    # Uses wait_for_selector (not query_selector) to account for async recharts rendering.
    ref_line = chart.wait_for_selector(".recharts-reference-line", timeout=45000)
    assert ref_line is not None, "No recharts-reference-line found inside asset-timeline-chart-ev"


@then("the battery asset cell shows a SoC value")
def step_battery_soc_visible(context):
    el = context.browser_page.wait_for_selector(
        tid("asset-soc-battery"), timeout=45000
    )
    assert el is not None and el.is_visible(), "asset-soc-battery not visible"


@then("the battery asset timeline chart is visible")
def step_battery_timeline_visible(context):
    el = context.browser_page.wait_for_selector(
        tid("asset-timeline-chart-battery"), timeout=45000
    )
    assert el is not None and el.is_visible(), "asset-timeline-chart-battery not visible"


@then("the base_load asset timeline chart is visible")
def step_base_load_timeline_visible(context):
    el = context.browser_page.wait_for_selector(
        tid("asset-timeline-chart-base_load"), timeout=45000
    )
    assert el is not None and el.is_visible(), "asset-timeline-chart-base_load not visible"


@then("the global time range extend button is visible")
def step_global_extend_btn_visible(context):
    el = context.browser_page.wait_for_selector(
        tid("global-time-range-extend-btn"), timeout=45000
    )
    assert el is not None and el.is_visible(), "global-time-range-extend-btn not visible"


# ── Simulation controls: 03_simulation_controls.feature ───────────────────────


def _expand_ev_right_section(page):
    """Expand the right section if it is collapsed (collapsed by default)."""
    btn = page.wait_for_selector(
        tid("asset-cell-ev-collapse-right"), timeout=45000
    )
    label = btn.get_attribute("aria-label") or ""
    if "Expand" in label:
        page.click(tid("asset-cell-ev-collapse-right"))
        # Wait for right section content to appear
        page.wait_for_selector(
            tid("asset-cell-ev-right"), state="visible", timeout=45000,
        )


def _expand_ev_status_accordion(page):
    """Expand the right section (if collapsed) so controls are visible.
    The controller UI renders controls directly in the right section without an accordion.
    """
    _expand_ev_right_section(page)
    # Controls are directly visible in the right section — no accordion to expand.
    page.wait_for_selector(tid("right-section-ev"), state="visible", timeout=45000)


@then("the EV plugged toggle is visible in the EV cell right section")
def step_ev_plugged_toggle_visible(context):
    _expand_ev_status_accordion(context.browser_page)
    # Quick diagnostic: check if any ctrl-* element exists (schema loaded?)
    try:
        context.browser_page.wait_for_selector('[data-testid^="ctrl-"]', timeout=5000)
    except Exception:
        html = context.browser_page.evaluate(
            '() => document.querySelector(\'[data-testid="right-section-ev"]\')?.outerHTML ?? "NOT FOUND"'
        )
        print(f"\n[DIAG] No ctrl-* elements within 5s. right-section-ev: {html[:600]}")
    el = context.browser_page.wait_for_selector(
        tid("ctrl-ev-plugged"), state="visible", timeout=60000
    )
    assert el is not None and el.is_visible(), "ctrl-ev-plugged toggle not visible"


@then("the EV SoC slider is visible in the EV cell right section")
def step_ev_soc_slider_visible(context):
    _expand_ev_status_accordion(context.browser_page)
    el = context.browser_page.wait_for_selector(
        tid("ctrl-ev-soc"), timeout=45000
    )
    assert el is not None and el.is_visible(), "ctrl-ev-soc slider not visible"


@when("I toggle the EV plugged switch in the controller V2 EV cell")
def step_toggle_ev_plugged(context):
    _expand_ev_status_accordion(context.browser_page)
    # Read current state before toggle; None means "not set" → displays as True (checked)
    r = ven_get("/sim/inject")
    r.raise_for_status()
    overrides = r.json()
    ev_plugged_raw = overrides.get("ev_plugged")
    context.ev_plugged_before = True if ev_plugged_raw is None else ev_plugged_raw

    # MUI Switch renders data-testid on a <span>; click the inner <input> to reliably toggle
    checkbox_sel = f'{tid("ctrl-ev-plugged")} input[type="checkbox"]'
    context.browser_page.wait_for_selector(checkbox_sel, timeout=45000)
    context.browser_page.click(checkbox_sel)


@then("the EV plugged state changes in VEN-1 sim override")
def step_ev_plugged_state_changed(context):
    expected = not context.ev_plugged_before

    def get_plugged():
        r = ven_get("/sim/inject")
        r.raise_for_status()
        v = r.json().get("ev_plugged")
        return True if v is None else v

    poll_until(
        get_plugged,
        lambda v: v == expected,
        timeout=30,
        interval=1,
        description=f"ev_plugged changes to {expected}",
    )


# ── Navigation: 04_navigation.feature ─────────────────────────────────────────

@then("the EV asset cell has a pin button")
def step_ev_has_pin_button(context):
    el = context.browser_page.wait_for_selector(
        tid("asset-cell-ev-pin-btn"), timeout=45000
    )
    assert el is not None and el.is_visible(), "asset-cell-ev-pin-btn not visible"


@then("the grid tariff cell has a pin button")
def step_grid_tariff_has_pin_button(context):
    el = context.browser_page.wait_for_selector(
        tid("grid-tariff-cell-pin-btn"), timeout=45000
    )
    assert el is not None and el.is_visible(), "grid-tariff-cell-pin-btn not visible"


@when("I click the pin button on the EV asset cell")
def step_pin_ev_cell(context):
    context.browser_page.wait_for_selector(
        tid("asset-cell-ev-pin-btn"), timeout=45000
    )
    context.browser_page.dispatch_event(tid("asset-cell-ev-pin-btn"), "click")


@when("I click the pin button on the EV asset cell again")
def step_unpin_ev_cell(context):
    context.browser_page.wait_for_selector(
        tid("asset-cell-ev-pin-btn"), timeout=45000
    )
    context.browser_page.dispatch_event(tid("asset-cell-ev-pin-btn"), "click")


@then("the EV asset cell is visible in the pinned zone")
def step_ev_cell_in_pinned_zone(context):
    pinned_zone = context.browser_page.wait_for_selector(
        tid("pinned-zone"), timeout=45000
    )
    assert pinned_zone is not None and pinned_zone.is_visible(), "pinned-zone not visible"
    ev_in_pinned = pinned_zone.query_selector(tid("asset-cell-ev"))
    assert ev_in_pinned is not None, "asset-cell-ev not found inside pinned-zone"


@then("the EV asset cell is not in the pinned zone")
def step_ev_cell_not_in_pinned_zone(context):
    pinned_zone = context.browser_page.query_selector(tid("pinned-zone"))
    if pinned_zone is None:
        return  # No pinned zone = nothing is pinned = pass
    ev_in_pinned = pinned_zone.query_selector(tid("asset-cell-ev"))
    assert ev_in_pinned is None, "asset-cell-ev is still inside pinned-zone after unpin"


@when("I click the collapse right button on the EV asset cell")
def step_collapse_right_ev(context):
    context.browser_page.wait_for_selector(
        tid("asset-cell-ev-collapse-right"), timeout=45000
    )
    context.browser_page.click(tid("asset-cell-ev-collapse-right"))


@then("the EV asset cell right section is visible")
def step_ev_right_visible(context):
    right = context.browser_page.wait_for_selector(
        tid("asset-cell-ev-right"), state="visible", timeout=45000
    )
    assert right is not None and right.is_visible(), (
        "asset-cell-ev-right is not visible after expand"
    )


@then("the EV asset cell right section is not visible")
def step_ev_right_not_visible(context):
    right = context.browser_page.query_selector(tid("asset-cell-ev-right"))
    assert right is None or not right.is_visible(), (
        "asset-cell-ev-right is still visible after collapse"
    )
