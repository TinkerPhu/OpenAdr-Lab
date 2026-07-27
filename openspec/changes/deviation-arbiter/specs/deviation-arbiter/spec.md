## ADDED Requirements

### Requirement: A single arbiter owns every reactive actuator adjustment per tick
The system SHALL compute, once per tick and in exactly one call site, a reactive adjustment to the
plan's base setpoint allocation, and no other code path SHALL independently write reactive
(non-plan, non-VTN-override) adjustments to the same setpoints map.

#### Scenario: Opportunistic EV charging and battery correction do not run as separate passes
- **WHEN** a tick runs with the arbiter enabled
- **THEN** `dispatcher::build_setpoints` SHALL NOT itself invoke any EV-surplus or
  battery-correction logic — that logic SHALL execute only inside `arbiter::reconcile`

### Requirement: The deviation signal uses this tick's PV and base-load values, not a stale snapshot
The system SHALL compute the projected net site power for the arbiter's deviation calculation using
`SimState::peek_pv_kw` and `SimState::peek_base_load_kw` (this tick's physics-computed values),
falling back to the prior-tick snapshot only when a preview is unavailable.

#### Scenario: A PV step change is reflected without a one-tick lag
- **WHEN** PV output changes materially between the previous tick and the current tick
- **THEN** the arbiter's `deviation_kw` for the current tick SHALL reflect the current tick's PV
  value, not the previous tick's

#### Scenario: A base-load step change is reflected without a one-tick lag
- **WHEN** base load changes materially between the previous tick and the current tick
- **THEN** the arbiter's `deviation_kw` for the current tick SHALL reflect the current tick's
  base-load value, not the previous tick's

### Requirement: Levers are ranked by marginal cost, with zero-capacity levers excluded outright
The system SHALL rank available levers (battery, EV, heater-pause, heater-emergency-mode, PV
curtailment) by ascending marginal cost and greedily consume deviation capacity cheapest-first; a
lever with zero or negative remaining capacity SHALL be excluded from ranking entirely, not merely
deprioritized.

#### Scenario: An EV already at target SoC is excluded, not deprioritized
- **WHEN** the EV's SoC has reached its target
- **THEN** the EV lever SHALL NOT appear in the ranked lever list for that tick

#### Scenario: Cheapest available lever absorbs first
- **WHEN** a PV cloud transient produces an import deviation, and the EV is mid-session with
  headroom while the battery has a non-zero marginal cost
- **THEN** the arbiter SHALL reduce the EV's opportunistic draw before adjusting the battery

### Requirement: A lever switch requires the challenger to beat the incumbent by more than a preemption margin
The system SHALL NOT switch the actively-selected lever for a given deviation direction merely
because a challenger is nominally cheaper; the challenger's marginal cost SHALL be lower than the
incumbent's by more than a configurable margin before a switch occurs.

#### Scenario: Near-tied lever costs do not cause tick-to-tick switching
- **WHEN** two levers' marginal costs differ by less than the configured preemption margin across
  several consecutive ticks under a sustained deviation
- **THEN** the arbiter SHALL keep using the incumbent lever, not alternate between the two

### Requirement: Heater emergency-mode levers require the marginal cost to exceed a comfort-override threshold, with hysteresis against chattering
The system SHALL only offer `HeaterEmergencyMode::Curtail`/`Absorb` as an available lever when the
relevant directional marginal cost exceeds a configurable `heater_comfort_override_eur_per_kwh`
threshold, and SHALL apply a minimum dwell time (or equivalent hysteresis) to prevent rapid mode
toggling when the marginal cost hovers near that threshold.

#### Scenario: Routine tariff levels do not invade the safety envelope
- **WHEN** the directional marginal cost is at or below `heater_comfort_override_eur_per_kwh`
- **THEN** the heater emergency-mode lever SHALL NOT be offered, even if capacity exists

#### Scenario: An obligation breach penalty does invade the safety envelope
- **WHEN** the directional marginal cost (inflated by a VTN obligation breach penalty) exceeds
  `heater_comfort_override_eur_per_kwh`
- **THEN** the heater emergency-mode lever SHALL be offered and ranked by its (now-exceeded) cost
  like any other lever

#### Scenario: Marginal cost oscillating narrowly around the threshold does not chatter the heater mode
- **WHEN** the directional marginal cost crosses `heater_comfort_override_eur_per_kwh` back and
  forth across consecutive ticks by a small margin
- **THEN** `HeaterEmergencyMode` SHALL NOT flip on every such crossing; the dwell-time/hysteresis
  guard SHALL suppress rapid toggling

### Requirement: PV curtailment is a backstop lever priced at the forgone export tariff
The system SHALL only offer the PV curtailment lever in the export-excess direction, priced at that
slot's `export_tariff_eur_kwh`.

#### Scenario: PV curtailment is used only when other levers are exhausted
- **WHEN** battery, EV, and heater levers have all reached zero remaining capacity and an
  export-excess deviation remains
- **THEN** the arbiter SHALL tighten the PV export limit to absorb the remainder

### Requirement: The battery reactive-correction lever converges under a stationary disturbance without oscillating
The system SHALL, when the battery lever is active under a sustained (stationary) deviation across
multiple consecutive ticks, converge the applied battery setpoint to a stable value rather than
ringing or reversing sign after convergence.

#### Scenario: A constant unplanned load step converges within a few ticks
- **WHEN** a stationary (non-decaying) base-load deviation persists across several consecutive
  ticks
- **THEN** the battery lever's applied setpoint SHALL converge and SHALL NOT oscillate or reverse
  sign once converged

### Requirement: Absorbed deviation accumulates per SoC-coupled asset and triggers a replan past a threshold
The system SHALL track, per battery/EV asset, the kWh absorbed by the arbiter since the last plan
adoption, and SHALL emit `PlanTrigger::ResidualThreshold` when that accumulated amount exceeds a
configurable fraction of the asset's available charge/discharge capacity as of the last plan
adoption.

#### Scenario: A sequence of small absorptions crosses the threshold in aggregate
- **WHEN** four small battery absorptions each individually stay under the threshold fraction but
  their sum exceeds it
- **THEN** `PlanTrigger::ResidualThreshold` SHALL be emitted after the fourth absorption

#### Scenario: The accumulator resets on plan adoption
- **WHEN** a new plan is adopted (via any trigger)
- **THEN** each tracked asset's absorbed-kWh accumulator SHALL reset to zero and its capacity
  baseline SHALL be re-snapshotted from the newly adopted plan

#### Scenario: Raw per-tick deviation never triggers a replan directly
- **WHEN** a single tick's deviation is large but the asset's accumulated residual fraction remains
  under threshold
- **THEN** no `PlanTrigger::ResidualThreshold` SHALL be emitted for that tick

### Requirement: Residual-threshold replan triggers respect a minimum cooldown interval
The system SHALL NOT emit `PlanTrigger::ResidualThreshold` again within a configurable minimum
interval of the previous emission, even if the accumulated residual fraction remains above
threshold.

#### Scenario: A persistent (non-transient) cause does not cause back-to-back replans
- **WHEN** the residual fraction re-crosses the threshold shortly after a `ResidualThreshold`-
  triggered replan was adopted
- **THEN** no second `PlanTrigger::ResidualThreshold` SHALL be emitted until the cooldown interval
  has elapsed

### Requirement: The arbiter is gated behind a rollout flag, default disabled
The system SHALL only run `arbiter::reconcile` when `deviation_arbiter_enabled` is true for the
active profile; when false, the tick loop SHALL behave exactly as before this change.

#### Scenario: Disabled profiles are unaffected
- **WHEN** `deviation_arbiter_enabled` is false
- **THEN** `dispatcher::build_setpoints` SHALL apply the opportunistic EV overlay inline exactly as
  it did before this change, and no heater/PV-curtailment reactive lever SHALL fire
