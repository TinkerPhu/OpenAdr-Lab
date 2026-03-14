# Feature Specification: VEN Controller Dashboard V2

**Feature Branch**: `001-controller-dashboard-v2`
**Created**: 2026-03-14
**Status**: Draft
**Input**: User description: "ven-controller-dashboard-v2: make a second controller page in the ven ui with the final goal to replace the current. the page shall contain a series of cells, stacked vertically. One cell for each asset. On top of the page are also grid related cells."

## Clarifications

### Session 2026-03-14

- Q: Where do all dashboard values (including simulation settings) come from? → A: All values are sourced exclusively from the VEN API. If endpoints to read or change simulation settings do not yet exist, they MUST be stubbed with minimal placeholder implementations to be replaced in a subsequent change.
- Q: Are grid cells fixed at the top of the page? → A: No. Grid cells are simply ordered above asset cells in the default scroll order. They are only fixed (non-scrolling) when the user explicitly pins them — exactly like any other cell.
- Q: How does the stacked area chart handle sign? → A: Assets with positive power values stack upward above the x-axis; assets with negative power values stack downward below the x-axis. Their combined total equals grid power at every point in time.
- Q: How should offline or unresponsive assets be handled? → A: No special handling required. The VEN API will simply stop updating values; the last received values remain displayed unchanged.
- Q: How should missing or unavailable forecast data be handled? → A: Only draw data that the API actually returns. If forecast data is absent for a time range or series, that portion of the graph is left empty — no placeholder or error indication needed.
- Q: What happens when all cells are pinned? → A: No special handling. The operator can reload the page to reset all pinned states.
- Q: How should the dashboard behave if no assets are configured? → A: Check the VEN API for a baseline power measurement. If a baseline is available, display it. If not, show an empty placeholder cell labeled "Baseline" with a note that full baseline support will be added in a future feature.
- Q: What is the correct nomenclature for per-kWh vs. per-hour values? → A: **Tariff** = a price per unit of energy (X/kWh, e.g., €/kWh, g CO₂eq/kWh). **Rate** = an instantaneous flow per unit of time (X/h, e.g., €/h, g CO₂eq/h, kW). The VEN API endpoint `GET /rates` and its `RateSnapshot` struct predate this distinction and return tariff data (per-kWh values). A future API rename (`/tariffs`, `TariffSnapshot`) is planned but out of scope for this feature.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Monitor Asset Real-Time Status (Priority: P1)

A VEN operator opens the new controller dashboard and sees a vertically scrollable list of cells — one per energy asset — each displaying the asset's current power (import/export in kW), current cost rate (€/h), and current CO₂eq emission rate (CO₂eq/h). The operator can immediately tell the direction of energy flow (positive = importing from grid, negative = exporting) and the current financial and environmental impact of each asset.

**Why this priority**: Provides the core monitoring value of the dashboard. Without real-time asset status, the page has no standalone value. All other stories build on top of this foundation.

**Independent Test**: Load the dashboard with at least one simulated asset active. Verify that each asset cell's left section shows current power, cost rate, and CO₂eq rate values with correct units and sign convention. This delivers actionable operational awareness.

**Acceptance Scenarios**:

1. **Given** the dashboard is loaded with at least one active asset, **When** the operator views an asset cell, **Then** the left section shows current power in kW, cost rate in €/h, and CO₂eq rate in CO₂eq/h, each labeled and with correct units.
2. **Given** an asset is exporting energy to the grid, **When** the operator views that asset cell, **Then** the power value is negative, reflecting the export direction.
3. **Given** the dashboard has multiple assets, **When** the operator scrolls the page, **Then** each asset has its own dedicated cell stacked vertically, clearly separated.

---

### User Story 2 - View Asset Energy Timeline (Priority: P2)

The operator views a time-series graph in the center section of each asset cell. The graph shows the asset's past, present, and future power, cost rate, and CO₂eq rate on a shared time axis. A vertical red dotted line marks the present moment. The left half shows historical values and the right half shows planned/forecasted values. Multiple line types (solid, dashed, dotted) distinguish the three metrics while a single color per asset maintains visual consistency.

