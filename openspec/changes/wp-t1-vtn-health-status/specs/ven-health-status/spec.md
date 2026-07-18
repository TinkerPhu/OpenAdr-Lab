## ADDED Requirements

### Requirement: Health endpoint reports componentised status
`GET /health` SHALL return a JSON object `{status, components}` where `components`
has keys `ven_process`, `vtn_connection`, `storage`, and `planner`, each with at
least a `status` of `"ok"` or `"degraded"`. The top-level `status` SHALL equal
`"degraded"` if any component is degraded, else `"ok"`. The HTTP status code SHALL
be 200 whenever the VEN process itself is responsive, regardless of the other
components' status.

#### Scenario: Healthy VEN reports all components ok
- **WHEN** the VTN poll loop's last attempt succeeded and the last state-persist
  write succeeded and the active plan (if any) is not infeasible
- **THEN** `GET /health` returns `status: "ok"` with all four components `"ok"`
- **AND** the HTTP status code is 200

#### Scenario: VTN outage is reported without failing the healthcheck
- **WHEN** the VTN poll loop's last attempt failed and is currently backing off
- **THEN** `GET /health` returns `status: "degraded"` with `components.vtn_connection.status == "degraded"` and a `detail` string
- **AND** the HTTP status code is still 200

#### Scenario: Infeasible active plan degrades the planner component
- **WHEN** the active plan's `solve_status` is `INFEASIBLE`
- **THEN** `GET /health` returns `components.planner.status == "degraded"`

#### Scenario: No active plan yet is not a degraded state
- **WHEN** no plan has been adopted yet (VEN just started)
- **THEN** `GET /health` returns `components.planner.status == "ok"`

### Requirement: VTN connection detail is available on a dedicated endpoint
`GET /vtn/status` SHALL return `{connected, last_success_ts, last_error,
current_backoff_s, token_expires_at}` reflecting the VTN poll loop's current
reachability state and the VEN's OAuth token expiry, without requiring a new poll or
blocking on network I/O.

#### Scenario: Connected VEN reports last success and no error
- **WHEN** the VTN poll loop's most recent attempt succeeded
- **THEN** `GET /vtn/status` returns `connected: true`, a `last_success_ts` matching
  that attempt, and `last_error: null`

#### Scenario: Disconnected VEN reports backoff and last error detail
- **WHEN** the VTN poll loop's most recent attempt failed
- **THEN** `GET /vtn/status` returns `connected: false`, a non-null `last_error`, and
  `current_backoff_s` reflecting the loop's current retry delay

#### Scenario: Token expiry is reported as a wall-clock timestamp
- **WHEN** the VEN holds a valid OAuth token acquired with a known `expires_in`
- **THEN** `GET /vtn/status`'s `token_expires_at` is a UTC timestamp approximately
  `expires_in` seconds after the token was acquired
