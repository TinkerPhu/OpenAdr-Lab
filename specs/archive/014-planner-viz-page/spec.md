# Feature Specification: Planner Visualization Page

**Feature Branch**: `014-planner-viz-page`
**Created**: 2026-04-04
**Status**: Draft
**Input**: User description: "take d:\Tinker\OpenAdr-Lab\docs\plans\planner_visualization_proposal.md as specification input. split it up more if necessary. make sure all introduced ui features are well tested."

## Overview

The VEN HEMS planner runs continuously and makes scheduling decisions every 20 seconds or when triggered by external events (rate changes, OpenADR events, user requests). Currently these decisions are opaque: the UI shows aggregate numbers (total cost, total import kWh) but not *why* each asset was scheduled as it was, *what caused* a replan, or *whether* a packet will meet its deadline.

This feature adds a dedicated **Planner** page to the VEN Web UI that gives engineers and operators full transparency into planner behavior across four complementary views: a plan summary header, a controller event timeline, a per-slot decision matrix, and a packet progress board.

---

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Understand Why an Asset Was Scheduled (Priority: P1)

An engineer notices the battery is charging heavily and wants to understand why. They open the Planner page and look at the Decision Matrix. Each time slot for each asset is displayed with a color-coded reason label. The battery row clearly shows green "cheap tariff" cells aligned with low-tariff columns in the tariff header. The engineer clicks one cell and sees the exact tariff value and the threshold that triggered the decision.

**Why this priority**: This is the core transparency problem. Without slot-level reasoning, engineers cannot verify that the planner is behaving correctly or diagnose unexpected behavior. This is the fundamental justification for the entire feature.

**Independent Test**: Deploy only the Decision Matrix section. An engineer can open it, read the reason labels on each cell, click a cell to see detail, and confirm the battery is charging because the tariff is below a threshold. Delivers full planning transparency as a standalone feature.

**Acceptance Scenarios**:

1. **Given** the Planner page is open and the planner has produced a plan, **When** the Decision Matrix section renders, **Then** each asset is shown as a separate row, each time slot is shown as a column, and every cell displays a color and icon matching one of the 12 defined reason labels.

2. **Given** the Decision Matrix is visible, **When** the engineer hovers over a cell, **Then** a tooltip displays the reason name (e.g., "Cheap Tariff") and the asset name.

3. **Given** the Decision Matrix is visible, **When** the engineer clicks a cell, **Then** a detail drawer opens showing: the slot timestamp, the asset ID, the setpoint power, the actual power executed, and a structured breakdown of the reason with its numeric parameters (e.g., tariff value and threshold for CheapTariff).

4. **Given** the Decision Matrix is visible, **When** the engineer looks at the column header row, **Then** each column header is colored on a gradient from green (cheap) to red (expensive) reflecting the import tariff value for that time slot.

5. **Given** the Decision Matrix is visible, **When** the engineer looks at the planning horizon, **Then** a visible vertical divider separates the FIRM zone (left, full opacity) from the FLEXIBLE zone (right, reduced opacity), so the boundary between committed and tentative slots is immediately clear.

6. **Given** the FIRM/FLEX boundary divider is visible, **When** the engineer looks at the flexible-zone cells, **Then** those cells appear visually distinct (reduced opacity or dashed border) compared to firm-zone cells.

7. **Given** the Decision Matrix is displaying the full planning horizon (up to 288 columns), **When** the page loads, **Then** the default view shows only FIRM-zone columns (approximately 36 columns) to avoid rendering hundreds of cells immediately, and a control to expand to the full horizon is available.

8. **Given** the full horizon expand control exists, **When** the engineer clicks it, **Then** the Decision Matrix expands to display all time slots including the FLEXIBLE zone.

9. **Given** the Decision Matrix is visible and a plan update arrives, **When** the data refreshes (every 10 seconds), **Then** the matrix updates in place without the engineer navigating away.

10. **Given** no plan exists yet (planner has not yet run), **When** the Decision Matrix section renders, **Then** an informative empty state message is shown instead of an empty grid.

