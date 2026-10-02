# Design

## Context

See `proposal.md` — Why. The constraints that shape the approach:

**Where the concept lives today** (the `one-concept-one-function` inventory this
change reuses or consolidates):

| Concept | Where it lives today | What happens to it |
| --- | --- | --- |
| The session itself | `EvSession` — `VEN/src/entities/device_session.rs:30` | Gains a window start |
| Storage | `HemsState.ev_session: Option<EvSession>` — `VEN/src/state/mod.rs:114`; `AppState::ev_session()`/`set_ev_session()` — `:460`/`:464` | Becomes the queue + queue accessors |
| Producer 1 — user request | `routes/hems/sessions.rs:358`, via `services::user_request::create_ev` | Inserts into the queue |
| Producer 2 — VTN `CHARGE_STATE_SETPOINT` | `tasks/poll_signals.rs:117-150` | Inserts / removes by id |
| Producer 3 — simulated usage | `tasks/sim_tick/usage_sim_plan_ahead.rs::sync_plan_ahead_session` | Maintains a rolling 7-day span |
| Expiry | `tasks/sim_tick/arbiter_glue.rs::resolve_overlay_enabled:129-136` | Drops passed sessions from the head |
| Cancellation | `AppState::cancel_request` — `state/mod.rs:423` | Removes that request's session by id |
| Trip generation | `assets/ev_schedule.rs::daily_trip` / `active_trip_at` / `next_trip_after` | **Reused unchanged** |
| Per-slot absence / return drops | `assets/ev_schedule.rs::availability_per_slot` / `soc_drop_frac_per_slot` | **Reused unchanged — already multi-trip** |
| Session to MILP | `assets/ev_session_context.rs::EvMilpContext::from_state` | Builds one obligation per session |
| Predicted departure to MILP | `assets/ev_usage_forecast.rs::target_next_predicted_departure` | Pushes into the same obligation list |
| Deadline in the model | `EvMilpContext.t_dead_step` + `.e_required_kwh` — `controller/milp_planner/asset_port.rs:92,101` | Replaced by the obligation list |
| Deadline-bounded energy | `assets/ev_milp.rs::energy_expr:96` / `reachable_energy_kwh:109` | Generalised to take a window |
| Shortfall reporting | `assets/ev_diagnostics.rs::firm_shortfall` | Reports per obligation |
| Tick-level departure | `EvCharger.departure_time` + `TickOverrides.ev_departure_time` — `assets/ev.rs:53,447` | Fed from the head session |
| UI session type | `VEN/ui/src/api/types.ts:522`; `components/sessions/SessionProgressBoard.tsx` | Queue-aware |

**What is already multi-trip and must not be duplicated**: the EV's absence mask
and return-drop projection. `availability_per_slot` and `soc_drop_frac_per_slot`
are evaluated per planning slot and already produce any number of away-windows
and drops in one horizon (see `availability_per_slot_reflects_multiple_trips_in_one_horizon`
in `ev_schedule.rs`). This change adds **no** second notion of "when is the car
away" or "how much does a trip cost in SoC".

**The EV MILP model is energy-based, not state-based.** `EvMilpVars`
(`asset_port.rs:140`) has no state-of-charge variable: the obligation is one
cumulative-energy constraint `energy_expr(...) == bought` over slots `0..=t_dead_step`,
with a floor `delivered >= min(e_required_kwh, reachable_energy_kwh)`. SoC appears
only as `soc_init` plus the post-solve trajectory. Any design that needs the SoC at
a *future* session's window start therefore has to predict it outside the solver.

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

- No state-of-charge decision variable in the MILP (see Decision 4).
- No closed-loop optimisation of how much pre-charge survives a trip; the drop is
  predicted open-loop, exactly as `soc_drop_frac_per_slot` already does.
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
("when must the car be ready"), it appears in the UI and in the VTN path, and
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
- *VTN* — as today: it owns the session it created, identified by id, and replaces
  only that one.
- *user request* — in this change, preserve today's externally observable
  behaviour explicitly: ask `conflicts`, remove exactly those sessions, then
  `insert`. This is the silent overwrite that exists today, now concentrated at one
  site and visible in one place. It is also precisely the call site the follow-up
  change (`ev-session-user-conflict-resolution`) replaces with a refusal plus a
  named conflict, which is why this change leaves it behaving as it does rather
  than half-building the refusal here.

Origin precedence (`EvSessionOrigin`) therefore stays in the producers, where it
already lives.

### Decision 4 — The planner receives a list of obligations; SoC between sessions is predicted open-loop, not solved

`EvMilpContext.t_dead_step: Option<usize>` and `.e_required_kwh: f64` are replaced
by `obligations: Vec<EvObligation>`, where

    EvObligation {
        first_step: usize,
        last_step: usize,
        required_kwh: f64,
        session_id: Option<Uuid>,
    }

