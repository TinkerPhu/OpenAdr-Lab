# Feature Specification: VEN Raw Data Diagnostics Page

**Feature Branch**: `006-ven-raw-diagnostics`
**Created**: 2026-03-18
**Status**: Draft
**Input**: User description: "create a ven ui page to display raw data in diagrams. make some cells stacked from top to down, each containing a diagram and a small refresh button. the refresh button directly fetches data from one specific ven api endpoint and displays it on the diagram. endpoints are: /sim, /plan, /tariffs, /timeline/all (plus minus 1h, for this endpoint, create a dropdown of assets/grid to choose the series). All graphs shall simply show all data points that are connected by lines. each graph has different color."

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Inspect Simulator State (Priority: P1)

An operator opens the VEN Raw Data page and views a live snapshot of the device simulator state — showing the current power output/consumption and energy counters for each asset. A single click on the refresh button in that cell pulls the latest values from the backend and redraws the chart.

**Why this priority**: Simulator state is the primary diagnostic view; if assets are misbehaving, the operator checks this first. It is the most frequently needed cell and the simplest to implement independently.

**Independent Test**: Navigate to the diagnostics page, click the Sim cell refresh button, and verify the chart updates with the current asset readings — delivers a standalone read of simulator state.

**Acceptance Scenarios**:

1. **Given** the diagnostics page is open, **When** the user clicks the Sim refresh button, **Then** the chart redraws showing the latest power readings for all assets as connected data points.
2. **Given** no VEN data is available (empty response), **When** the Sim cell loads, **Then** the chart shows an empty state message and no error is thrown.
3. **Given** multiple assets exist, **Then** each asset's data series is drawn in a distinct color.

---

### User Story 2 - Inspect Tariff Data (Priority: P2)

An operator checks the Tariffs cell to see the current pricing schedule (import/export tariffs over time) visualized as a line chart, refreshable on demand.

**Why this priority**: Tariff visibility is needed to validate that the planner is responding to correct price signals; less urgent than sim/plan but important for diagnosing scheduling decisions.

**Independent Test**: Click the Tariffs cell refresh button and verify the tariff chart renders price-over-time data as connected lines.

**Acceptance Scenarios**:

1. **Given** the diagnostics page is open, **When** the user clicks the Tariffs refresh button, **Then** the chart redraws showing tariff values over time.
2. **Given** both import and export tariffs exist as separate series, **Then** each series appears in a distinct color.

---

### User Story 3 - Browse Historical and Near-Future Timeline (Priority: P3)

An operator selects a specific asset (or the grid) from a dropdown in the Timeline cell, then clicks refresh to see its power readings over a ±1 hour window centered on now, displayed as a connected line chart.

**Why this priority**: Timeline view adds historical context but is only useful once the operator has the other cells working; it also requires extra interaction (dropdown selection) making it slightly more complex.

**Independent Test**: Select "grid" from the Timeline dropdown, click refresh, and verify the chart shows grid power readings spanning from one hour ago to one hour ahead as connected data points.

**Acceptance Scenarios**:

1. **Given** the Timeline cell is visible, **When** the user selects an asset from the dropdown and clicks refresh, **Then** the chart shows that asset's readings over the ±1 hour window as connected data points.
2. **Given** the user changes the dropdown selection and clicks refresh, **Then** the chart updates to reflect the newly selected series.
3. **Given** "grid" is selected, **When** the user clicks refresh, **Then** grid-level readings are shown instead of individual asset data.
4. **Given** no data exists in the time window, **Then** the chart shows an empty state message.

---

### Edge Cases

- What happens when a backend endpoint returns an error (network failure, 500)? The affected cell shows an inline error message; other cells are unaffected.
- What happens when a cell has never been refreshed (initial page load)? Each cell starts in an empty/unloaded state — data is only fetched when the user clicks the cell's refresh button.
- What if the timeline window contains hundreds of data points? All points are rendered and connected; no downsampling.
- What if an asset has only one data point? A single dot is shown; no connecting line is drawn (graceful degrade).

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The page MUST display exactly three diagnostic cells stacked vertically in this order: Simulator State, Tariffs, Timeline.
- **FR-002**: Each cell MUST contain a line chart area and a clearly labeled refresh button.
- **FR-003**: Clicking a cell's refresh button MUST fetch fresh data exclusively from that cell's designated endpoint without triggering other cells to refresh.
- **FR-004**: Each chart MUST render all returned data points connected by continuous lines (no gaps between consecutive points).
- **FR-005**: Each distinct data series within a chart MUST use a different color; colors MUST be visually distinguishable from one another.
- **FR-006**: The Simulator State cell MUST source its data from the `/sim` endpoint, plotting power and/or energy values per asset.
- **FR-007**: The Tariffs cell MUST source its data from the `/tariffs` endpoint, plotting price values over time.
- **FR-008**: The Timeline cell MUST source its data from the `/timeline/all` endpoint using a time window of ±1 hour relative to the current moment at the time of refresh.
- **FR-009**: The Timeline cell MUST include a dropdown control listing all available series (individual assets and a "grid" option); only the selected series is displayed on the chart after refresh.
- **FR-010**: The diagnostics page MUST be accessible via a dedicated tab or route within the existing VEN UI navigation.
- **FR-011**: While a cell is loading data, it MUST display a loading indicator.
- **FR-012**: If a cell's data fetch fails, the cell MUST display a brief error message; the failure MUST NOT affect other cells.

### Key Entities

- **Diagnostic Cell**: A visual unit containing a chart and a refresh button, bound to one backend endpoint.
- **Data Series**: A named sequence of (x, y) value pairs displayed as a colored line within a chart.
- **Timeline Series Selector**: A dropdown within the Timeline cell that determines which asset or grid series the chart displays.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: All three diagnostic cells are visible on a standard desktop screen (1280×720 or larger) without horizontal scrolling.
- **SC-002**: Each cell refreshes its data independently — clicking one refresh button does not update any other cell's chart.
- **SC-003**: Updated chart data is visible within 3 seconds of clicking a refresh button under normal network conditions.
- **SC-004**: Changing the Timeline dropdown selection and clicking refresh correctly replaces the chart with the newly selected series.
- **SC-005**: Each chart renders with at least two visually distinct colors when two or more series are present.
- **SC-006**: An error in one cell does not cause any other cell to show an error or lose its currently displayed data.

## Assumptions

- The VEN backend exposes `/sim`, `/tariffs`, and `/timeline/all` endpoints that return JSON suitable for charting (time-indexed or array-of-objects format).
- The `/timeline/all` endpoint accepts query parameters for start and end time to constrain the ±1 hour window.
- The list of valid series for the Timeline dropdown (asset names + "grid") is derived from the `/timeline/all` response keys or a static list matching the VEN's known assets.
- Charts do not need interactive tooltips or zoom controls in this initial version — plain connected line charts are sufficient.
- No auto-refresh or polling is required; all data fetching is manual (on button click only).
- The page is read-only; no controls to write back to the VEN are included.
