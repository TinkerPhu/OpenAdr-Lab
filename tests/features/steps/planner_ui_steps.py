"""Step definitions for Planner Visualization Page UI scenarios."""

from behave import given, when, then
from features.helpers.api_client import ven_get, ven_post
from features.helpers.ui import tid
from features.helpers.wait import poll_until


# ── Background / Navigation ───────────────────────────────────────────────────

# NOTE: "the VEN UI is open" step is defined in ven_ui_raw_diagnostics_steps.py

@when("I navigate to the Planner page")
def step_navigate_planner(context):
    context.ven_ui.go_planner()


@then('I see a nav button with testid "{testid}"')
def step_see_nav_button(context, testid):
    el = context.browser_page.wait_for_selector(tid(testid), timeout=10000)
    assert el is not None and el.is_visible(), f'Nav button "{testid}" not visible'


@when('I click the nav button "{testid}"')
def step_click_nav_button(context, testid):
    context.browser_page.click(tid(testid))


@then('I see an element with testid "{testid}"')
def step_see_element(context, testid):
    el = context.browser_page.wait_for_selector(tid(testid), timeout=15000)
    assert el is not None and el.is_visible(), f'Element "{testid}" not visible'


@when('I click the element with testid "{testid}"')
def step_click_element(context, testid):
    context.browser_page.click(tid(testid))


# ── State preconditions ───────────────────────────────────────────────────────

@given("the planner has not yet generated a plan")
def step_no_plan_yet(context):
    """
    This step is satisfied when the VEN has been freshly restarted and no plan exists.
    In practice, we cannot guarantee this in the test environment — this scenario is
    better exercised via unit tests. Skip gracefully if a plan already exists.
    """
    r = ven_get("/plan")
    if r.status_code == 200 and r.json() is not None:
        context.scenario.skip("Plan already exists; empty-state scenario skipped")


@given("no energy packets exist for this VEN")
def step_no_packets(context):
    """Verify no active/pending packets exist; skip if packets are present."""
    r = ven_get("/packets")
    r.raise_for_status()
    packets = r.json()
    active = [p for p in packets if p.get("status") in ("ACTIVE", "PENDING", "SCHEDULED")]
    if active:
        context.scenario.skip("Active packets exist; empty-state scenario skipped")


@given("at least one energy packet exists for this VEN")
def step_has_packets(context):
    """Poll /packets until at least one non-done packet exists (up to 30s)."""
    def _has_packet():
        r = ven_get("/packets")
        if not r.ok:
            return False
        packets = r.json()
        return any(p.get("status") in ("ACTIVE", "PENDING", "SCHEDULED") for p in packets)

    poll_until(
        _has_packet,
        lambda result: result is True,
        timeout=30,
        interval=2,
        description="VEN has at least one active/pending packet",
    )


# ── Trigger Timeline steps ────────────────────────────────────────────────────

@then("at least one trigger chip is visible")
def step_at_least_one_chip(context):
    page = context.browser_page
    page.wait_for_selector(tid("trigger-timeline"), timeout=10000)
    chips = page.query_selector_all('[data-testid^="trigger-chip-"]')
    assert len(chips) > 0, "No trigger chips found in timeline"


@when("I click the first trigger chip")
def step_click_first_chip(context):
    page = context.browser_page
    page.wait_for_selector('[data-testid^="trigger-chip-"]', timeout=10000)
    chips = page.query_selector_all('[data-testid^="trigger-chip-"]')
    assert len(chips) > 0, "No trigger chips to click"
    chips[0].click()


# ── Decision Matrix steps ─────────────────────────────────────────────────────

@then("the decision matrix shows at least one asset row")
def step_matrix_has_asset_rows(context):
    page = context.browser_page
    page.wait_for_selector(tid("decision-matrix"), timeout=10000)
    cells = page.query_selector_all('[data-testid^="matrix-cell-"]')
    assert len(cells) > 0, "Decision matrix has no asset cells"


@then("the decision matrix shows the tariff header row")
def step_matrix_has_tariff_header(context):
    page = context.browser_page
    header = page.wait_for_selector(tid("matrix-tariff-header"), timeout=10000)
    assert header is not None and header.is_visible(), "Tariff header row not visible"


@when("I click the first visible matrix cell")
def step_click_first_matrix_cell(context):
    page = context.browser_page
    page.wait_for_selector('[data-testid^="matrix-cell-"]', timeout=10000)
    cells = page.query_selector_all('[data-testid^="matrix-cell-"]')
    assert len(cells) > 0, "No matrix cells to click"
    cells[0].click()


@when("I click the first matrix cell with nonzero power")
def step_click_first_nonzero_matrix_cell(context):
    """Click the first matrix cell that has a nonzero power value (has an associated step)."""
    page = context.browser_page
    page.wait_for_selector('[data-testid^="matrix-cell-"]', timeout=10000)
    cells = page.query_selector_all('[data-testid^="matrix-cell-"]')
    assert len(cells) > 0, "No matrix cells found"
    # Find first cell with data-power > 0 so there is an associated PlanStep
    clicked = False
    for cell in cells:
        power_str = cell.get_attribute("data-power") or "0"
        try:
            if float(power_str) > 0.01:
                cell.click()
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
