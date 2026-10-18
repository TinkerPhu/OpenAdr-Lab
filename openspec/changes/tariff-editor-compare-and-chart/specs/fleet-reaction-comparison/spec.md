## ADDED Requirements

### Requirement: Frozen reference at publish
When a draft is published, the workbench SHALL keep the fleet's import and export power over the
comparison window as it was planned just before the publish and draw it as a reference series.

#### Scenario: Reference after publish
- **WHEN** the user publishes a draft
- **THEN** the fleet power chart shows the pre-publish series faintly and the live series solid

### Requirement: Delta is shown once the fleet has replanned
The workbench SHALL show per-direction (import, export) energy and peak differences between the
reference and the live series, and SHALL say while VENs have not yet replanned that the delta is
not final.

#### Scenario: Not yet replanned
- **WHEN** fewer than all targeted VENs have a plan newer than the publish time
- **THEN** the delta is labelled "n of m VENs replanned"

#### Scenario: Settled
- **WHEN** all targeted VENs have replanned
- **THEN** the delta is shown without the provisional label

### Requirement: Reference survives a reload
The reference SHALL be recoverable after the page is reloaded from stored data, not only from
browser memory.

#### Scenario: Reload
- **WHEN** the page is reloaded after a publish
- **THEN** the same reference and publish time are shown
