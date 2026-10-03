# Design

## Context

See `proposal.md` — Why. The relevant current state is that the MILP consumes exactly three
EV quantities, and two producers derive all three independently:

| quantity | consumed by | session path | forecast path |
|---|---|---|---|
| `a_ev: Vec<bool>` | `p_ev[t] <= (a_ev[t] ? p_max_kw : 0) * z_ev_on[t]` | `ev_session_context::availability_from_sessions` | `ev_schedule::availability_per_slot` |
| `soc_drops` | the balance equality `soc_ev[t+1] == soc_ev[t] + … − drop_frac + drop_unmet` | `ev_session_context::trip_drops_between_sessions` | `ev_schedule::soc_drop_frac_per_slot` |
| `obligations: Vec<EvObligation>` | `soc_ev[deadline_step+1] + shortfall_soc[k] >= target_soc` | `ev_session_context::obligations_from_sessions` | `ev_usage_forecast::target_next_predicted_departure` |

The six derivations already disagree in two ways that are the defects this change closes: the
forecast path produces one obligation where the session path produces N, and the session path
times a trip's consumption from the *next* session's `window_start` where the forecast path
times it from the trip's own return.

The constraint shapes themselves are correct and stay untouched. `obligations` is already a
list and `soc_ev[t]` is already a per-slot variable (R-92/R-93), so nothing in the model needs
widening — only its inputs.

The EV asset is the sole authority for interpreting its own state and forecast
(`asset-competence-assurance`), which fixes where each piece may live: producers may state
*what trips are expected*; only the EV may say what a trip costs; only one function may turn
trips into planner inputs.

## Goals / Non-Goals

**Goals:**

- One derivation from an expected-trip series to the three MILP quantities, with the session
  queue and the trip generator as its only two callers.
- Delete `availability_per_slot`, `soc_drop_frac_per_slot`, `trip_drops_between_sessions`,
  `availability_from_sessions` and `obligations_from_sessions` rather than add a seventh
  derivation beside them. The change must be net-subtractive in derivation count.
- Make "no estimate" representable in the type system, so the absent case cannot silently
  acquire a value on its way to the solver.

**Non-Goals:**

- No change to `EvMilpContext`'s constraint or objective shape, to `EvObligation`, or to the
  two-phase solve. This change only alters what fills them.
- No change to the live simulation tick's physics (`active_trip_at`, `apply_usage_sim_tick`).
  Those decide what *happens*; this change decides what is *planned*.
- No renaming of the `usage_sim` / `usage_forecast` classes (recorded as debt — see
  proposal, Out of scope).
- No change to conflict/overlap policy, which stays `EvSessionQueue::insert`'s and remains the
  subject of `ev-session-user-conflict-resolution`.

## Decisions

### 1. The series element is "one expected use of the vehicle", with consumption optional

```rust
/// One expected use of the vehicle: when it may charge, when it must be ready, and what
/// the trip that follows is expected to cost — when that is known at all.
pub struct ExpectedVehicleUse {
    /// Charging may begin here.
    pub window_start: DateTime<Utc>,
    /// The vehicle must hold `target_soc` by here.
    pub departure_at: DateTime<Utc>,
    pub target_soc: f64,
    /// A firm departure is a guarantee and binds an obligation; a soft one states a
    /// preference priced by the comfort curve and binds none (`ev-comfort-piecewise-core`).
    pub firm: bool,
    /// `None` when nobody said what the trip costs or when it ends. The plan then holds the
    /// state of charge flat until a real return is measured.
    pub consumption: Option<ExpectedTripConsumption>,
    /// The stated session behind this, when there is one, so a shortfall can name it.
    pub session_id: Option<Uuid>,
}

pub struct ExpectedTripConsumption {
    pub return_at: DateTime<Utc>,
    pub soc_drop_frac: f64,
}
```

