# Design

## Context

See `proposal.md` — Why.

**Prerequisite: `ev-soc-state-variables`** (R-93/R-92) must land first. It gives the
EV MILP per-slot SoC variables, a balance constraint carrying trip drops, and an
`obligations: Vec<EvObligation>` list bound at each obligation's deadline step. This
change consumes that list; it adds no planner mechanism of its own.

The constraints that shape the approach:

**Where the concept lives today** (the `one-concept-one-function` inventory this
change reuses or consolidates):

| Concept | Where it lives today | What happens to it |
| --- | --- | --- |
| The session itself | `EvSession` — `VEN/src/entities/device_session.rs:30` | Gains a window start |
| Storage | `HemsState.ev_session: Option<EvSession>` — `VEN/src/state/mod.rs:114`; `AppState::ev_session()`/`set_ev_session()` — `:460`/`:464` | Becomes the queue + queue accessors |
| Producer 1 — user request | `routes/hems/sessions.rs:358`, via `services::user_request::create_ev` | Inserts into the queue |
| ~~Producer 2 — VTN `CHARGE_STATE_SETPOINT`~~ | `tasks/poll_signals.rs` | **Disabled** (R-100): a VTN no longer creates sessions; code preserved, uncalled |
| Producer 3 — simulated usage | `tasks/sim_tick/usage_sim_plan_ahead.rs::sync_plan_ahead_session` | Maintains a rolling 7-day span |
| Expiry | `tasks/sim_tick/arbiter_glue.rs::resolve_overlay_enabled:129-136` | Drops passed sessions from the head |
| Cancellation | `AppState::cancel_request` — `state/mod.rs:423` | Removes that request's session by id |
| Trip generation | `assets/ev_schedule.rs::daily_trip` / `active_trip_at` / `next_trip_after` | **Reused unchanged** |
| Per-slot absence / return drops | `assets/ev_schedule.rs::availability_per_slot` / `soc_drop_frac_per_slot` | **Reused unchanged — already multi-trip** |
| Session to MILP | `assets/ev_session_context.rs::EvMilpContext::from_state` | Builds one obligation per session |
| Predicted departure to MILP | `assets/ev_usage_forecast.rs::target_next_predicted_departure` | Pushes into the same obligation list |
| Obligation in the model | `EvMilpContext.obligations: Vec<EvObligation>` (built by the prerequisite change) | **Reused unchanged** — one obligation per queued session |
| Per-slot SoC + trip drops | `EvMilpVars.soc_ev` + balance constraint (prerequisite change) | **Reused unchanged** — the solver chains SoC across departures |
| Shortfall reporting | `milp_planner/ev_diagnostics.rs::firm_shortfall` (reads the solved slack after the prerequisite change) | **Reused** — already names its obligation's `session_id` |
| Tick-level departure | `EvCharger.departure_time` + `TickOverrides.ev_departure_time` — `assets/ev.rs:53,447` | Fed from the head session |
| UI session type | `VEN/ui/src/api/types.ts:522`; `components/sessions/SessionProgressBoard.tsx` | Queue-aware |

**What is already multi-trip and must not be duplicated**: the EV's absence mask
and return-drop projection. `availability_per_slot` and `soc_drop_frac_per_slot`
are evaluated per planning slot and already produce any number of away-windows
and drops in one horizon (see `availability_per_slot_reflects_multiple_trips_in_one_horizon`
in `ev_schedule.rs`). This change adds **no** second notion of "when is the car
away" or "how much does a trip cost in SoC".

**The EV MILP already reasons about SoC** once the prerequisite change lands:
`soc_ev[t]` over `0..=n`, a balance equality per slot carrying charging power and the
exogenous trip drop, and one `soc_ev[deadline_step] + shortfall_soc >= target_soc`
bound per obligation. A session therefore needs no energy arithmetic of its own —
only its deadline step and its target.

