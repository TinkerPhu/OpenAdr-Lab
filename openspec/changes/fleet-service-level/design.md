## Context

See proposal.md - Why. Two facts shape the design. The fleet already ships every VEN's full simulator snapshot
(`SimSnapshot`, with each asset's `AssetSnapshot`) every 5 s on the lab broker, and the BFF stores it in
`lab_recorder.fleet_telemetry` for 7 days (`RAW_RETENTION_DAYS`; the 1-minute rollup past that keeps only the
site power). So a figure that rides on the asset snapshot reaches the fleet's history with no new channel. And
the planner already reports a *planned* shortfall (`WarningKind::EvCoreEnergyUnmet`, `HeaterTargetUnmet`,
`BudgetShortfall`, from the solve): that is a prediction made at plan time, absent under the opportunistic
objective (no solve), and not what a site delivered. The new figure is the delivered one.

## Goals / Non-Goals

**Goals**: a service level and a change in stored energy that every objective is judged against, from what sites
did; kind-free in the report and the card, so heat pumps (R-151) join by declaring.
**Non-Goals**: pricing the shortfall; a per-EV departure state of charge or per-heater minutes below comfort in the
drill-down (per-kind episodes: filed when this is implemented); storing the figures in the VEN's own SQLite history;
reporting them through OpenADR (they are lab telemetry; a report would need `payloadDescriptors`); replacing the
planner's plan warnings.

## Inventory of existing logic reused or consolidated

| Concept | Where | This change |
|---|---|---|
| Energy an EV needs to reach its active target | `ev.rs::full_effort_time_to_target_s_inner` (`(soc_target_frac - soc) * battery_kwh`), `ev_session_context.rs` `core_kwh` (the same product for a session) | One `energy_to_target_kwh_inner` on the EV; time to target divides it by the charge rate, unserved energy reads it; `core_kwh` calls it if its target is the same quantity (task 1.1 checks and consolidates, never a third copy) |
| Energy to lift a tank by some degrees | `heater.rs` uses `thermal_mass_kwh_per_c` in `step` (`delta_c`) and the capability builders | One `energy_to_temperature_kwh` on the heater; unserved and stored energy call it (task 1.2 greps `heater_capabilities.rs` and `heater_milp.rs` first and folds any copy) |
| Storage capacity | `Asset::storage_capacity_kwh` (battery, EV) | Reused: stored energy is `soc * storage_capacity_kwh` through one place per asset, not a new capacity |
| Weighted state of charge across assets | `report_intervals.rs` (`soc_frac * storage_capacity_kwh`) over history rows | Left as is: it reads immutable history; noted so a reader sees why the product exists twice (this one declares the live value) |
| Deadline / departure transition | `EvState::was_away_by_usage_sim`, `ev.rs` step | The departure latch is set where that transition is already detected; no second detector |
| Planned shortfall | `WarningKind::*Unmet`, `plan_outcome.rs`, `heater_declared.rs::target_unmet` | Kept, different concept (plan time); the docs name the difference |
| Switch instant and the instant before it | `fleetObjective.ts::latestObjectiveSwitch`, `fleetPlan.ts::instantBefore`, `PlanComparison.tsx` | Reused for the card's windows and caption |
| Raw-vs-rollup choice of a stored query | `fleet_store.rs::source_for`, `RAW_QUERY_HORIZON_DAYS` | Reused: a window needing the rollup is "unavailable" for this report |
| Energy formatting | `lab-ui/src/unitFormat.ts::formatEnergyKwh` | Reused |
| Unit conversions | `entities/units.rs` | Used for any time/energy conversion in the assets; none inline |

## Decisions

**D1 Declared, per asset, as a level.** Each asset declares its unserved energy as a level (what is missing now),
not as an event stream. Alternatives: a published "service miss" event per departure (a new retained-versus-lossy
channel and a second store: rejected); the BFF deriving shortfall from SoC and a target it would have to read
(a second authority for the formula: rejected by `asset-competence-assurance`).

**D2 Accumulate by the sum of increases.** The report adds the positive steps of the level between consecutive
stored samples. This gives one rule for every kind: an EV's departure is a step up, a heater's breach is a step
up, a shiftable load's deadline is a step up, and a recovery is a step down that adds nothing. Alternatives: the
time integral of the level (kWh x hours: a trip held for 8 h would read as eight times its gap); the peak per window
(forgets a second departure); a per-kind rule in the BFF (branches on kind). Lossy QoS-0 samples are safe because
the level holds (the EV latch lasts the whole trip, a heater breach lasts many samples) and a missing sample only
merges neighbouring steps.

**D3 What "due" means, per asset.**
- EV: due at departure. `EvState` gains a latch set on the plugged-to-away transition to `energy_to_target_kwh`
  at that instant, held while away and cleared on the next plug-in (`#[serde(default)]`, so stored states load).
  The target is the EV's active target (`soc_target_frac`, which a session overrides each tick), the quantity
  the arbiter's implicit request and the planner's core energy already use. Task 1.1 first confirms that a
  forecast-class EV's planner honours that same target; if it plans to a lower trip need, the declaration reads
  that need through the same function, because comparing a planner against a target it never aimed at would
  blame it for physics.
- Heater: due continuously: `energy_to_temperature_kwh(max(temp_min_c, requested target past its time) - T)`,
  zero above. Electric-equivalent is thermal over the element's efficiency (1 for a resistive element, the COP
  curve for a heat pump, R-151).
