## ADDED Requirements

### Requirement: Signal types are declared in a registry
The workbench SHALL take everything signal-specific — payload type, unit, currency, sign
convention, interval grid, value range, value kind — from one registry entry per signal type,
and SHALL contain no per-signal-type branching in its editor, validator or publisher. This
change registers `PRICE` and `EXPORT_PRICE`.

#### Scenario: Registry drives the grid
- **WHEN** the user picks `EXPORT_PRICE`
- **THEN** the column headers, unit and snapping step come from its registry entry

#### Scenario: Unknown type
- **WHEN** an event loaded into the draft carries a payload type that is not registered
- **THEN** the workbench refuses to edit it and names the type

### Requirement: Edits happen on a draft, not on the live event
All edits SHALL go to a draft held in the UI. The live tariff SHALL stay unchanged and visible
until the user publishes.

#### Scenario: Draft overlay
- **WHEN** the user changes a price in the grid
- **THEN** the preview draws the draft next to the live tariff and the live event on the VTN is
  unchanged

#### Scenario: Discard
- **WHEN** the user discards the draft
- **THEN** the grid returns to the live values and nothing is sent

### Requirement: Grid editing
The grid SHALL provide one row per interval with start, end and a value per registered signal,
cell editing, pasting a block of rows from tab- or comma-separated text, fill-down and "set
selection to value".

#### Scenario: Paste a day
- **WHEN** the user pastes 24 lines of one number each into the first value cell
- **THEN** 24 consecutive rows are filled and the preview updates

#### Scenario: Rejected cell
- **WHEN** a value is not a number or is outside the registered range
- **THEN** the cell is marked invalid and publishing is blocked

### Requirement: Published events declare their contract
Publishing SHALL create an event whose payload descriptors declare the registry's payload type,
unit and currency, and whose priority is higher than the superseded event's. The publisher SHALL
be unable to build an event for a registered type without those descriptors.

#### Scenario: Descriptor present
- **WHEN** a `PRICE` draft is published
- **THEN** the created event declares `payloadType = PRICE`, units `KWH` and currency `EUR`

#### Scenario: Supersede, not overwrite
- **WHEN** a draft loaded from event E is published
- **THEN** a new event with a higher priority than E is created and E is left unchanged

### Requirement: Targeting is shown
The workbench SHALL state which VENs the published event will reach and SHALL warn when the
fleet currently disagrees about price, using the existing shared-tariff dissent logic.

#### Scenario: Dissenting VENs
- **WHEN** two VENs currently receive different price bands
- **THEN** the workbench names them before a draft is loaded from the shared tariff

### Requirement: Fleet reaction is observable from the workbench
After publishing, the workbench SHALL show the fleet's planned import and export power so the
user can watch plans change as VENs replan.

#### Scenario: Publish and observe
- **WHEN** the user publishes a draft
- **THEN** the page shows the tariff chart and the fleet power chart on the same time axis
