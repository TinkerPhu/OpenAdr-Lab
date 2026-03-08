## ADDED Requirements

### Requirement: Simulator module is redesigned for 3.1 event payloads
The VEN simulator (`VEN/src/simulator/`) SHALL be rewritten from scratch to accept 3.1
`EventInterval` payloads directly. The new design SHALL be simpler, more testable, and
decoupled from 3.0 TargetMap structures.

#### Scenario: Simulator processes SIMPLE signal from event interval
- **WHEN** an event interval contains a payload `{ "type": "SIMPLE", "values": [1.0] }`
- **THEN** the simulator updates its device setpoints accordingly

#### Scenario: Simulator state is accessible via GET /sim
- **WHEN** `GET /sim` is called on the VEN app
- **THEN** the response includes current device state (power values per device) and energy totals

#### Scenario: Simulator state persists across restart
- **WHEN** the VEN container is restarted
- **THEN** `GET /sim` returns device state consistent with the last persisted snapshot from `/data/sim_state.json`

### Requirement: Reactor FSM is redesigned for 3.1
The VEN reactor (`VEN/src/reactor/`) SHALL be rewritten with a clean FSM. The reactor
SHALL process 3.1 event intervals to determine device setpoints. The decision trace SHALL
remain accessible via `GET /trace`.

#### Scenario: Reactor transitions through ramp cycle
- **GIVEN** the reactor receives an active DR event with a SIMPLE signal
- **WHEN** the event interval begins
- **THEN** the reactor FSM transitions: Idle → Ramping → Holding
- **AND** when the event ends, the FSM transitions: Holding → RampingBack → Idle

#### Scenario: Decision trace records each state transition
- **WHEN** the reactor changes state
- **THEN** `GET /trace` returns an entry with timestamp, event_id, old_state, new_state, and reason

### Requirement: Per-VEN YAML profile includes client_id
Each profile in `VEN/profiles/ven-{1,2,3}.yaml` SHALL include a `client_id` field.
The simulator and reactor devices MAY remain configurable via the profile.

#### Scenario: Profile is loaded with client_id
- **WHEN** the VEN starts with `PROFILE_PATH=/profiles/ven-1.yaml`
- **THEN** the app reads `client_id` from the profile for OAuth authentication

### Requirement: VEN app does NOT expose POST /sim/override in this migration
The `POST /sim/override` endpoint SHALL be removed in the 3.1 redesign. It may be
re-added as a future improvement.

#### Scenario: Override endpoint returns 404
- **WHEN** `POST /sim/override` is called on a 3.1 VEN instance
- **THEN** the response is HTTP 404
