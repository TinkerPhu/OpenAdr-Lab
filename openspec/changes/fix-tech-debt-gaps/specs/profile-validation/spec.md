## ADDED Requirements

### Requirement: Profile is validated on startup before any task is spawned
The VEN SHALL call `Profile::validate()` immediately after loading the YAML profile. If any invariant is violated, the process SHALL exit with a non-zero status code and print all violations to stderr before spawning any background task.

#### Scenario: Valid profile starts normally
- **WHEN** the YAML profile passes all validation checks
- **THEN** the VEN starts normally and spawns all background tasks

#### Scenario: Invalid profile exits with all errors listed
- **WHEN** the YAML profile contains one or more invalid values
- **THEN** the VEN prints all violated invariants to stderr
- **AND** the VEN exits with a non-zero status code
- **AND** no background tasks are started

#### Scenario: Multiple violations reported at once
- **WHEN** the profile has three separate invalid fields
- **THEN** all three violations are reported in a single startup-failure message
- **AND** the user does not need to restart multiple times to discover all problems

### Requirement: Absorber asset IDs must reference declared assets
Every asset ID in `absorber.assets[].id` SHALL match an asset `id` declared in `assets:`.

#### Scenario: Absorber references unknown asset
- **WHEN** the profile declares an absorber asset with id `"ev"` but no asset with id `"ev"` exists in `assets:`
- **THEN** validation fails with the message: `absorber asset "ev" not found in assets`

### Requirement: Numeric profile fields must be within valid bounds
The following field constraints SHALL be enforced:
- `ev.soc_target` ∈ [0.0, 1.0]
- `battery.min_soc` ∈ [0.0, 1.0)
- `battery.round_trip_efficiency` ∈ (0.0, 1.0]
- `planner.replan_interval_s` > 0
- `planner.phase2_epsilon_eur` ≥ 0.0
- `absorber.dead_band_kw` ≥ 0.0
- `ev.max_discharge_kw` ≥ 0.0

#### Scenario: soc_target out of range
- **WHEN** `ev.soc_target` is set to `1.5`
- **THEN** validation fails with a message indicating `soc_target` must be in [0.0, 1.0]

#### Scenario: efficiency out of range
- **WHEN** `battery.round_trip_efficiency` is set to `0.0`
- **THEN** validation fails with a message indicating `round_trip_efficiency` must be > 0.0

### Requirement: At least one asset must be declared
The profile SHALL declare at least one asset in `assets:`.

#### Scenario: Empty asset list
- **WHEN** the profile has an empty `assets:` list
- **THEN** validation fails with the message: `profile must declare at least one asset`