**Why this priority**: The timeline graph is the main analytical tool — it lets operators see trends, anticipate demand spikes, and verify that planned responses are aligned with forecasts. It transforms the dashboard from a status display into a planning tool.

**Independent Test**: With historical and forecast data available, verify the mid section graph renders past values to the left of the "now" line and forecast/planned values to the right, using solid/dashed/dotted line types for power/cost/CO₂eq respectively.

**Acceptance Scenarios**:

1. **Given** an asset cell is visible, **When** the operator looks at the center section, **Then** the graph shows a time axis with past on the left and future on the right, and a vertical red dotted line marking the present.
2. **Given** an asset has forecast or planned data, **When** the graph renders, **Then** future power values are shown as a solid line, future cost rate as a dashed line, and future CO₂eq rate as a dotted line, all in the same asset color.
3. **Given** the graphs may use different value scales (kW vs. €/h vs. CO₂eq/h), **When** the operator reads the graph, **Then** units are identified in the legend so values can be interpreted without ambiguity.
4. **Given** values are signed, **When** an asset has negative power (exporting), **Then** the power graph line falls below the x-axis.

---

### User Story 3 - Simulate Asset Behavior via Controls (Priority: P3)

The operator uses the right section of an asset cell to adjust simulation parameters in real time. The upper sub-section contains status controls (e.g., state of charge, power on/off toggle). The lower sub-section contains simulation characteristic overrides (e.g., battery capacity, power limits, response delays). Changes take immediate effect on the simulation without requiring a page reload.

**Why this priority**: Simulation control is essential for testing DR responses and what-if scenarios. Without it, the dashboard is read-only. It is placed at P3 because monitoring (P1, P2) delivers standalone value even without controls.

**Independent Test**: With the dashboard loaded, change the power on/off state for one asset via its right section controls. Verify the asset's power value in the left section and graph reflect the change within one simulation tick.

**Acceptance Scenarios**:

1. **Given** an asset cell with a battery asset, **When** the operator adjusts the State of Charge (SoC) slider in the right section, **Then** the asset's SoC value updates and the simulation reflects the new value.
2. **Given** an asset cell, **When** the operator toggles the power on/off control, **Then** the asset's current power updates accordingly (e.g., zero power when off, simulating a device disconnected or EV unplugged).
3. **Given** the right section has both status settings and simulation characteristic overrides, **When** displayed, **Then** the two groups are visually separated and individually collapsible to manage available space.
4. **Given** the right section is collapsed by the user, **When** the cell renders, **Then** only the left section and graph (mid section) are visible, and the collapse state is preserved while scrolling.

---

### User Story 4 - Monitor Grid Tariffs and Power Balance (Priority: P2)

The operator views two grid-level cells at the top of the page (above all asset cells): a tariff cell and an accumulated asset power cell. These cells scroll with the page by default and become fixed only if the operator pins them. The tariff cell shows current import/export tariffs [€/kWh], import CO₂eq tariff [g CO₂eq/kWh], total cost rate [€/h], total CO₂eq rate [g CO₂eq/h], and grid power [kW]. The accumulated asset power cell shows a breakdown of all assets' current power values and a stacked area chart visualizing how each asset contributes to total grid power over time.

**Why this priority**: Grid-level context is essential for interpreting individual asset data — operators need to see the overall power balance and tariff conditions before acting on specific assets. Rated P2 alongside the timeline story because it provides independent, non-asset-specific value.

**Independent Test**: Load the dashboard with multiple active assets. Verify that the accumulated asset power cell's chart shows each asset as a colored stacked area, with positive values above and negative values below the x-axis, and that their sum at any point matches the total grid power shown in the tariff cell.

**Acceptance Scenarios**:

