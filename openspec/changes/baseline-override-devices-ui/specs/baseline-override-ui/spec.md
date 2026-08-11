## ADDED Requirements

### Requirement: Devices page shows a baseline override control
The VEN UI Devices page SHALL render a `BaselineOverrideCard` control, alongside the existing per-device
cards, that surfaces the backend `/baseline-override` capability (GET/POST/DELETE) to the user.

#### Scenario: Baseline override card is visible on the Devices page
- **WHEN** a user navigates to the Devices page
- **THEN** an element with a baseline-override card testid is visible on the page

### Requirement: Card displays the currently active override
The card SHALL fetch the active baseline override via the existing `useBaselineOverride` hook and
display its per-slot `slot_start` / `add_kw` rows when one is active, or an empty-state message when
none is active (`GET /baseline-override` returned 204 / `null`).

#### Scenario: No active override shows an empty state
- **WHEN** `GET /baseline-override` returns no active override
- **THEN** the card shows a message indicating no baseline override is active
- **AND** the clear/delete action is disabled

#### Scenario: Active override renders its slots
- **WHEN** `GET /baseline-override` returns an override with one or more slots
- **THEN** the card renders one row per slot showing that slot's `slot_start` and `add_kw` values

### Requirement: User can edit, add, and remove slot rows before saving
The card SHALL let the user add a new blank row, edit an existing row's `slot_start` and `add_kw`
fields, and remove a row, entirely in local component state, without calling the backend until Save is
pressed.

#### Scenario: Adding a row
- **WHEN** the user clicks the add-row action
- **THEN** a new editable row appears in the card's row list
- **AND** no network request has been made yet

#### Scenario: Removing a row
- **WHEN** the user clicks the remove action on a row
- **THEN** that row is removed from the card's row list
- **AND** no network request has been made yet

### Requirement: Save persists edited slots via POST /baseline-override
The card SHALL call `usePostBaselineOverride` with the current edited row set, using the unmodified
`slot_start` / `add_kw` field names (per the project's no-DTO-normalization convention), when the user
presses Save, and SHALL disable Save while the request is pending or while there are zero rows.

#### Scenario: Saving edited slots
- **WHEN** the user has edited or added at least one row and clicks Save
- **THEN** `postBaselineOverride` is called with a body whose `slots` array matches the currently
  displayed rows' `slot_start` and `add_kw` values
- **AND** on success the card reflects the newly active override returned by the request

#### Scenario: Save is disabled with no rows
- **WHEN** the card's row list is empty
- **THEN** the Save action is disabled

### Requirement: Clear removes the active override via DELETE /baseline-override
The card SHALL call `useDeleteBaselineOverride` when the user presses the clear/delete action, and
SHALL disable that action when there is no active override or while the request is pending.

#### Scenario: Clearing an active override
- **WHEN** an override is active and the user clicks the clear action
- **THEN** `deleteBaselineOverride` is called
- **AND** on success the card shows the empty state (no active override)

### Requirement: BDD coverage of the Devices page baseline-override control
A BDD scenario SHALL exercise the baseline-override control on the live Devices page, following this
project's testid-driven `@ven-ui` step convention.

#### Scenario: End-to-end baseline override set and clear via the UI
- **WHEN** a user opens the VEN UI, navigates to the Devices page, adds a slot row with a `slot_start`
  and `add_kw`, and saves
- **THEN** the card shows the saved slot as part of the active override
- **WHEN** the user then clicks clear
- **THEN** the card returns to the no-active-override empty state
