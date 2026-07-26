## ADDED Requirements

### Requirement: PV profiles declare inverter AC output capability separately from panel peak
The PV profile schema SHALL support an `inverter_max_kw` parameter, distinct from `rated_kw` (installed DC panel peak), defaulting to `rated_kw` when unset. `inverter_max_kw` SHALL be greater than zero.

#### Scenario: Default preserves existing behavior
- **WHEN** a PV profile does not set `inverter_max_kw`
- **THEN** the resolved value equals `rated_kw` and PV physics behaves exactly as before this change

#### Scenario: An explicit inverter capability is respected
- **WHEN** a PV profile sets `inverter_max_kw` below `rated_kw`
- **THEN** the resolved value used everywhere PV output is computed is the configured `inverter_max_kw`, not `rated_kw`

#### Scenario: A non-positive inverter capability is rejected
- **WHEN** a PV profile sets `inverter_max_kw` to zero or a negative value
- **THEN** profile validation rejects it

### Requirement: PV output is physically capped at the inverter's AC capability
Simulated PV output and every forecast/planning input derived from irradiance and rated capacity SHALL be clamped to `inverter_max_kw` before any externally-commanded export limit is applied. `inverter_max_kw` SHALL be visible in PV's live state alongside `rated_kw`.

#### Scenario: DC potential exceeding inverter capability is clipped
- **WHEN** the modeled DC potential (irradiance × `rated_kw`, or the weather-sourced value) exceeds `inverter_max_kw`
- **THEN** simulated actual output does not exceed `inverter_max_kw`, with no commanded export limit involved

#### Scenario: A commanded limit at or above inverter capability has no additional effect
- **WHEN** a commanded export limit is set at or above `inverter_max_kw`
- **THEN** output is bounded by `inverter_max_kw`, not the looser commanded value, exactly as if no commanded limit were set

#### Scenario: Planner forecast respects the same ceiling
- **WHEN** the planner builds its PV forecast input for a VEN whose `inverter_max_kw` is below `rated_kw`
- **THEN** the forecast input for any slot does not exceed `inverter_max_kw`

### Requirement: The applied export limit and its source are recorded per tick, not from live config
Each tick's active export limit and its source — the plan's own target, a live capacity/VTN source, or neither — SHALL be determined at the moment the limit is resolved and attached to that tick's own state, so a later historical reconstruction of that tick reports what was actually active then, never the current configuration. "No limit" SHALL be represented as absence (no value), never as a sentinel number such as infinity or a value equal to `inverter_max_kw` — a present-but-loose limit (e.g. one at or above `inverter_max_kw`) is a distinct, still-recorded state from no limit being commanded at all. When both sources produce an equally tight limit, the source SHALL be tagged as the plan.

#### Scenario: A past tick's limit reflects what was active then, not the current value
- **WHEN** an export limit was active during a past tick and a different (or no) limit is active now
- **THEN** querying that past tick's recorded state shows the limit that was active during that tick, not the current one

#### Scenario: A loose commanded limit is still recorded, distinct from no limit at all
- **WHEN** a live capacity source commands an export limit at or above `inverter_max_kw` (a limit that has no physical effect)
- **THEN** the tick's recorded limit is that commanded value with source tagged as the live capacity source — not absent, and not conflated with a tick where no source commanded any limit

#### Scenario: The plan's own target is tagged as the source when it is strictly tighter
- **WHEN** the plan's curtailment target for the current slot is strictly tighter than any live capacity cap
- **THEN** the resolved limit's source is tagged as the plan

#### Scenario: A live capacity cap unanticipated by the plan is tagged as the source
- **WHEN** a live VTN/capacity export limit is strictly tighter than the plan's own target for the current slot (or the plan has no target at all)
- **THEN** the resolved limit's source is tagged as the live capacity source

#### Scenario: An equally tight limit from both sources is tagged as planned
- **WHEN** the plan's own target and a live capacity cap resolve to the same limit value
- **THEN** the resolved limit's source is tagged as the plan

### Requirement: Curtailment facts are persisted without losing brief unplanned events
The applied export limit and its source SHALL be sampled into long-term history, retrievable via the existing tick-history query path, subject to the existing retention policy. No derived potential-output value SHALL be persisted. Within a downsample window, the persisted source SHALL prefer the live capacity source over the plan source over no source at all, so a brief unplanned event within the window is never discarded in favor of a plan-sourced or unlimited majority.

#### Scenario: A limit active at any point in the window survives a restart
- **WHEN** an export limit was active at some point during a past downsample window, and the VEN is later restarted
- **THEN** querying history for that window still returns the recorded limit value and source

#### Scenario: An uncurtailed window has no limit recorded
- **WHEN** no export limit was active at any point during a downsample window
- **THEN** the persisted sample for that window has an absent (null) limit value and source — not
  zero, not `inverter_max_kw`, and not any other sentinel

#### Scenario: A brief unplanned event within a mostly-planned window is not discarded
- **WHEN** a downsample window contains both plan-sourced and live-capacity-sourced limits at different moments
- **THEN** the window's persisted source is the live capacity source, not the plan source

### Requirement: The Controller page's PV timeline visually distinguishes three curtailment states
The PV asset's timeline chart on the Controller page SHALL render visually distinct treatments for: no curtailment, hardware-capped output (output at `inverter_max_kw` with no commanded limit tighter than that ceiling), and imposed curtailment (a commanded limit below `inverter_max_kw` that is actually reducing output) — with imposed curtailment further distinguished as planned (past or future) or unplanned (past only).

#### Scenario: Hardware-capped output is shown neutrally, not as an alert
- **WHEN** output is at `inverter_max_kw` and no commanded limit is tighter than that ceiling (including when a commanded limit is set but at or above `inverter_max_kw`)
- **THEN** the chart renders this distinctly from imposed curtailment, not as an alerting color

#### Scenario: Planned imposed curtailment is shaded consistently across past and future
- **WHEN** a time range, past or future, shows imposed curtailment tagged as planned
- **THEN** the chart shades that range with the planned treatment

#### Scenario: Unplanned imposed curtailment is shaded distinctly, past only
- **WHEN** a past time range shows imposed curtailment tagged as unplanned
- **THEN** the chart shades that range with a treatment visually distinct from the planned treatment

#### Scenario: Uncurtailed, uncapped periods show no shading
- **WHEN** a time range has neither hardware-capped nor imposed curtailment
- **THEN** the chart renders no curtailment shading for that range
