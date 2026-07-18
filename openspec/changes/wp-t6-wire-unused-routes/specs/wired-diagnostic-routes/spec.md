## ADDED Requirements

### Requirement: Controller page shows per-asset flexibility and forecast
The Controller page SHALL display, for each asset, its current feasible power
range and its predicted next power/confidence/source from the latest plan cycle.

#### Scenario: Capability and forecast render together per asset
- **WHEN** both `/capability/:asset_id` and `/forecast` data are available for
  an asset
- **THEN** the Controller page shows that asset's max import/export power and
  its predicted next power, confidence, and forecast source

#### Scenario: Missing data renders a placeholder, not an error
- **WHEN** capability or forecast data for an asset is not yet available
- **THEN** the corresponding field renders a placeholder dash rather than
  blocking the rest of the panel

### Requirement: History page shows plan snapshots for the selected day
The History page SHALL list plan snapshots created within the selected day's
window, with a way to view each snapshot's full plan JSON.

#### Scenario: Plan snapshots list for the selected day
- **WHEN** the History page's selected date has one or more plan snapshots
- **THEN** each renders with its created time and horizon start/end
- **AND** clicking a snapshot's detail control shows its full plan JSON

### Requirement: Reports page shows pending report obligations
The Reports page SHALL list report obligations with a computed status
(Pending, Overdue, or Fulfilled) derived from `due_at` and `fulfilled`.

#### Scenario: An unfulfilled, not-yet-due obligation is Pending
- **WHEN** an obligation has `fulfilled: false` and `due_at` in the future
- **THEN** it renders with status "Pending"

#### Scenario: An unfulfilled, past-due obligation is Overdue
- **WHEN** an obligation has `fulfilled: false` and `due_at` in the past
- **THEN** it renders with status "Overdue"