---

### User Story 2 - Track Whether an Energy Packet Will Meet Its Deadline (Priority: P2)

An operator wants to know if their EV will finish charging before the 07:00 departure deadline. They open the Planner page and look at the Packet Progress Board. The EV packet card shows a fill gauge at 62%, a countdown "T−4h30m" to the deadline, and a budget bar showing remaining spend headroom. The operator can tell at a glance whether the packet is on track.

**Why this priority**: Operators care about outcomes, not planning mechanics. The Packet Board answers "will my request complete?" without requiring any understanding of the underlying scheduling algorithm. It is complementary to the Decision Matrix and independently valuable.

**Independent Test**: Deploy only the Packet Board section. An operator can open it, read the fill gauge and deadline countdown for the EV packet, and determine if it is on track. Delivers outcome transparency as a standalone feature.

**Acceptance Scenarios**:

1. **Given** the Planner page is open and packets exist, **When** the Packet Board section renders, **Then** each energy packet is shown as a card displaying: asset label, current status chip, fill percentage gauge, deadline countdown, and target energy in kWh.

2. **Given** a packet has a budget limit set for the active deadline tier, **When** the packet card is displayed, **Then** a budget bar shows accumulated cost against the limit (e.g., "€0.44 / €1.20") with a visual fill bar.

3. **Given** a packet has no budget limit for the active tier, **When** the packet card is displayed, **Then** no budget bar is shown (the field is cleanly absent, not shown as null).

4. **Given** packets in multiple status states exist (ACTIVE, SCHEDULED, PENDING, COMPLETED, ABANDONED), **When** the Packet Board renders, **Then** cards are grouped into labeled sections: "Active", "Queued" (SCHEDULED + PENDING), and "Done" (COMPLETED, PARTIAL_COMPLETED, ABANDONED, FAILED).

5. **Given** the "Active" and "Queued" groups have packets, **When** the page loads, **Then** both groups are expanded by default and the "Done" group is collapsed by default.

6. **Given** a packet has a fill gauge, **When** the fill percentage is above 80%, **Then** the gauge is colored green; between 40–80% it is amber; below 40% it is red.

7. **Given** a packet's deadline has passed, **When** the card is displayed, **Then** the deadline field shows "OVERDUE" in a red indicator rather than a countdown.

8. **Given** the engineer expands a packet card, **When** the expanded view renders, **Then** all deadline tiers for that packet are listed in a table showing: tier index, deadline, minimum required completion percentage, and maximum total cost (if set).

9. **Given** a packet with status ABANDONED or FAILED is in the "Done" group, **When** its card is displayed, **Then** the card shows which tier was active when the packet was abandoned and the final fill percentage at that point.

10. **Given** new packet data arrives via polling, **When** the Packet Board refreshes, **Then** cards update their fill gauges and status chips in place without layout shift.

---

### User Story 3 - Identify What Triggered a Replan (Priority: P3)

An engineer suspects that a rate change from the VTN caused the planner to rerun unexpectedly. They open the Planner page and look at the Trigger Timeline. They see a horizontal strip of event chips ordered by time, with a blue "RateChange" chip immediately followed by a colored "PlanCycle" chip. They click the RateChange chip and see the old and new tariff values in a popover.

**Why this priority**: Understanding causation (rate change → replan → schedule shift) is essential for diagnosing system behavior during anomalous periods. It is lower priority than the core Decision Matrix because it requires context from multiple events rather than being self-explanatory.

**Independent Test**: Deploy only the Trigger Timeline section. An engineer can open it, see the sequence of controller events, and identify the cause-and-effect chain that led to the latest plan cycle. Delivers controller transparency as a standalone feature.

**Acceptance Scenarios**:

1. **Given** the Planner page is open and controller events exist, **When** the Trigger Timeline renders, **Then** a horizontal scrollable strip shows event chips ordered chronologically with the newest chip on the right, and the strip is auto-scrolled to the rightmost (newest) end.

