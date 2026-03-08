## ADDED Requirements

### Requirement: VEN UI Programs page uses flat targets display
The VEN UI Programs page SHALL display a program's `targets` as a flat list of strings.
The 3.0 `{type, values}` representation MUST NOT appear in the UI.

#### Scenario: Programs page shows flat target strings
- **WHEN** the user navigates to Programs in the VEN UI
- **THEN** any program with targets shows them as a list of plain strings (e.g., `ven-1-client`)

### Requirement: VEN UI Reports page omits programId
The VEN UI Reports page SHALL display report entries without a `programId` field.
It SHALL display `eventID` instead.

#### Scenario: Reports page shows eventID not programId
- **WHEN** the user navigates to Reports in the VEN UI
- **THEN** each report entry shows `eventID`
- **AND** no `programId` field is shown

### Requirement: VEN UI Simulation page is updated for redesigned simulator
The VEN UI Simulation page SHALL display device state from the redesigned 3.1 simulator.
The `GET /sim` endpoint structure may change; the UI SHALL adapt to the new response shape.

#### Scenario: Simulation page renders device state from 3.1 simulator
- **WHEN** the user opens the Simulation page
- **THEN** current device power values are displayed
- **AND** the page does not error if `POST /sim/override` is absent

### Requirement: VEN UI TypeScript types match 3.1 wire format
All TypeScript types in `VEN/ui/src/api/hooks.ts` and related files SHALL reflect the 3.1
wire format: `targets: string[]`, no `programId` in reports, `clientID` on VEN objects.

#### Scenario: No TypeScript compile errors after 3.1 type updates
- **WHEN** `npm run build` is executed in `VEN/ui/`
- **THEN** the build succeeds with no TypeScript errors