**Constraints**: VEN file-size caps (500 production lines in `VEN/src/`, 200 in
`tasks/`) — `ev_session_context.rs`, `ev_milp.rs` and `ev_usage_forecast.rs` are
already split out of `ev.rs`/`ev_milp.rs` for exactly this reason. `EvSession` is
never persisted (`state/persistence.rs::PersistedVenState` holds only programs,
events, reports, sensor) and never deserialised from a request body, so its shape
can change without a migration. R-97 benchmarks put EV phase-2 solve at 11-13 s,
so added solver variables are not free.

## Goals / Non-Goals

**Goals:**

- One authority for "do these two sessions conflict" and "where does this session
  go in the queue", used by all three producers.
- One representation of "an obligation the planner must meet", used by both the
  stated-session path and the predicted-departure path.
- The planner can pre-charge for a deadline that lies behind an intervening
  departure, to the extent the physics allows.
- No change to the EV's absence/return-drop mechanism.

**Non-Goals:**

- No planner mechanism: the SoC variables, balance constraint, obligation bounds
  and shortfall slack all arrive with the prerequisite change. This change only maps
  sessions onto the obligation list it already exposes.
- No change to the EV MILP's valuation of energy (comfort bands, per-slot rewards,
  budgets) beyond which session supplies the curve (Decision 5).
- No multiple *concurrent* sessions: one charge point, one vehicle. The queue is
  strictly time-ordered and non-overlapping by construction.
- No change to how a user submits a request (that is the follow-up change).
- The forecast usage class keeps targeting only its next predicted departure in
  this change (see Decision 7).

## Decisions

### Decision 1 — The queue is an ordered `Vec<EvSession>` behind a newtype, not a bare `Vec`

`HemsState.ev_session: Option<EvSession>` becomes
`ev_sessions: EvSessionQueue` — a newtype over `Vec<EvSession>` in
`entities/device_session.rs` that owns the ordering and non-overlap invariant.
`AppState` grows `ev_sessions()`, `current_ev_session(now)`,
`insert_ev_session(session) -> Result<(), EvSessionConflict>`,
`remove_ev_session(id)` and `expire_ev_sessions(now)`; `ev_session()` /
`set_ev_session()` are removed.

*Why a newtype*: the invariant ("ordered, non-overlapping") is the whole point of
this change, and `shiftable_loads: Vec<ShiftableLoad>` — the existing multi-session
precedent in `HemsState` — has no invariant to protect, so it is not a model to
copy here. A bare `Vec` would leave every one of the three producers free to
insert at the wrong index or on top of an overlap, which is the shape of bug
`one-concept-one-function` exists to prevent. Making the only public mutator a
checked `insert` means an overlapping queue is unrepresentable rather than merely
untested.

*Alternative considered*: `HashMap<Uuid, EvSession>` with sorting at every read.
Rejected — the ordering is load-bearing for the planner and for expiry, and
re-deriving it at each read site is exactly the duplicated-rule shape to avoid.

### Decision 2 — A session declares `window_start`; the pair `(window_start, departure_time)` is its window

`EvSession` gains `window_start: DateTime<Utc>`, and the window is the half-open
interval `[window_start, departure_time)`. Half-open makes "one session's
departure is the next one's window start" a non-conflict, which is the natural
encoding of "the car leaves and comes back" and matches `active_trip_at`'s
existing `ts >= leave_at && ts < return_at` convention.

*Why an explicit start rather than implicit "now"*: today a session's window is
implicitly "from now until `departure_time`", which is only meaningful for the one
session that is current. Overlap cannot be defined without a start, and the gap
between one session's `departure_time` and the next's `window_start` is precisely
the absence the SoC drop belongs to.

*Why not rename `departure_time` to `window_end`*: it is the user-facing concept
("when must the car be ready"), it appears in the UI, and
`naming-transparency` favours keeping the word the UI uses. `window_start` is
documented as the opening of the same window.

*Alternative considered*: `Option<DateTime<Utc>>`, `None` meaning "open from now".
Rejected — it reintroduces "the reader decides what the window is", the exact
`wire-contracts`-adjacent failure of a value whose meaning lives in the reader.
Nothing persists or accepts an `EvSession`, so there is no compatibility reason to
accept the weaker type.