2. **Given** the Trigger Timeline is visible, **When** the engineer reads the chip labels, **Then** each chip type is visually distinct: PlanCycle chips use filled circles colored by their trigger reason, RateChange and CapacityChange chips use diamond shapes, OpenAdrArrived chips use a star shape, and PacketTransition chips use an arrow shape.

3. **Given** the Trigger Timeline is visible, **When** the engineer clicks any chip, **Then** a popover appears showing the full detail of that event (timestamp, event type, and all available fields for that event type).

4. **Given** a RateChange event is followed within 2 seconds by a PlanCycle event, **When** both chips are visible in the timeline, **Then** they are visually grouped or nudged together to suggest a causal relationship.

5. **Given** the controller event history is empty (no events yet), **When** the Trigger Timeline renders, **Then** an informative empty state is shown ("No controller events recorded yet").

6. **Given** new controller events arrive via polling, **When** the timeline refreshes, **Then** new chips appear on the right end of the strip without resetting the scroll position to the left.

---

### User Story 4 - See the Current Plan Summary and Warnings (Priority: P4)

An engineer opens the Planner page and immediately wants a quick read on the state of the current plan: when it last ran, what triggered it, what the committed cost is, and whether there are any problems. The Plan Header section shows this at a glance. A warning badge is visible. The engineer expands it to see a CRITICAL-severity warning about an energy packet that cannot be completed within its budget.

**Why this priority**: A compact summary serves as the "health check" that users read before diving into the matrix or board. It provides essential context (trigger type, staleness, cost) without requiring the user to parse the full matrix. Lower priority because the matrix and board provide more actionable detail.

**Independent Test**: Deploy only the Plan Header section. An engineer can open the Planner page and immediately see when the plan was last computed, what triggered it, and the committed cost. Delivers quick-status transparency as a standalone feature.

**Acceptance Scenarios**:

1. **Given** the Planner page is open and a plan exists, **When** the Plan Header renders, **Then** it displays: the plan trigger type (as a color-coded badge), the relative age of the plan ("3s ago"), total committed cost in €, total planned import in kWh, total estimated CO₂ in kg, and a warning count badge (if warnings exist).

2. **Given** trigger type badges exist, **When** the Plan Header displays a trigger type, **Then** the badge color reflects the trigger type: Periodic → gray, RateChange → blue, CapacityChange → orange, UserRequest → purple, Event → teal.

3. **Given** the plan has one or more warnings, **When** the Plan Header renders, **Then** a warning badge showing the count is visible and the warnings list is collapsed by default.

4. **Given** the warning list is collapsed, **When** the engineer clicks to expand it, **Then** each warning is shown with its severity chip (INFO / WARNING / CRITICAL), the warning message text, and any suggested action text.

5. **Given** the plan has no warnings, **When** the Plan Header renders, **Then** no warning badge or section is shown.

6. **Given** no plan has been generated yet, **When** the Plan Header renders, **Then** an informative message "No plan available" is shown instead of summary numbers.

7. **Given** a new plan is generated while the page is open, **When** the header refreshes (every 10 seconds), **Then** the trigger badge and age update automatically.

---

### Edge Cases

- What happens when the planner produces a plan with only FIRM slots and no FLEXIBLE slots (near-horizon planning only)? The FIRM/FLEX divider is placed at the end of the plan, and no faded columns are shown.
- What happens when an asset has zero steps in the plan (asset was inactive the entire horizon)? The row is still shown but all cells display as "Idle" (gray).
- What happens when the plan payload is very large (288 slots × 5 assets = 1440 cells)? The default FIRM-only view limits initial render to ~36 columns. Expanding to full horizon may be slow on low-powered devices — an accepted trade-off for full transparency.
- What happens when a packet's `deadline_tiers` list is empty? The card omits the deadline countdown entirely; no countdown or "OVERDUE" is shown.
- What happens when `accumulated_cost_eur` exceeds `max_total_cost_eur` for the active tier (budget overrun)? The budget bar is shown filled and overflowing with a red indicator, not capped at 100%.
- What happens when the controller trace ring buffer is empty (freshly started VEN)? The Trigger Timeline shows an empty state message.
- What happens when two events in the Trigger Timeline have identical timestamps? They are shown in the order they appear in the API response; no deduplication is applied.

