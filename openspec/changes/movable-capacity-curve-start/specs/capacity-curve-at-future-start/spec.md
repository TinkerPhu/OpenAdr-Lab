## ADDED Requirements

### Requirement: Capacity curves can be requested for a future commitment start
`GET /flexibility/capacity` SHALL accept an optional `start` query parameter (RFC 3339). When
`start` is later than now and an active plan exists, the VEN SHALL return both sustained-commitment
curves (`import`, `export`) anchored at the plan slot boundary `start` snaps down to. The curves
SHALL start from each asset's plan-forecasted state at that boundary, as returned by
`resolve_plan_state_at`, and SHALL sweep to the active plan's horizon end. The response SHALL
include a top-level `start` field holding the snapped start actually used, and each curve's own
`start` SHALL equal it.

#### Scenario: Future start inside a plan slot snaps down to the slot boundary
- **WHEN** a client requests `GET /flexibility/capacity?start=<ts>` where `<ts>` lies strictly
  between two remaining plan slot starts
- **THEN** the response's `start`, `import.start` and `export.start` all equal the earlier of
  those two slot starts

#### Scenario: Future start uses the plan-forecasted state, not the live state
- **WHEN** the active plan charges the battery during the slots before the requested start
- **THEN** the import curve anchored at that start reports a shorter full-power import
  duration than the import curve anchored at now

#### Scenario: Future start's horizon ends at the plan horizon
- **WHEN** a client requests curves for a future start
- **THEN** no curve step's `elapsed_s` exceeds the active plan's `horizon.end_time` minus the
  returned `start`

### Requirement: Requests without a future start keep today's behavior
When `start` is absent or not later than now, `GET /flexibility/capacity` SHALL return the
curves computed by the latest sim tick without any additional computation. The response
SHALL contain `import` and `export` unchanged from before this change, plus the top-level `start`
set to those curves' own start. It SHALL still return 204 before the first dispatcher tick.

#### Scenario: No start parameter
- **WHEN** a client requests `GET /flexibility/capacity` without `start`
- **THEN** the `import` and `export` curves equal the per-tick stored curves

#### Scenario: Start in the past
- **WHEN** a client requests `GET /flexibility/capacity?start=<ts>` with `<ts>` earlier than now
- **THEN** the response equals the response without `start`

### Requirement: Future start degrades explicitly when it cannot be honored
The VEN SHALL NOT fail a well-formed future-start request because no plan covers it. Without an
active plan, it SHALL return the per-tick now-anchored curves with `start` set to their own start.
A `start` past the last remaining plan slot's start SHALL be clamped to that slot's start. An
unparsable `start` SHALL be rejected with HTTP 400.

#### Scenario: No active plan
- **WHEN** no active plan exists and a client requests a future `start`
- **THEN** the response carries the now-anchored curves, and its `start` differs from the
  requested value

#### Scenario: Start beyond the plan's last slot
- **WHEN** a client requests a `start` later than the last remaining plan slot's start
- **THEN** the response's `start` equals the last remaining plan slot's start

#### Scenario: Malformed start
- **WHEN** a client requests `GET /flexibility/capacity?start=not-a-time`
- **THEN** the VEN responds with HTTP 400

### Requirement: A future-start curve touches the headroom band at its start
For every asset kind, the first step of a curve anchored at a plan slot SHALL equal the Site
Headroom forecast's value for that same slot in the same direction: the import curve's first
`power_kw` equals the slot's `down_kw`, and the export curve's first `power_kw` equals the slot's
`up_kw`. This extends to every future slot the seam invariant that already holds at now.

#### Scenario: Seam at a future slot
- **WHEN** curves are computed for a future slot's start and the Site Headroom forecast is
  computed from the same simulator state and plan
- **THEN** `import.steps[0].power_kw` equals that slot's `down_kw` and `export.steps[0].power_kw`
  equals that slot's `up_kw`

### Requirement: The now-anchored and future-anchored curves share one computation
The capacity-curve engine SHALL compute now-anchored and future-anchored curves through the same
core function. The two paths SHALL differ only in the per-asset starting states and the start
time passed in. For `t1 = now` with live states, the core SHALL produce output identical to the
per-tick curves.

#### Scenario: Core with live states reproduces the per-tick curve
- **WHEN** the core is called with `t1 = now`, the live per-asset states, and the per-tick horizon
- **THEN** its output equals `compute_site_capacity_curve`'s output for the same inputs

### Requirement: Site Headroom chart offers a "Move commitment start" cursor mode
The Controller's Site Headroom chart SHALL offer a cursor mode switch with the options "Values"
(default, the existing tooltip-only behavior) and "Move commitment start". In "Move commitment
start" mode, hovering a future time SHALL redraw the dashed Import/Export commitment curves
anchored at the plan slot that time snaps to. Hovering at or before now, off the chart, or with no
remaining plan slots SHALL show the now-anchored curves. The chart SHALL mark the anchored start
with a vertical reference line. It SHALL show a caption naming the start actually used, as
reported by the server, including when the server fell back to now.

#### Scenario: Hover moves the curves
- **WHEN** the user selects "Move commitment start" and hovers a time 2 h in the future
- **THEN** the dashed curves begin at the plan slot containing that time, and the caption shows
  that slot's start time

#### Scenario: Default mode unchanged
- **WHEN** the chart is shown with the default "Values" mode
- **THEN** the dashed curves begin at now regardless of cursor position

#### Scenario: Hover left of now
- **WHEN** in "Move commitment start" mode the user hovers a past time
- **THEN** the dashed curves begin at now

### Requirement: Double-click pins the commitment start
In "Move commitment start" mode, a double-click (or a tap on touch devices) SHALL pin the curves
at the snapped slot, so further hovering does not move them. A second double-click, or switching
back to "Values", SHALL unpin them. While pinned, the curves SHALL refresh at the same interval as
the now-anchored curves.

#### Scenario: Pin and unpin
- **WHEN** the user double-clicks a future time, moves the cursor elsewhere, then double-clicks again
- **THEN** the curves stay at the first slot after the first double-click, and follow the cursor
  again after the second

### Requirement: Future-start requests are bounded by cursor rest, and only the server snaps
The UI SHALL NOT issue a future-start request while the cursor is moving; it SHALL issue one
request once the cursor has rested on a future time for the hover debounce, carrying that time
unsnapped. Only the server SHALL decide which plan slot a time falls in; the UI SHALL render the
`start` the server returns.

#### Scenario: Sweeping across the chart
- **WHEN** the cursor moves continuously and then rests on a future time
- **THEN** no future-start request is issued while it moves, and exactly one is issued for the
  time it rests on
