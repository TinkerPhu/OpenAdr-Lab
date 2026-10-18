## ADDED Requirements

### Requirement: Chart and grid edit one draft
Pointer edits on the chart SHALL change the same draft the grid shows; a change in either SHALL
appear in the other without a separate state.

#### Scenario: Drag updates the grid
- **WHEN** the user drags the 17:00 step from 0.35 to 0.40
- **THEN** the grid cell for 17:00 shows 0.40

### Requirement: Snapping and readout
Dragging SHALL snap values to the registry's step and times to the registry's interval grid and
show the value being set.

#### Scenario: Snap
- **WHEN** the registry step is 0.01 and the pointer is at 0.3047
- **THEN** the draft value becomes 0.30 and the readout shows 0.30

### Requirement: Edits to a repeating pattern edit the pattern
With a recurring series, a drag on any repetition SHALL edit the single pattern interval it
represents, and all repetitions SHALL update.

#### Scenario: Drag on the second day
- **WHEN** a "forever" daily series is shown over two days and the user drags a step on day 2
- **THEN** the same step on day 1 changes too and the draft still holds 24 intervals

### Requirement: Operators are defined once
Scale, shift, cap, add-range and template application SHALL each be one pure function on the
draft, used by both the toolbar and any other caller, and SHALL leave the recurrence unchanged.

#### Scenario: Scale
- **WHEN** the user applies "scale ×1.2" to a draft
- **THEN** every value is multiplied by 1.2, rounded to the registry step, and the recurrence
  is unchanged

#### Scenario: Shift across the pattern end
- **WHEN** a repeating daily draft is shifted by +2 h
- **THEN** intervals wrap within the pattern period so the pattern stays contiguous