---

## Requirements *(mandatory)*

### Functional Requirements

**Plan Header**

- **FR-001**: The Planner page MUST display a Plan Header section showing the plan's trigger type, age (relative timestamp), committed cost in €, planned import in kWh, and estimated CO₂ in kg.
- **FR-002**: The Plan Header MUST display a warning count badge when the current plan contains one or more warnings.
- **FR-003**: The Plan Header MUST allow the user to expand a collapsible warnings list that shows each warning's severity, message, and suggested action.
- **FR-004**: The Plan Header MUST show an empty state when no plan has been generated.

**Trigger Timeline**

- **FR-005**: The Planner page MUST display a horizontally scrollable Trigger Timeline showing the most recent controller events as chips ordered chronologically with newest on the right.
- **FR-006**: The Trigger Timeline MUST visually distinguish event types by chip shape and color (PlanCycle, RateChange, CapacityChange, OpenAdrArrived, OpenAdrExpired, PacketTransition each have distinct visual identity).
- **FR-007**: The Trigger Timeline MUST allow the user to click any chip and see the full event detail in a popover.
- **FR-008**: The Trigger Timeline MUST auto-scroll to the newest (rightmost) chip on initial render and on refresh.

**Decision Matrix**

- **FR-009**: The Planner page MUST display a Decision Matrix showing one row per asset and one column per planning time slot, with each cell colored and labeled by the planning reason that fired for that asset in that slot.
- **FR-010**: The Decision Matrix MUST display a tariff header row above the asset rows, with each column header colored on a green-to-red gradient reflecting the import tariff value for that slot.
- **FR-011**: The Decision Matrix MUST display a visible vertical divider separating the FIRM zone from the FLEXIBLE zone, with FLEXIBLE-zone cells rendered at reduced opacity.
- **FR-012**: The Decision Matrix MUST display only FIRM-zone columns by default (approximately the nearest 3 hours at 5-min resolution) and provide a control to expand to the full planning horizon.
- **FR-013**: The Decision Matrix MUST allow the user to click any cell and view a detail drawer showing the full slot decision: timestamp, asset ID, setpoint power, actual power, reason with parameters, asset capability, and state before the decision.
- **FR-014**: The Decision Matrix MUST display a persistent legend identifying the 12 planning reason types by color and icon.
- **FR-015**: The Decision Matrix MUST provide a collapse/expand control for the entire section to allow users to hide the heavy render when not needed.
- **FR-016**: The Decision Matrix MUST show an empty state when no plan is available.

**Packet Progress Board**

- **FR-017**: The Planner page MUST display a Packet Progress Board showing one card per energy packet, grouped into: Active (ACTIVE), Queued (PENDING, SCHEDULED), and Done (COMPLETED, PARTIAL_COMPLETED, ABANDONED, FAILED) sections.
- **FR-018**: Each packet card MUST display: asset label, status chip, fill percentage as a visual gauge bar with color coding (green > 80%, amber 40–80%, red < 40%), deadline countdown ("T−Xh Xm") or "OVERDUE" indicator, and target energy in kWh.
- **FR-019**: Each packet card MUST display a budget bar showing accumulated cost against the active tier's budget limit, when a budget limit is defined.
- **FR-020**: Each packet card MUST provide an expand control that reveals all deadline tiers as a table (tier index, deadline, minimum completion %, maximum cost).
- **FR-021**: Cards for ABANDONED and FAILED packets MUST display the tier that was active at termination and the final fill percentage.
- **FR-022**: The Active and Queued groups MUST be expanded by default; the Done group MUST be collapsed by default.
- **FR-023**: The Packet Board MUST show an empty state when no packets exist.

**Page-Level**

