## ADDED Requirements

### Requirement: Every adopted plan is stored
The BFF SHALL store each plan it receives on a VEN's `plan` topic, keyed by VEN and plan id, with the
time the BFF received it, and SHALL keep it for a configured retention. A message it cannot read SHALL
cost only itself.

#### Scenario: Plan stored
- **WHEN** a VEN publishes a plan
- **THEN** the plan can be read back by `GET /api/fleet/plan` for any instant after it was adopted

#### Scenario: Unreadable message
- **WHEN** a `plan` message is not valid JSON
- **THEN** it is counted and skipped and later plans are stored normally

### Requirement: The fleet plan can be read as of an instant
`GET /api/fleet/plan?at=<instant>` SHALL, for each VEN, use the latest plan adopted at or before
`at`, resample it to a common grid with the shared resampler, and return planned fleet import and
export power per slot with the number of VENs contributing, and which plan each VEN contributed.
`at` defaults to now. A VEN with no plan yet contributes nothing and is listed as such, never as zero.

#### Scenario: As of now
- **WHEN** two VENs have each adopted a plan
- **THEN** each slot's fleet import equals the sum of the two VENs' planned import in that slot

#### Scenario: As of an earlier instant
- **WHEN** a VEN adopted plan A at 10:00 and plan B at 11:00 and `at` is 10:30
- **THEN** its contribution is plan A

#### Scenario: A VEN without a plan
- **WHEN** a known VEN has published no plan
- **THEN** it is named under "no plan" and the contributing count excludes it

### Requirement: The same function answers before and after
"As of an earlier instant" and "now" SHALL be computed by the same function, so a before/after
comparison cannot disagree with itself about how plans are summed.

#### Scenario: Equal inputs, equal answers
- **WHEN** `at` is an instant after the last plan was adopted
- **THEN** the answer equals the answer for now

### Requirement: The planned fleet is visible in the VTN UI
The Fleet page SHALL show planned fleet import and export power over the plan horizon, the number of
VENs contributing, and when the newest contributing plan was adopted.

#### Scenario: Card shown
- **WHEN** the BFF has plans for the fleet
- **THEN** the Fleet page shows a planned-import and a planned-export series and "n of m VENs"

#### Scenario: Store unavailable
- **WHEN** the telemetry store is not connected
- **THEN** the card says there is no plan history, as the other fleet cards do
