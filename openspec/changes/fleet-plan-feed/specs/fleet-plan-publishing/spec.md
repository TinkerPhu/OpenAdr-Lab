## ADDED Requirements

### Requirement: An adopted plan is published
When a plan is adopted, the VEN SHALL publish a projection of it on `openadr-lab/fleet/<ven>/plan`,
retained, at QoS 1. A plan the acceptance gate rejects SHALL NOT be published.

#### Scenario: Adoption publishes
- **WHEN** the planner produces a plan and the acceptance gate adopts it
- **THEN** one message appears on the VEN's `plan` topic carrying that plan's id

#### Scenario: Rejection is silent
- **WHEN** the acceptance gate rejects a newly solved plan
- **THEN** nothing is published and the previously published plan stays the retained one

### Requirement: The payload reuses the plan's own field names
The message SHALL carry `id`, `created_at`, `trigger` and, for every slot, `start`, `end`,
`net_import_kw`, `net_export_kw`, `import_tariff_eur_kwh` and `export_tariff_eur_kwh`, with the names
and units `GET /plan` uses, and SHALL NOT rename or convert any of them.

#### Scenario: Same values as the plan
- **WHEN** a plan is published
- **THEN** each slot's published `net_import_kw` equals the same slot's value in `GET /plan`

### Requirement: Publishing never blocks or fails planning
A broker that is down, slow or unconfigured SHALL NOT delay plan adoption, change its outcome, or
raise an error to the planner. With `FLEET_MQTT_HOST` unset the publish is a no-op.

#### Scenario: Broker away
- **WHEN** the broker is unreachable at adoption
- **THEN** the plan is adopted exactly as without a broker and the dropped publish is only logged

### Requirement: Power is stated in kW with its sign convention
`net_import_kw` and `net_export_kw` SHALL be non-negative kilowatts, import and export being separate
fields, as in the plan; the payload SHALL NOT contain a watt quantity.

#### Scenario: No negative or watt values
- **WHEN** a plan with both importing and exporting slots is published
- **THEN** every published `net_import_kw` and `net_export_kw` is >= 0 and in kW
