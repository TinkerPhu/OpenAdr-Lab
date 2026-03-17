"""Step definitions for VEN Controller V2 Dashboard UI scenarios."""

from behave import given, when, then
from features.helpers.api_client import ven_get, ven_post
from features.helpers.ui import tid
from features.helpers.wait import poll_until


# ── Given ─────────────────────────────────────────────────────────────────────

@given("I open the VEN-1 controller V2 UI")
def step_open_controller_v2(context):
    context.ven_ui.open()
    context.ven_ui.go_controller_v2()


# ── Layout: 01_layout.feature ─────────────────────────────────────────────────

@then("the grid tariff cell is visible")
def step_grid_tariff_visible(context):
    el = context.browser_page.wait_for_selector(
        tid("grid-tariff-cell"), timeout=10000
    )
    assert el is not None and el.is_visible(), "grid-tariff-cell not visible"


@then("the grid accumulated cell is visible")
def step_grid_accumulated_visible(context):
    el = context.browser_page.wait_for_selector(
        tid("grid-accumulated-cell"), timeout=10000
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
        tid("scrollable-content"), timeout=10000
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
    cells = context.browser_page.query_selector_all("[data-testid^='asset-cell-']")
    assert len(cells) > 0, "No asset cells found on controller V2 page"
    assert cells[0].is_visible(), "First asset cell is not visible"


# ── Asset cells: 02_asset_cells.feature ───────────────────────────────────────

@then("the EV asset cell shows a power value")
def step_ev_power_visible(context):
    el = context.browser_page.wait_for_selector(
        tid("asset-power-ev"), timeout=10000
    )
    assert el is not None and el.is_visible(), "asset-power-ev not visible"
    text = el.inner_text().strip()
    assert text, "asset-power-ev has no text content"


@then("the EV asset cell shows a cost rate value")
def step_ev_cost_rate_visible(context):
    el = context.browser_page.wait_for_selector(
        tid("asset-cost-rate-ev"), timeout=10000
    )
    assert el is not None and el.is_visible(), "asset-cost-rate-ev not visible"


@then("the EV asset cell shows a CO2eq rate value")
def step_ev_co2_rate_visible(context):
    el = context.browser_page.wait_for_selector(
        tid("asset-co2-rate-ev"), timeout=10000
    )
    assert el is not None and el.is_visible(), "asset-co2-rate-ev not visible"


@then("the EV asset timeline chart is visible")
def step_ev_timeline_visible(context):
    el = context.browser_page.wait_for_selector(
        tid("asset-timeline-chart-ev"), timeout=10000
    )
    assert el is not None and el.is_visible(), "asset-timeline-chart-ev not visible"


@then("the NOW reference line is visible on the EV timeline chart")
def step_now_line_visible(context):
    # recharts ReferenceLine renders as a <line> inside the chart SVG
    chart = context.browser_page.wait_for_selector(
        tid("asset-timeline-chart-ev"), timeout=10000
    )
    assert chart is not None, "asset-timeline-chart-ev not found"
    # Wait for the recharts reference line element (has class recharts-reference-line).
    # Uses wait_for_selector (not query_selector) to account for async recharts rendering.
    ref_line = chart.wait_for_selector(".recharts-reference-line", timeout=5000)
    assert ref_line is not None, "No recharts-reference-line found inside asset-timeline-chart-ev"


@then("the battery asset cell shows a SoC value")
def step_battery_soc_visible(context):
    el = context.browser_page.wait_for_selector(
        tid("asset-soc-battery"), timeout=10000
    )
    assert el is not None and el.is_visible(), "asset-soc-battery not visible"


@then("the battery asset timeline chart is visible")
def step_battery_timeline_visible(context):
    el = context.browser_page.wait_for_selector(
        tid("asset-timeline-chart-battery"), timeout=10000
    )
    assert el is not None and el.is_visible(), "asset-timeline-chart-battery not visible"


@then("the base_load asset timeline chart is visible")
def step_base_load_timeline_visible(context):
    el = context.browser_page.wait_for_selector(
        tid("asset-timeline-chart-base_load"), timeout=10000
    )
    assert el is not None and el.is_visible(), "asset-timeline-chart-base_load not visible"


@then("the EV asset cell shows an extend-window button")
def step_ev_extend_btn_visible(context):
    el = context.browser_page.wait_for_selector(
        tid("asset-cell-ev-extend-btn"), timeout=10000
    )
    assert el is not None and el.is_visible(), "asset-cell-ev-extend-btn not visible"


@then("the heater asset cell has no extend-window button")
def step_heater_no_extend_btn(context):
    el = context.browser_page.query_selector(tid("asset-cell-heater-extend-btn"))
    assert el is None, "asset-cell-heater-extend-btn should not be present but was found"


@then("the grid tariff cell shows an extend-window button")
def step_tariff_extend_btn_visible(context):
    el = context.browser_page.wait_for_selector(
        tid("grid-tariff-cell-extend-btn"), timeout=10000
    )
    assert el is not None and el.is_visible(), "grid-tariff-cell-extend-btn not visible"


# ── Simulation controls: 03_simulation_controls.feature ───────────────────────

@then("the EV plugged toggle is visible in the EV cell right section")
def step_ev_plugged_toggle_visible(context):
    el = context.browser_page.wait_for_selector(
        tid("ctrl-ev-plugged"), timeout=10000
    )
    assert el is not None and el.is_visible(), "ctrl-ev-plugged toggle not visible"


@then("the EV SoC slider is visible in the EV cell right section")
def step_ev_soc_slider_visible(context):
    el = context.browser_page.wait_for_selector(
        tid("ctrl-ev-soc"), timeout=10000
    )
    assert el is not None and el.is_visible(), "ctrl-ev-soc slider not visible"


@when("I toggle the EV plugged switch in the controller V2 EV cell")
def step_toggle_ev_plugged(context):
    # Read current state before toggle; None means "not set" → displays as True (checked)
    r = ven_get("/sim/override")
    r.raise_for_status()
    overrides = r.json()
    ev_plugged_raw = overrides.get("ev_plugged")
    context.ev_plugged_before = True if ev_plugged_raw is None else ev_plugged_raw

    # MUI Switch renders data-testid on a <span>; click the inner <input> to reliably toggle
    checkbox = context.browser_page.wait_for_selector(
        f'{tid("ctrl-ev-plugged")} input[type="checkbox"]', timeout=10000
    )
    checkbox.click()


@then("the EV plugged state changes in VEN-1 sim override")
def step_ev_plugged_state_changed(context):
    expected = not context.ev_plugged_before

    def get_plugged():
        r = ven_get("/sim/override")
        r.raise_for_status()
        v = r.json().get("ev_plugged")
        return True if v is None else v

    poll_until(
        get_plugged,
        lambda v: v == expected,
        timeout=15,
        interval=1,
        description=f"ev_plugged changes to {expected}",
    )


# ── Navigation: 04_navigation.feature ─────────────────────────────────────────

@then("the EV asset cell has a pin button")
def step_ev_has_pin_button(context):
    el = context.browser_page.wait_for_selector(
        tid("asset-cell-ev-pin-btn"), timeout=10000
    )
    assert el is not None and el.is_visible(), "asset-cell-ev-pin-btn not visible"


@then("the grid tariff cell has a pin button")
def step_grid_tariff_has_pin_button(context):
    el = context.browser_page.wait_for_selector(
        tid("grid-tariff-cell-pin-btn"), timeout=10000
    )
    assert el is not None and el.is_visible(), "grid-tariff-cell-pin-btn not visible"


@when("I click the pin button on the EV asset cell")
def step_pin_ev_cell(context):
    el = context.browser_page.wait_for_selector(
        tid("asset-cell-ev-pin-btn"), timeout=10000
    )
    el.click()


@when("I click the pin button on the EV asset cell again")
def step_unpin_ev_cell(context):
    el = context.browser_page.wait_for_selector(
        tid("asset-cell-ev-pin-btn"), timeout=10000
    )
    el.click()


@then("the EV asset cell is visible in the pinned zone")
def step_ev_cell_in_pinned_zone(context):
    pinned_zone = context.browser_page.wait_for_selector(
        tid("pinned-zone"), timeout=10000
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


@when("I click the collapse left button on the EV asset cell")
def step_collapse_left_ev(context):
    el = context.browser_page.wait_for_selector(
        tid("asset-cell-ev-collapse-left"), timeout=10000
    )
    el.click()


@then("the EV asset cell left section is not visible")
def step_ev_left_not_visible(context):
    left = context.browser_page.query_selector(tid("asset-cell-ev-left"))
    assert left is None or not left.is_visible(), (
        "asset-cell-ev-left is still visible after collapse"
    )


@when("I click the collapse right button on the EV asset cell")
def step_collapse_right_ev(context):
    el = context.browser_page.wait_for_selector(
        tid("asset-cell-ev-collapse-right"), timeout=10000
    )
    el.click()


@then("the EV asset cell right section is not visible")
def step_ev_right_not_visible(context):
    right = context.browser_page.query_selector(tid("asset-cell-ev-right"))
    assert right is None or not right.is_visible(), (
        "asset-cell-ev-right is still visible after collapse"
    )