- **FR-024**: The Planner page MUST be accessible as a dedicated tab in the VEN Web UI navigation.
- **FR-025**: All four sections MUST refresh their data automatically on a polling interval of 10 seconds without requiring a page reload.
- **FR-026**: All four sections MUST update in place on each refresh — no full re-render that loses scroll position or expanded/collapsed state.

### Key Entities

- **Plan**: A single planning output generated by the scheduler. Contains a horizon (start/end time, step size), the FIRM/FLEX boundary timestamp, summary totals (cost, import energy, CO₂), a list of warnings with severity levels, and a list of per-slot per-asset decision steps.
- **Planning Slot**: One time interval in the plan horizon (e.g., 5 minutes). Has a type (FIRM or FLEXIBLE), a tariff value, a baseline load, and a list of asset allocations.
- **Planning Decision Step**: One decision record for one asset in one time slot. Captures the asset state before the decision, what the planner decided (setpoint power), what actually happened (actual power), and the reason that fired.
- **Planning Reason**: A categorized explanation for why the planner chose a setpoint. Has 12 variants: Idle, CheapTariff, ExpensiveTariff, FirmObligation, UserOverride, SocCeiling, SocFloor, ComfortBound, GridImportLimit, GridExportLimit, PolicyReserve, OpportunityMissed. Each variant carries numeric parameters (e.g., tariff value and threshold for CheapTariff).
- **Energy Packet**: A discrete energy delivery task assigned to one asset. Has a status (PENDING through FAILED), a target energy amount, a current fill level (estimated completion), one or more deadline tiers each with a minimum completion target and optional budget limit, and accumulated cost tracking.
- **Controller Event**: A significant occurrence recorded in the controller's event log. Types include: PlanCycle (plan was recalculated), RateChange (tariff updated), CapacityChange (grid limit changed), OpenAdrArrived (event received from VTN), OpenAdrExpired (event ended), PacketTransition (packet changed status).

---

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: An engineer can identify the planning reason for any specific asset-slot combination within 30 seconds of opening the Planner page, without consulting any external documentation or other pages.
- **SC-002**: An operator can determine whether an active energy packet is on track to meet its nearest deadline within 15 seconds of opening the Planner page.
- **SC-003**: An engineer can trace the cause-and-effect chain from a controller event (e.g., RateChange) to the resulting plan cycle within 30 seconds using only the Trigger Timeline.
- **SC-004**: All 12 planning reason types are visually distinguishable from each other in the Decision Matrix without relying on color alone (each has a unique icon or label).
- **SC-005**: The Planner page loads and displays meaningful content within 3 seconds on the Pi4 ARM64 deployment target, assuming normal plan payload size (≤300 decision steps).
- **SC-006**: All four sections remain functional and display correct data when the planner has not yet generated a plan (each section shows an appropriate empty state rather than an error).
- **SC-007**: Expanding a Decision Matrix cell drawer and reading the reason parameters takes fewer than 3 interactions (click cell → drawer opens → data visible, no further navigation required).
- **SC-008**: 100% of introduced UI behaviors — section rendering, cell clicks, drawer content, empty states, grouping, fill colors, budget bars, deadline countdowns — are covered by automated tests (BDD scenarios or unit tests).

---

## Assumptions

- No backend API changes are required. All data is available via existing endpoints: `GET /plan` (full, with steps), `GET /packets`, `GET /trace/events`.
- The planner produces decision steps for all assets across all time slots in the plan. If a step is absent for an asset-slot combination, it is treated as "Idle" by the UI.
- Planning time slot granularity is 5 minutes (configurable in profile), producing approximately 36 FIRM slots (3 hours) and 252 FLEXIBLE slots (21 hours) for a 24-hour horizon.
- The VEN UI is accessed on desktop browsers only; mobile responsiveness is not a requirement for this feature.
- Data polling at 10-second intervals is sufficient freshness for this diagnostic view. Real-time push is not required.
- A maximum of 5 assets per VEN is assumed for layout purposes; the matrix design handles more but is not optimized for it.