`energy_expr` and `reachable_energy_kwh` take `(first_step, last_step)` instead of
reading `t_dead_step`; `constraints()` emits one energy-floor constraint per
obligation (`delivered_in_window >= min(required_kwh, reachable_in_window)`), and
the single `ev_energy == bought` equality becomes a whole-horizon equality so the
band/extra accounting is unchanged.

`required_kwh` for each obligation is computed **outside** the solver, chaining:

    soc_at_window_start[0] = live SoC
    soc_at_window_start[k] = max(target[k-1] - predicted_drop[k-1], floor)
    required_kwh[k]        = max(0, (target[k] - soc_at_window_start[k]) * battery_kwh)

with `predicted_drop` taken from the same trip the gap belongs to — i.e. from
`ev_schedule`, not from a second source.

*Why open-loop*: the EV MILP has no SoC state variable (see Context). Introducing
one plus a per-session chaining constraint would let the solver trade pre-charge
against a later session's requirement optimally, but it adds `n` continuous
variables and `n` equalities per EV to a model whose phase 2 already costs 11-13 s
(R-97), and it changes the EV model from energy-accounting to state-accounting —
a far larger change than this one, affecting every existing mode arm. Open-loop
prediction is also already the project's accepted treatment of the same quantity:
`soc_drop_frac_per_slot` predicts the drop for the trajectory without the solver
deciding it. Where the prediction is wrong, the next planning cycle corrects it,
because `soc_init` is re-read from the live asset every cycle.

*The honest limitation this accepts*: pre-charging beyond `target[k-1]` before an
intervening departure cannot be rewarded, because `target[k-1]` caps the chain.
The plan therefore pre-charges across a departure only up to the earlier session's
own target, and reports the rest as shortfall on the later session. This is
recorded in `ev-session-queue-planning`'s "Pre-charging across an intervening
departure" scenario, and is the right conservative answer for a firm obligation —
promising energy that an unpredictable trip may consume would be the worse failure.
It goes into `docs/reference/TECHNICAL_DEBTS.md` as the known gap, with the
state-variable model as its fix.

*Alternative considered*: keep one obligation and simply pick the most binding
session. Rejected — it is today's behaviour with extra bookkeeping, and cannot
satisfy two different targets at two different departures.

### Decision 5 — The comfort curve prices the head session only

`segments` (the priced bands from `ev_comfort::ev_energy_segments`) continues to
be built once, from the live SoC and the head session's curve and target. Queued
sessions behind the head contribute a firm `required_kwh` and no bands.

*Why*: a band's `kwh` is measured from a *known* starting SoC. For a session two
departures out the starting SoC is itself a prediction (Decision 4), so bands
built from it would price energy against a number the solver may invalidate. A
firm requirement does not have that problem: it is a guarantee, and
`declare_vars` already handles "requirement not covered by bands" by adding one
unpriced guarantee band (`ev_milp.rs:60-66`) — the mechanism for exactly this case
already exists and is reused unchanged.

*Alternative considered*: build bands per session from the predicted start SoC.
Rejected for the reason above, and because it multiplies `e_seg` variables by the
number of queued sessions for a valuation that is speculative anyway.

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

## Risks / Trade-offs

- **Opportunistic charging silently pauses forever** once the queue is rarely
  empty → Decision 6 changes the test to "window open now" in the same step as the
  producer; pinned by a scenario asserting opportunistic charging resumes between
  sessions.
- **Pre-charge across a departure is capped by the earlier target** (Decision 4's
  accepted limitation) → recorded in `TECHNICAL_DEBTS.md` with the
  SoC-state-variable model named as the fix; the shortfall is reported per session
  rather than hidden.
- **Solve time grows with queue length** — one energy-floor constraint per
  obligation inside the horizon (at most ~7) → constraints only, no new variables
  except the existing uncovered-guarantee band; re-benchmark against R-97's
  battery+EV figures before merge and record the delta.
- **A wrong open-loop drop prediction mis-sizes a later obligation** → every
  planning cycle re-reads live SoC, so the error is corrected at the next cycle
  rather than accumulating; the firm floor is additionally capped at
  `reachable_energy_kwh` for the window, so a mis-prediction degrades to a reported
  shortfall and never an infeasible site solve.
- **Three producers inserting into one invariant-bearing structure** → the only
  mutator is the checked `insert`; a property test asserts the invariant holds
  after arbitrary interleavings of insert/remove/expire.
- **File-size caps** on `ev_session_context.rs` / `ev_milp.rs` /
  `usage_sim_plan_ahead.rs` (the latter under the 200-line `tasks/` cap) → the
  obligation-chaining arithmetic lands in its own module beside
  `ev_session_context.rs`, and `scripts/audit_file_sizes.py` runs before commit.

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