1. **Given** the dashboard is loaded, **When** the operator views the top of the page, **Then** the tariff cell and accumulated asset power cell are visible above all asset cells.
2. **Given** the tariff cell is visible, **When** the operator reads it, **Then** it shows current import tariff [€/kWh], import CO₂eq tariff [g CO₂eq/kWh], export tariff [€/kWh], total cost rate [€/h], total CO₂eq rate [g CO₂eq/h], and current grid power [kW].
3. **Given** the tariff cell right section graph, **When** rendered, **Then** it shows import tariff (red dashed), import CO₂eq (red dotted), export tariff (green dashed), total cost rate (black dashed), and grid power (black solid) lines on a shared time axis.
4. **Given** the accumulated asset power cell, **When** multiple assets are active, **Then** the left section lists each asset's current power [kW] and the right section shows a stacked area chart where each asset has a distinct color, positive contributions stack above the x-axis, and negative contributions stack below.
5. **Given** all asset areas are summed, **When** the operator compares the total at any point in time to the grid power in the tariff cell, **Then** the values match.

---

### User Story 5 - Navigate and Customize Dashboard Layout (Priority: P3)

The operator scrolls through the vertically stacked cells on a long page. To keep frequently referenced cells in view while scrolling, the operator pins one or more cells. Pinned cells relocate to the top of the page and remain fixed while the rest of the content scrolls underneath. The operator can also collapse the left and/or right sections of any asset cell to focus on the graph, reducing visual clutter.

**Why this priority**: The page is expected to exceed a typical screen height with many assets. Layout controls make the dashboard usable at scale and let operators personalize their view. Rated P3 because the dashboard delivers value without pinning; this enhances ergonomics.

**Independent Test**: On a page with at least 4 asset cells, pin one cell, then scroll down. Verify the pinned cell stays at the top of the viewport while unpinned cells scroll normally. Then collapse the left section of a non-pinned asset cell and verify the graph and right section remain visible.

**Acceptance Scenarios**:

1. **Given** a dashboard with enough cells to exceed screen height, **When** the operator scrolls down, **Then** the page scrolls smoothly and all cells remain accessible.
2. **Given** the operator pins a cell, **When** scrolling the page, **Then** the pinned cell remains fixed at the top of the viewport and does not scroll away.
3. **Given** multiple cells are pinned, **When** scrolling, **Then** all pinned cells stack at the top in the order they were pinned, and unpinned content scrolls beneath them.
4. **Given** a pinned cell, **When** the operator unpins it, **Then** it returns to its original position in the scrollable content area.
5. **Given** an asset cell, **When** the operator collapses the left section, **Then** only the mid (graph) and right (controls) sections are visible, and collapsing is indicated visually (e.g., a toggle icon).
6. **Given** an asset cell, **When** the operator collapses the right section, **Then** only the left (metrics) and mid (graph) sections are visible.

---

### Edge Cases

- What happens when an asset goes offline or data stops updating? No special handling — the VEN API simply stops updating values and the last received values remain displayed.
- What happens if forecast data is unavailable for an asset? Only data actually returned by the API is drawn. Missing time ranges or series are left visually empty with no placeholder or error.
- What happens when all cells are pinned? No special handling — the operator reloads the page to reset pinned state.
- What happens when `GET /sim` returns an error? The dashboard MUST display an error state (not a placeholder cell). The "Baseline" placeholder cell is only relevant if the API responds successfully but reports zero named assets — which does not currently occur since `base_load_w` is always present in a valid `/sim` response.

---

## Requirements *(mandatory)*

### Functional Requirements

**Page Layout**

- **FR-001**: The dashboard MUST be presented as a separate page/view accessible alongside the existing controller page (not replacing it), reachable via a tab or navigation link labeled "Controller V2" or equivalent.
- **FR-002**: The page content MUST be scrollable vertically when the total height of all cells exceeds the viewport height.
- **FR-003**: Grid cells MUST appear above all asset cells in the default page order. Grid cells scroll with the page like any other cell; they are only fixed to the viewport when the user explicitly pins them.
- **FR-004**: There MUST be exactly two grid cells: a Tariff Cell and an Accumulated Asset Power Cell.
- **FR-005**: There MUST be exactly one asset cell per configured asset, stacked vertically below the grid cells.