### Decision 3 — One overlap predicate, one insertion rule, in the queue type

`EvSessionQueue::conflicts(&self, candidate) -> Vec<Uuid>` and
`insert(&mut self, session) -> Result<(), EvSessionConflict>` are the only way a
session enters the queue. `EvSessionConflict` carries the conflicting session ids
so a caller can name them without re-deriving the overlap.

**Conflict policy belongs to the producer, not to the queue.** The queue reports
the conflict; what to do about it differs per producer, and keeping that out of the
queue is what lets one `insert` serve all three:

- *simulated usage* — skip the conflicting trip (origin precedence: never displace
  a stated session).
- *VTN* — **no longer a producer** (R-100). A grid command about an asset's state of
  charge is an external constraint, not the driver's intent about their own travel,
  and the queue's non-overlap rule turned that conflation into a live conflict: a
  VTN session could block the user from booking their own car. The code is
  preserved and uncalled pending a decision on whether such a command belongs in
  the planner as a constraint instead.
- *user request* — **refuse**, surfacing `EvSessionConflict` with the clashing ids.
  Today's behaviour (last writer wins) is not preserved, deliberately: it is the
  data loss this work exists to prevent, and preserving it until the follow-up
  lands would mean knowingly shipping the failure case. The cost is accepted and
  bounded — until the follow-up adds the prompt, a user whose submission clashes
  gets a named error and must remove the old plan themselves. Friction beats a plan
  disappearing unnoticed.

Origin precedence (`EvSessionOrigin`) therefore stays in the producers, where it
already lives.

### Decision 4 — Each session becomes one `EvObligation`; the solver chains SoC itself

`ev-soc-state-variables` (R-93/R-92) has already replaced the scalar deadline pair
with `obligations: Vec<EvObligation>` — `{ deadline_step, target_soc, session_id }`
— bound as `soc_ev[deadline_step] + shortfall_soc >= target_soc` against solved
per-slot SoC variables. So this change adds **no** planner mechanism at all. It
maps each queued session inside the horizon to one obligation:

    deadline_step = the slot containing session.departure_time
    target_soc    = session.target_soc
    session_id    = Some(session.id)

and that is the whole planner-facing change.

*Why this ordering mattered*: computing each session's required energy outside the
solver would have meant predicting the SoC at a future session's window start,
which is a third implementation of "this EV's future SoC" beside
`asset_port::ev_soc_trajectory` and `ev_schedule::soc_drop_frac_per_slot` — the
duplication `one-concept-one-function` forbids, and the reason R-93 is recorded as
the structural blocker under R-92. With SoC in the model, the chaining *is* the
balance constraint: charge before a departure, the trip drop, and the recharge
after the return are all one series the solver reasons over, so carrying charge
across an intervening departure is expressible rather than capped.

*What the session's `window_start` contributes*: it bounds availability, not the
obligation. A session's window start and the preceding session's departure are the
absence the drop belongs to, and absence is already asserted per slot by
`ev_schedule::availability_per_slot` (`a_ev`). The obligation itself needs only its
deadline — the solver's SoC chain supplies everything else.

*Alternative considered*: keep one obligation and pick the most binding session.
Rejected — it is today's behaviour with extra bookkeeping, and cannot satisfy two
different targets at two different departures, which is the point of the queue.

### Decision 9 — A session states the trip distance; the EV converts it

This change's own planning spec already requires that "the charge the vehicle is
expected to consume while away SHALL be accounted for between sessions". For
*simulated* sessions that holds today: `ev_schedule::soc_drop_frac_per_slot` derives
the drop from `EvUsageSimParams`. For a *manually stated* series it does not hold at
all — `EvSession` carries no consumption field, so the planner assumes the car
returns as it left.

