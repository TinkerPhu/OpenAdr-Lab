"""Step definitions for Planner Visualization Page UI scenarios."""

from behave import when, then
from features.helpers.ui import tid
from features.helpers.wait import poll_until


# ── Background / Navigation ───────────────────────────────────────────────────

# NOTE: "the VEN UI is open" step is defined in ven_ui_raw_diagnostics_steps.py

@when("I navigate to the Planner page")
def step_navigate_planner(context):
    context.ven_ui.go_planner()


@then('I see a nav button with testid "{testid}"')
def step_see_nav_button(context, testid):
    el = context.browser_page.wait_for_selector(tid(testid), timeout=45000)
    assert el is not None and el.is_visible(), f'Nav button "{testid}" not visible'


@when('I click the nav button "{testid}"')
def step_click_nav_button(context, testid):
    context.browser_page.dispatch_event(tid(testid), "click")


@then('I see an element with testid "{testid}"')
def step_see_element(context, testid):
    el = context.browser_page.wait_for_selector(tid(testid), timeout=45000)
    assert el is not None and el.is_visible(), f'Element "{testid}" not visible'


@when('I click the element with testid "{testid}"')
def step_click_element(context, testid):
    context.browser_page.dispatch_event(tid(testid), "click")


# ── Trigger Timeline steps ────────────────────────────────────────────────────

@then("at least one trigger chip is visible")
def step_at_least_one_chip(context):
    page = context.browser_page
    page.wait_for_selector('[data-testid^="trigger-chip-"]', timeout=45000)
    chips = page.query_selector_all('[data-testid^="trigger-chip-"]')
    assert len(chips) > 0, "No trigger chips found in timeline"


@when("I click the first trigger chip")
def step_click_first_chip(context):
    page = context.browser_page
    page.wait_for_selector('[data-testid^="trigger-chip-"]', timeout=45000)
    # Use dispatch_event to handle MUI Tooltip wrapping around chip buttons
    page.dispatch_event('[data-testid^="trigger-chip-"]', "click")


# ── Decision Matrix steps ─────────────────────────────────────────────────────

@then("the decision matrix shows at least one asset row")
def step_matrix_has_asset_rows(context):
    page = context.browser_page
    page.wait_for_selector(tid("decision-matrix"), timeout=45000)
    cells = page.query_selector_all('[data-testid^="matrix-cell-"]')
    assert len(cells) > 0, "Decision matrix has no asset cells"


@then("the decision matrix shows the tariff header row")
def step_matrix_has_tariff_header(context):
    page = context.browser_page
    header = page.wait_for_selector(tid("matrix-tariff-header"), timeout=45000)
    assert header is not None and header.is_visible(), "Tariff header row not visible"


@when("I click the first visible matrix cell")
def step_click_first_matrix_cell(context):
    page = context.browser_page
    page.wait_for_selector('[data-testid^="matrix-cell-"]', timeout=45000)
    cells = page.query_selector_all('[data-testid^="matrix-cell-"]')
    assert len(cells) > 0, "No matrix cells to click"
    page.dispatch_event(cells[0], "click")


@when("I click the first matrix cell with nonzero power")
def step_click_first_nonzero_matrix_cell(context):
    """Click the first matrix cell that has a nonzero power value (has an associated step)."""
    page = context.browser_page
    page.wait_for_selector('[data-testid^="matrix-cell-"]', timeout=45000)
    cells = page.query_selector_all('[data-testid^="matrix-cell-"]')
    assert len(cells) > 0, "No matrix cells found"
    # Find first cell with data-power > 0 so there is an associated PlanStep
    clicked = False
    for cell in cells:
        power_str = cell.get_attribute("data-power") or "0"
        try:
            if float(power_str) > 0.01:
                page.dispatch_event(cell, "click")
                clicked = True
                break
        except ValueError:
            pass
    assert clicked, "No matrix cell with nonzero power found to click"


@then("the decision matrix cells are hidden")
def step_matrix_cells_hidden(context):
    page = context.browser_page
    # After collapsing, no matrix cells should be visible
    cells = page.query_selector_all('[data-testid^="matrix-cell-"]')
    visible_cells = [c for c in cells if c.is_visible()]
    assert len(visible_cells) == 0, f"Expected 0 visible cells after collapse, got {len(visible_cells)}"


@then("the decision matrix cells are visible")
def step_matrix_cells_visible(context):
    page = context.browser_page
    cells = page.query_selector_all('[data-testid^="matrix-cell-"]')
    visible_cells = [c for c in cells if c.is_visible()]
    assert len(visible_cells) > 0, "No matrix cells visible after expand"