**Cell Pinning**

- **FR-006**: Each cell MUST provide a pin control (button or icon) that the user can toggle.
- **FR-007**: When a cell is pinned, it MUST move to a fixed zone at the top of the viewport that does not scroll with the page.
- **FR-008**: Pinned cells MUST stack in the order they were pinned, with the most recently pinned cell at the bottom of the pinned zone.
- **FR-009**: When a cell is unpinned, it MUST return to its original position relative to other cells in the scrollable area.

**Asset Cell — Left Section (Metrics)**

- **FR-010**: Each asset cell MUST display a left section showing at minimum: current power [kW], current cost rate [€/h], and current CO₂eq emission rate [CO₂eq/h], each with a label and unit.
- **FR-011**: Power values MUST use signed representation: positive = importing from grid, negative = exporting to grid.
- **FR-012**: If forecast/planned energy data is available for the visible time window, the left section MUST also show the total forecasted energy [kWh] visible in the graph (sum of all planned/requested slices in view).
- **FR-013**: If an active or upcoming user request exists for the asset, the left section MUST display the closest or currently active user request, including: requested energy and due time. If multiple requests are active simultaneously, the one with the largest requested energy value MUST be shown.
- **FR-014**: The left section MUST be collapsible by the user.

**Asset Cell — Mid Section (Graph)**

- **FR-015**: Each asset cell MUST display a center section with a time-series graph covering a past half and a future half on a shared time axis.
- **FR-016**: The present moment MUST be marked with a vertical red dotted line at the center of the graph's time axis.
- **FR-017**: The graph MUST display three data series for the asset: power [kW] as a solid line, cost rate [€/h] as a dashed line, and CO₂eq rate [CO₂eq/h] as a dotted line.
- **FR-018**: All three lines in a single asset cell MUST share the same color (unique per asset across the full page).
- **FR-019**: The y-axis MUST support the presence of multiple value scales; units MUST be identifiable via the legend at minimum.
- **FR-020**: Graph lines MUST render below the x-axis when values are negative (export direction).

**Asset Cell — Right Section (Controls)**

- **FR-021**: Each asset cell MUST display a right section with interactive controls to modify the asset's simulation state.
- **FR-022**: The right section MUST be divided into two groups: Status Settings (upper) and Simulation Characteristics (lower).
- **FR-023**: Status Settings MUST include asset-appropriate controls such as: State of Charge (SoC) for battery-type assets, and a power on/off toggle applicable to all assets.
- **FR-024**: Simulation Characteristics MUST include asset-appropriate overrides such as: battery capacity, power limits, and response delays.
- **FR-025**: Both control groups in the right section MUST be individually collapsible to manage vertical space.
- **FR-026**: The right section MUST be collapsible by the user (hiding both groups at once).
- **FR-027**: All right-section control values MUST be read from and written to the VEN API. Changes MUST take effect in the simulation without requiring a page reload. If API endpoints for reading or modifying simulation settings do not yet exist, the controls MUST be implemented with minimal stub code that will be replaced in a subsequent change.

**Grid Cell — Tariff Cell**

- **FR-028**: The Tariff Cell MUST display in its left section: current import tariff [€/kWh], import CO₂eq tariff [g CO₂eq/kWh], current export tariff [€/kWh], total cost rate [€/h], total CO₂eq rate [g CO₂eq/h], and current grid power [kW]. (Distinction: tariff values come directly from the API per-kWh; rate values are derived as tariff × power.)
- **FR-029**: The Tariff Cell MUST display in its right section a time-series graph showing: import tariff [€/kWh] (red dashed), import CO₂eq tariff [g CO₂eq/kWh] (red dotted), export tariff [€/kWh] (green dashed), total cost rate [€/h] (black dashed), and grid power [kW] (black solid).
- **FR-030**: The Tariff Cell MUST NOT provide any user controls (grid data is read-only).