The failure is concrete. 50 kWh pack, two stated sessions, no usage simulation
configured: the user asks for 80 % by Monday 08:00 and 80 % by Wednesday 08:00. The
plan charges to 80 % for Monday, believes the car returns Monday evening still at
80 %, and schedules **nothing** for Wednesday. The car sits at 40 % on Wednesday
morning, and Monday night's cheap tariff went unused.

So the session declares it:

    EvSession.expected_trip_distance_km: Option<f64>   // None = use the EV's default
    EvParams.default_trip_distance_km:   f64           // the suggestion/fallback
    EvParams.consumption_kwh_per_km:     f64           // converts km into energy

**Distance, not a percentage.** A driver knows "about 120 km", not "38 % of my
pack". The conversion needs the pack size and the vehicle's consumption, both of
which the EV already owns.

**The EV does the conversion, nobody else.** `EvCharger` exposes the
distance-to-SoC-drop function and is the single authority for it
(`asset-competence-assurance`): a route, the UI or the planner computing its own
`km × kWh/km ÷ battery_kwh` would be a second copy of the rule, which is how this
project's recurring class of bug starts.

**`Option` with a visible default, not a silent one.** `None` means "the user did not
say", and the EV's configured default fills it — but the plan reports that it did.
That is the `wire-contracts` pattern already required of protocol values: apply the
documented default *and surface that you applied it*. A silently defaulted 120 km is
a number whose origin lives only in the reader.

**Naming.** `trip`, not `usage`: `ev_schedule` already calls a journey a trip
(`UsageTrip`, `daily_trip`, `next_trip_after`), while `usage` names the *config
block* (`usage_sim`), so `trip` is the word that greps from one to the other
(`naming-transparency`). Units are suffixed per the `naming` rule, and
`consumption_kwh_per_km` follows the existing `tariff_eur_per_kwh` shape.
`kWh/km` rather than Europe's more common kWh/100 km, because every call site wants
the per-km figure and a factor of 100 in the arithmetic is a bug waiting to happen —
the UI may still present it per 100 km.

*Alternative considered — derive the drop from the gap length using the EV's learned
heuristics.* Rejected as the primary mechanism: a 10-hour gap might be a commute or a
car sitting on the drive, and the user would have no way to correct the guess. It
remains a reasonable future refinement of the *default*, which is why the default is
an EV-owned function rather than a constant read at the call site.

### Decision 5 — The comfort curve prices the head session only

`segments` (the priced bands from `ev_comfort::ev_energy_segments`) continues to
be built once, from the live SoC and the head session's curve and target. Queued
sessions behind the head contribute a firm `required_kwh` and no bands.