- Shiftable load: due after its deadline: the energy of the run still owed, latched until finished or removed.
- Battery: none. Its minimum state of charge is a limit the battery enforces on itself; it never "misses" it.

**D4 Stored energy is declared too, reference cancels.** Battery and EV: `soc * storage_capacity_kwh`, the EV also
while away (the energy is in the car). Heater: `energy_to_temperature_kwh(T - temp_min_c)`, so a tank is zero at
its comfort floor. The carry-over is a difference of two samples, so each asset's reference point drops out;
only the asset needs to be consistent with itself. Alternative: one site-wide stored figure from SoC alone
(misses the tank, the third store).

**D5 The two fields ride on `AssetSnapshot`.** `unserved_energy_kwh: Option<f64>` and
`stored_energy_kwh: Option<f64>`, filled by the snapshot builder from the two new `Asset` methods, `None` meaning
"this asset declares none" and serialised away. They flow to `/sim`, the VEN UI and the telemetry store with the
rest of the snapshot, so the lab channel needs no new topic. Names carry the unit (`naming`), pass through every
layer unchanged (`dto`). Because the fields are our own telemetry vocabulary and not an OpenADR payload,
`wire-contracts` does not require descriptors here; an OpenADR report of them would.

**D6 The BFF route.** `GET /api/fleet/service?from=&to=` reduces the stored rows in SQL to at most one sample per
VEN, asset and minute (the last in the minute), so a 24 h window over 20 VENs reads about 30 000 rows, not
345 000 payloads, and applies the one accumulation function over each asset's series. A window starting before
the raw horizon answers `available: false` with the reason, never a zero. The response carries per-asset values,
per-VEN sums, the fleet sum and `contributingVens` per total (same convention as `/api/fleet/power`).
Alternative: reading every 5 s row (cost on the Pi for no gain given D2).

**D7 Windows on the card.** The after window is `[switch, now]`, the before window the equal-length span ending at
the switch (`instantBefore` semantics), so a 16 h run is compared with its preceding 16 h. Raw kWh are shown with
the window length; the card does not normalise to per day, because the comparison is the two windows side by side.
Carry-over across a window is sensitive to where in the day it starts and ends, so the caption says so, and the
first fleet runs use windows that are whole multiples of a day where available.

**D8 Surfaces.** Fleet page: a "Service delivered" card beside `PlanComparison` (two rows: unserved energy,
carry-over; a before/after column each; the VEN count) with a per-VEN, per-asset drill-down. VEN UI: each asset
card on the Devices page shows both figures (`ui-transparency`). One BFF function accumulates; one card component
displays; the card declares its two quantities as data (label, unit formatter) and looks them up
(`declare-dont-branch`).

**D9 Extensibility to heat pumps and air conditioners.** A compressor asset (R-151) declares the same two
quantities in electric-equivalent kWh using its COP. Its compressor lockout (a held start) can make an objective miss
a need through no fault of its own; recording the reason for a miss is added with R-151, not here.

## Risks / Trade-offs

- [The EV target is not what its planner aims at] -> task 1.1 verifies first and routes the declaration through the
  same function the planner uses.
- [Heater level chatters around the floor, and every up-step counts] -> the heater's own deadband latches at the
  floor, and a tolerance (as `SHORTFALL_TOLERANCE_KWH`) keeps solver-scale noise out; a test pins no step below it.
- [Carry-over compares windows that start at different times of day] -> equal-length windows, a caption, and a
  note that multiples of a day are comparable.
- [`EvState` gains a field] -> `#[serde(default)]`; a stored state without it loads with the latch at zero.
- [Raw telemetry is kept 7 days] -> older windows answer unavailable; a longer record needs a rollup of these
  two figures, filed rather than guessed.
- [A VEN restart resets the EV latch] -> the level drops to zero then rises again at the next departure; the
  accumulation counts only increases, so a restart cannot create or erase a departure's gap except during a trip.

## Migration Plan

Additive on the wire (two optional snapshot fields) and in the BFF (one new route). Deploy the VENs first (they
start publishing the fields), then the BFF and the VTN UI; the card shows "no data" until samples with the fields
exist in the window. Rollback by redeploying the previous images; the extra fields are ignored by older readers.

## Open Questions

- Should the VEN's own SQLite history also record the two figures (a VEN-local card)? Not needed for the Fleet
  card; decided when a VEN-local use appears.
- Whether the card should offer a per-day normalisation for windows that are not whole days: deferred until a
  comparison needs it.
