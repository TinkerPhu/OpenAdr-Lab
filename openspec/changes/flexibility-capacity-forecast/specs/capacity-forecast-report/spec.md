## ADDED Requirements

### Requirement: Capacity curve exposed via existing OpenADR report payload types
The system SHALL expose the per-direction capacity curve as an OpenADR 3.1 report, using an
existing standard report payload type (chosen from `STORAGE_MAX_DISCHARGE_POWER`,
`STORAGE_MAX_CHARGE_POWER`, `UP_REGULATION_AVAILABLE`, `DOWN_REGULATION_AVAILABLE`) with
`readingType: FORECAST`, one report interval per curve step point. The system SHALL NOT define a
new, non-standard payload type for this purpose.

#### Scenario: VTN requests a forecast capacity report
- **WHEN** a `reportDescriptor` with `historical: false` and a payload type from the approved set
  is active for a VEN
- **THEN** the VEN's report contains one interval per curve step, each interval's payload value
  equal to the curve's achievable power at that elapsed time, with `readingType: FORECAST`

### Requirement: Reuses existing payload-type dispatch mechanism
The system SHALL add the new payload type(s) as additional match arms in the existing
obligation-driven report dispatch (`controller/reporter.rs`), following the same pattern already
used for `IMPORT_CAPACITY_LIMIT`/`EXPORT_CAPACITY_LIMIT`/`USAGE_FORECAST`, rather than introducing
a parallel reporting pathway.

#### Scenario: New payload type follows the existing forecast-obligation path
- **WHEN** an obligation's `payload_type` matches one of the new capacity payload types and
  `historical` is false
- **THEN** the VEN builds report intervals via a dedicated builder function sourcing from the
  capacity-curve computation, reached through the same `payload_type` match dispatch as existing
  forecast payload types

### Requirement: Curve computable at an arbitrary start time
The capacity-curve computation SHALL accept a start time parameter so that a report requested with
a future `startInterval` produces a curve anchored at that future time rather than only "now." No
stored multi-start-time surface SHALL be built or persisted.

#### Scenario: Report requested for a future start
- **WHEN** a `reportDescriptor.startInterval` indicates a future offset
- **THEN** the VEN computes the capacity curve freshly, anchored at that future start time, using
  the best currently-available forecast state — it does not look up a precomputed surface
