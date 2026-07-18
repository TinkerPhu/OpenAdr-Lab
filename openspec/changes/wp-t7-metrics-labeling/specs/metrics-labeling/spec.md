## ADDED Requirements

### Requirement: Metrics are grouped under human-readable categories by default
The Metrics page SHALL group metrics into categories with human-readable labels
for known metric names, and SHALL fall back to an "Other" category using the raw
metric name for any metric not in the known set.

#### Scenario: Known VTN-polling metrics are labeled and grouped
- **WHEN** the Metrics page loads and `poll_success_total`/`poll_error_total` are
  present in the response
- **THEN** they render under a "VTN Polling" category heading with labels "Poll
  successes"/"Poll failures" instead of their raw names

#### Scenario: Known report metrics are labeled and grouped
- **WHEN** `reports_sent_total` is present
- **THEN** it renders under a "Reports" category heading with the label
  "Reports sent"

#### Scenario: An unrecognized metric is not hidden
- **WHEN** a metric name not in the known set is present
- **THEN** it renders under an "Other" category heading using its raw name as
  the label, not omitted from the page

### Requirement: A raw view reproduces the pre-grouping behavior exactly
The Metrics page SHALL offer a toggle that switches to a flat, ungrouped view
using raw metric names for every metric, with no category headings.

#### Scenario: Toggling to raw view removes grouping
- **WHEN** the raw-view toggle is switched on
- **THEN** no category headings are rendered
- **AND** every metric renders under its raw name, sorted alphabetically