**Grid Cell — Accumulated Asset Power Cell**

- **FR-031**: The Accumulated Asset Power Cell MUST display in its left section a list of all assets with their current power [kW].
- **FR-032**: The Accumulated Asset Power Cell MUST display in its right section a stacked area chart over time where each asset is represented by a distinct color.
- **FR-033**: In the stacked area chart, assets with positive power values (importing from grid) MUST stack their areas upward above the x-axis; assets with negative power values (exporting to grid) MUST stack their areas downward below the x-axis. The two groups are stacked independently on each side of the x-axis.
- **FR-034**: The sum of all stacked areas at any point in time MUST equal the total grid power displayed in the Tariff Cell.
- **FR-035**: The Accumulated Asset Power Cell MUST NOT provide any user controls.

### Key Entities

- **Asset Cell**: One per configured energy asset. Has three sections (left, mid, right), each independently collapsible. Displays metrics, timeline graph, and simulation controls for one asset.
- **Grid Cell**: Read-only information cell with two sections (left: values, right: graph). Two instances: Tariff Cell and Accumulated Asset Power Cell.
- **Power Timeline**: Time-series data for one asset covering past measurements and future forecasts, rendered as a multi-line graph with a "now" marker.
- **Pinned Zone**: A fixed non-scrolling area at the top of the viewport that holds all currently pinned cells.
- **User Request**: An operator-issued energy demand for an asset with a requested energy amount and a deadline. Displayed on the relevant asset cell when active or upcoming.
- **Tariff Snapshot**: Current import/export energy prices and associated CO₂eq intensity, plus derived total cost rate.

## Assumptions

- **A-001**: Forecast and planned energy data (future power values) are served by the existing controller API endpoints (e.g., /plan, /packets). Only data actually returned by the API is rendered; missing data for any time range or series is simply not drawn, with no placeholder or error indication.
- **A-002**: The graph time window shows 1 hour of history on the left half and 1 hour of forecast on the right half (2 hours total visible), centered on the present moment. This default may be revisited during planning.
- **A-003**: The CO₂eq unit label "CO₂eq/h" (CO₂ equivalent per hour) is used throughout the dashboard in place of the placeholder "GHGEQ/h". The underlying quantity and value scale are determined by whatever the data source provides.
- **A-004**: Asset colors are assigned deterministically (e.g., by asset index) so that the same asset always has the same color within a session.
- **A-005**: Pin state is not persisted; reloading the page resets all cells to unpinned. This is also the intended way to clear all pins if needed.
- **A-006**: The right section's two control groups (Status Settings, Simulation Characteristics) are individually collapsible via accordion-style disclosure to address the space constraint the author raised.
- **A-007**: Grid cells (Tariff and Accumulated Asset Power) can also be pinned using the same pin mechanism as asset cells. They scroll with the page by default.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: An operator can identify the current power, cost rate, and CO₂eq rate of every asset without scrolling horizontally — all key values are visible within each asset cell's left section.
- **SC-002**: An operator can read historical and forecast trends for any asset by looking at a single graph without navigating away from the main page.
- **SC-003**: An operator can change a simulation control (e.g., toggle power on/off or adjust SoC) and see the updated value reflected in the asset cell's metrics within 5 seconds.
- **SC-004**: An operator can pin at least one cell and continue scrolling the remaining cells without the pinned cell disappearing from view.
- **SC-005**: At any point in time, the sum of all asset power values in the Accumulated Asset Power Cell matches the grid power value in the Tariff Cell (within rounding tolerance of ±0.01 kW).
- **SC-006**: The dashboard remains usable (scrollable, interactive) with up to 10 simultaneous asset cells without visual overlap or layout breakage.
- **SC-007**: Collapsing both the left and right sections of an asset cell reduces the cell's visible footprint to the graph only, giving the operator a compact view when monitoring many assets.
