"""Step definitions for VEN Raw Data Diagnostics page UI scenarios."""

from behave import given, when, then
from features.helpers.ui import tid


def _slug(title: str) -> str:
    """Convert cell title to kebab-case testid slug."""
    return title.lower().replace(" ", "-")


# ── Background ────────────────────────────────────────────────────────────────

@given("the VEN UI is open")
def step_ven_ui_open(context):
    context.ven_ui.open()


@given("the user navigates to the Raw Data page")
def step_nav_raw_diagnostics(context):
    context.ven_ui.go_raw_diagnostics()


# ── Cell visibility ───────────────────────────────────────────────────────────

@then('I see the "{title}" cell')
def step_see_cell(context, title):
    el = context.browser_page.wait_for_selector(
        tid(f"diagnostic-cell-{_slug(title)}"), timeout=45000
    )
    assert el is not None and el.is_visible(), f'Cell "{title}" not visible'


# ── Refresh button interactions ───────────────────────────────────────────────

@when('I click the refresh button in the "{title}" cell')
def step_click_refresh(context, title):
    slug = _slug(title)
    sel = tid(f"refresh-btn-{slug}")
    context.browser_page.wait_for_selector(sel, timeout=45000)
    context.browser_page.dispatch_event(sel, "click")
    # Wait for the loading cycle to complete so downstream "Then" steps always
    # see post-load state. Some cells (Timeline) render with empty initial data,
    # so waiting here avoids the race where wait_for_selector finds the element
    # in its pre-load state before the fetch returns.
    try:
        context.browser_page.wait_for_selector(
            tid(f"loading-indicator-{slug}"), timeout=5000
        )
        context.browser_page.wait_for_selector(
            tid(f"loading-indicator-{slug}"), state="hidden", timeout=30000
        )
    except Exception:
        pass  # loading completed before we could observe it — proceed


# ── Chart assertions ──────────────────────────────────────────────────────────

@then("the Simulator State chart is displayed")
def step_sim_chart_displayed(context):
    el = context.browser_page.wait_for_selector(
        tid("sim-profile-chart"), timeout=45000
    )
    assert el is not None and el.is_visible(), "Simulator State chart not visible"


@then("the Tariffs chart is displayed")
def step_tariffs_chart_displayed(context):
    el = context.browser_page.wait_for_selector(
        tid("tariffs-line-chart"), timeout=45000
    )
    assert el is not None and el.is_visible(), "Tariffs chart not visible"


@then("the Timeline chart is displayed")
def step_timeline_chart_displayed(context):
    el = context.browser_page.wait_for_selector(
        tid("timeline-series-chart"), timeout=45000
    )
    assert el is not None and el.is_visible(), "Timeline chart not visible"


# ── Independence assertions ───────────────────────────────────────────────────

@then("only the Simulator State cell shows a loading state or data")
def step_sim_cell_has_content(context):
    # After clicking Sim refresh, the Sim cell should have data or be loading
    page = context.browser_page
    sim_cell = page.wait_for_selector(tid("diagnostic-cell-simulator-state"), timeout=45000)
    assert sim_cell is not None, "Simulator State cell not found"
    # Either loading or chart is present — not the unloaded placeholder any more
    loading = page.query_selector(tid("loading-indicator-simulator-state"))
    chart = page.query_selector(tid("sim-profile-chart"))
    assert loading is not None or chart is not None, (
        "Simulator State cell should show loading indicator or chart after refresh"
    )


@then("the Tariffs cell remains in its unloaded state")
def step_tariffs_cell_unloaded(context):
    page = context.browser_page
    # Tariffs chart should NOT be present (was never refreshed)
    chart = page.query_selector(tid("tariffs-line-chart"))
    assert chart is None, "Tariffs chart should not be present — cell was not refreshed"


# ── Timeline dropdown ─────────────────────────────────────────────────────────

@when('I select "{series}" from the Timeline series dropdown')
def step_select_timeline_series(context, series):
    page = context.browser_page
    select = page.wait_for_selector(tid("timeline-series-select"), timeout=45000)
    select.dispatch_event("click")
    # MUI Select renders options in a portal — find option by text content
    options = page.query_selector_all('li[role="option"]')
    option_found = None
    for opt in options:
        if series in opt.inner_text():
            option_found = opt
            break
    assert option_found is not None, f'Timeline option "{series}" not found in dropdown'
    option_found.dispatch_event("click")
    # Wait for dropdown to close and selection to be applied
    page.wait_for_timeout(500)
    context.selected_series = series


@then("the series dropdown lists the available asset series")
def step_series_dropdown_visible(context):
    page = context.browser_page
    el = page.wait_for_selector(
        tid("timeline-series-select"), timeout=45000
    )
    assert el is not None and el.is_visible(), "Timeline series dropdown not visible"

# MUI Select opens on mousedown, not a bare click event.  dispatch_event("click")
    # does not fire mousedown and leaves the popup closed. Use page.click() which
    # properly simulates the full pointer-event sequence.
    page.click(tid("timeline-series-select"))
    page.wait_for_selector('li[role="option"]', timeout=20000)
    options = page.query_selector_all('li[role="option"]')
    assert len(options) > 0, "Timeline dropdown has no options"


@then('the Timeline chart displays data for "{series}"')
def step_timeline_chart_shows_series(context, series):
    # Chart is displayed — verify the select value matches the chosen series
    page = context.browser_page
    el = page.wait_for_selector(tid("timeline-series-chart"), timeout=45000)
    assert el is not None and el.is_visible(), f"Timeline chart not visible for series '{series}'"
    # Verify the dropdown still shows the selected series
    select_el = page.query_selector(tid("timeline-series-select"))
    if select_el:
        inner = select_el.inner_text()
        assert series in inner, (
            f"Timeline series dropdown should show '{series}', got: '{inner}'"
        )