`Option<ExpectedTripConsumption>` is the heart of the change: distance and return time are a
pair in the domain type, not two independent `Option`s, so "distance with no return time" is
unconstructible below the route layer. The both-or-neither rule then needs enforcing in exactly
one place — the boundary that parses a user's submission — instead of being re-checked wherever
the pair is read.

*Alternative rejected:* keep `expected_trip_distance_km: Option<f64>` and add
`expected_return_time: Option<DateTime<Utc>>` as a sibling on `EvSession`. Two independent
`Option`s make the invalid combination representable, and every reader must then remember to
check both. The pair lives in the domain type; `EvSession` still stores the user's two raw
fields (that is what the user typed, and `dto` says pass values through), but the conversion
into `ExpectedVehicleUse` is the single place the pairing is asserted.

*Alternative rejected:* reuse `ev_schedule::UsageTrip` as the series element. It carries
`soc_drop_pct` unconditionally and no target or window, so it cannot express the absent-estimate
case or a stated target. It stays what the generator produces; the producer maps it.

### 2. Availability is the union of charging windows, plus a trailing window after a known final return

A slot is chargeable when it **overlaps** some window `[window_start, departure_at)` — overlap,
not containment of the slot's start, because GB-54 aligns a plan's `now` to the slot grid and
testing the start locks the EV out of the in-progress slot (the bug fixed in 049 and preserved
here).

The union alone has a gap: after the last known departure there is no further window, so the
vehicle would be unchargeable from its final return to the horizon end even though it is home.
So when the final use states a consumption — i.e. its return time is known — the derivation
appends a trailing window `[return_at, horizon_end)` carrying no obligation. When the final
return is unknown, nothing is appended, which is the honest answer: we do not know the vehicle
is back.

This single rule gives both classes correct behaviour:

- *forecast*: returns are always known, so the tail after the last predicted trip is chargeable.
- *stated session with an estimate*: same.
- *stated session without an estimate*: unchargeable after departure, because nothing says the
  car came back.

*Alternative rejected:* derive availability as the complement of away-intervals
`[departure_at, return_at)`. With consumption optional the away interval has no end, so the
complement is empty to the horizon edge — the same answer by a less direct route, and it cannot
express a user's stated "Available from" that starts later than the previous return.

### 3. A trip's consumption is placed at its own return, never at a neighbour's window

`drop_frac_per_slot[slot_of(return_at)] += soc_drop_frac`, from the same use that states it.
This deletes the `sessions.windows(2)` pairing and with it the lone-session and final-session
holes. Slot 0 still never carries a drop: a return already in the past is reflected in the live
state of charge the plan starts from, and counting it again charges the trip twice.

### 4. The forecast producer walks the horizon; the session producer maps the queue

Both become short, obviously-complete loops over an existing generator:

```rust
// forecast: every predicted trip in the horizon, not just the next
let mut cursor = now;
let mut window_open = now;
while let Some(trip) = next_trip_after(usage, tag, cursor, horizon_end) {
    uses.push(ExpectedVehicleUse {
        window_start: window_open,
        departure_at: trip.leave_at,
        target_soc: cfg.soc_target,
        firm: true,
        consumption: Some(ExpectedTripConsumption {
            return_at: trip.return_at,
            soc_drop_frac: trip.soc_drop_pct / 100.0,
        }),
        session_id: None,
    });
    window_open = trip.return_at;
    cursor = trip.leave_at;
}
```

This is the same walk `usage_sim_plan_ahead::sync_plan_ahead_session` already performs to fill
the queue, which is the evidence that the loop is the right shape: one generator, two
traversals, now with the same element type between them.

`engage_charge_planning: false` keeps its present meaning — the mask and the drops are stated
(they are fact), `firm` is false on every use, so no obligation binds and nothing drives
charging. That is a one-field change to the producer, not a second code path.

### 5. The EV converts distance to energy, and there is nothing left to default