*Why*: a band's `kwh` is measured from a starting SoC. For a session two departures
out, that starting SoC is a quantity the *solver* now decides (it is `soc_ev` at
that session's window start), so bands sized from it would have to be built from a
value that does not exist until the solve finishes. A firm requirement has no such
circularity: it is a bound at a deadline, and `declare_vars` already handles
"requirement not covered by bands" with one unpriced guarantee band
(`ev_milp.rs:60-66`) — reused unchanged.

*Alternative considered*: SoC-indexed band variables per session, letting each
session's bid price its own energy. Rejected here as a valuation change rather than
a queue change — it multiplies `e_seg` by queue length and belongs in its own piece
of work if the fleet ever wants it. Recorded in `TECHNICAL_DEBTS.md`.

### Decision 6 — "A session is active" means its window is open now, not "the queue is non-empty"

`arbiter_glue::resolve_overlay_enabled` currently derives
`EvSettings.paused_by_active_session` from `ev_session.is_some()`. Under a rolling
7-day simulated schedule the queue is almost never empty, so that test would pause
opportunistic charging permanently. It becomes `current_ev_session(now).is_some()`
— the session whose window contains `now`. Expiry in the same function generalises
from "clear if departure passed" to `expire_ev_sessions(now)`, which drops every
passed session from the head.

This is a behaviour trap rather than a free choice, and it is why the expiry and
the `paused_by_active_session` derivation must change in the same step as the
rolling producer, not after it.

### Decision 7 — The forecast usage class keeps a single target, but through the obligation list

`target_next_predicted_departure` stops writing `t_dead_step`/`e_required_kwh` and
pushes one `EvObligation` instead. Its behaviour is otherwise unchanged: still the
next predicted departure only, still only when no stated session governs the goal.

*Why not extend it to every predicted trip in the horizon now*: it is a genuine
behaviour change to a second usage class, with its own BDD coverage
(`tests/features/ev_usage_forecast.feature`), and it is not needed to prove the
queue. Routing it through the same obligation list means there is still only one
representation of an obligation, so widening it later is a loop, not a new
mechanism.

### Decision 8 — Producers insert; the head session feeds the tick

`EvCharger.departure_time` (the tick override that makes `simulate_forward` aware
the car leaves) is fed from `current_ev_session(now)`, or the head of the queue
when none is current. It stays a single instant: the tick only needs the next
departure, and `is_away_at` already ORs the session departure with the trip
schedule.

### Decision 10 — `earliest_start` is the user's window start; no new field

`CreateUserRequestParams` already carries `earliest_start` for shiftable loads —
"the earliest this may begin" is exactly what an EV session's window start is.
Reusing it keeps one request vocabulary; a second field beside it would be a second
name for one concept.

`services::user_request::create_ev` maps it to `window_start`, defaulting to `now`
when absent — which is both today's implicit semantics and the only sensible reading
of "available from unspecified".

An `earliest_start` at or after the request's deadline is refused as an empty
window: a distinct error from a clash, because it is a malformed request rather than
a collision with something else.

*Why this is here rather than in the follow-up change:* without it every stated
session opens at the submission instant, so any two of them overlap and the refusal
above would make a second session impossible. The queue would then be usable only
by the simulated schedule — which is not the capability being asked for.

## Risks / Trade-offs

- **Opportunistic charging silently pauses forever** once the queue is rarely
  empty → Decision 6 changes the test to "window open now" in the same step as the
  producer; pinned by a scenario asserting opportunistic charging resumes between
  sessions.
- **Solve time grows with queue length** — one SoC bound plus one shortfall slack
  per obligation inside the horizon (at most ~7) → re-benchmark against the figures
  the prerequisite change records and note the delta; a 7-session queue is the
  realistic worst case the rolling window produces.
- **A wrong drop prediction mis-sizes a later obligation** → every planning cycle
  re-reads live SoC into `soc_ev[0]`, so the error is corrected at the next cycle
  rather than accumulating, and the shortfall slack means a mis-prediction degrades
  to a reported gap, never an infeasible site solve.
- **Three producers inserting into one invariant-bearing structure** → the only
  mutator is the checked `insert`; a property test asserts the invariant holds
  after arbitrary interleavings of insert/remove/expire.
- **File-size caps** on `ev_session_context.rs` and `usage_sim_plan_ahead.rs` (the
  latter under the 200-line `tasks/` cap) → the trip-to-session mapping lands in
  `assets/` if the task file would overflow, and `scripts/audit_file_sizes.py` runs
  before commit.

## Migration Plan

No data migration: `EvSession` is in-memory only
(`state/persistence.rs::PersistedVenState` does not hold it) and is never
deserialised from a request body, so `window_start` can be a required field.

Deployment is a normal VEN rebuild. Rollback is a revert — the only externally
visible shape changes are additive (`GET /ev-session` and `GET /user-requests`
gain the queue; a client reading only the current session keeps working if the
current session remains addressable in the response, which the UI task preserves).

## Open Questions

- Should the simulated schedule's 7-day span be configurable
  (`EvUsageSimParams`) rather than fixed? Deferrable: a constant satisfies the
  spec as written, and adding a parameter later changes no behaviour at the
  default.
- Should `GET /ev-session` return the queue and keep the current session as a
  convenience field, or return the queue only and let clients pick? Deferrable to
  the UI task; it affects no requirement in the specs.