`EvCharger::expected_trip_drop(distance_km: f64) -> f64` loses its `Option` and its
`ExpectedTripDrop` wrapper: with no fallback there is no provenance to report, so `defaulted`,
`distance_km` echo-back and the `any_defaulted` plumbing all go. `default_trip_distance_km`
leaves `EvCharger`, `asset_params`, `profile::{schema, defaults, validate}` and the profile
contract. No shipped YAML profile sets it, so no profile file changes — but its removal is a
breaking profile change for anyone who did.

### 6. Soft deadlines and the non-deadline request modes keep their current treatment

`firm` is computed by the session producer exactly as `obligations_from_sessions` computes it
today: `!soft_deadline && mode.states_a_firm_deadline()`. `ByDeadlineFree`, `Opportunistic`,
`AsapFree` and `MaxCost` state no obligation and continue to express their window through
`a_ev` and their incentive through the per-kWh reward terms. A non-firm use still contributes
its window and its consumption — availability and consumption are facts, independent of whether
a goal is guaranteed.

### 7. The documentation is replaced, not amended

The affected sections are rewritten from the corrected model as whole sections, with no
"previously…" or "note that…" bridging sentence, because a correction appended to a wrong
explanation leaves the wrong explanation readable. Specifically targeted: the claim in
`VEN_ARCHITECTURE.md` that under the forecast class "the solver can plan the recharge the
*return* makes possible" — true of the mask, false of the plan, and the single sentence most
likely to make a future reader believe this change was already made. `project_journal.md`'s
048/049 entries are rewritten too, which is a deliberate exception to the journal's
append-only habit, taken because its account of the two classes is the origin of the
misunderstanding.

## Risks / Trade-offs

- **Fleet plans change on every EV VEN at once; a mistake is 12 VENs wide, not one.** →
  Verify on one VEN before the fleet: deploy to ven-12 alone, confirm its live 48 h timeline
  shows rise-fall-rise-fall across both trips, then roll out. ven-12 is the reproduction case
  and has two trips in the horizon today.
- **More binding obligations per solve could slow the MILP or widen the phase-2 gap.** → The
  048 benchmark got *faster* with per-slot SoC variables, but it measured one obligation.
  Re-run the same benchmark with two and three obligations before merging; record the result
  with the `GapLimit` caveat the 048 entry already carries.
- **Removing the distance default silently removes drops some plans currently show.** That is
  the intent, but it will look like a regression to anyone watching a chart. → Call it out in
  the use-case manual and the journal entry, and state it in the release note as a correction
  with its before/after shape.
- **An EV whose next departure sits just beyond the horizon edge still gets no obligation and
  so no charging** (ven-2 was 20 seconds outside, planning nothing at 0.640). This change does
  not fix that — it is a horizon-boundary problem, not a multi-trip one. → Record as debt in
  the same commit, with ven-2 as the evidence, so it is not mistaken for a symptom of this
  work.
- **`firm: true` on every predicted trip makes the forecast class state guarantees it cannot
  always honour**, so more plans may carry `EvCoreEnergyUnmet` warnings than before. → That is
  the designed meaning of the shortfall slack: the warning is the model reporting a gap it was
  asked to close and could not. Confirm the warning text names the departure usefully when no
  session id exists behind it.

## Migration Plan

1. Land the shared derivation with both producers mapped onto it, with the old six derivations
   deleted in the same commit — not left beside it.
2. Rust + UI suites green locally, then E2E on Node2.
3. Deploy ven-12 only; compare its `/plan` EV series against the recorded pre-change shape
   (`0.800 → 0.698 → 0.570`, flat between) and confirm charging now appears in both gaps.
4. Roll out to the remaining EV VENs.
5. Rollback is a redeploy of the previous image: the change is confined to planner inputs, and
   `EvSession`'s new field is additive and `#[serde(default)]`, so persisted sessions written by
   the new build still load on the old one.

## Open Questions

- Should the UI show *why* a plan projects no drop for a session with no estimate (a hint on the
  EV card beside the session), or is the absent drop self-explanatory? Affects one piece of UI
  copy, no spec requirement and no task boundary, so it can be settled when the card is built.
