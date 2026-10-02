# Proposal

## Why

An EV asset can hold exactly one charging obligation at a time:
`HemsState.ev_session` is an `Option<EvSession>` (`VEN/src/state/mod.rs:114`),
and every writer overwrites it. A household EV has a *sequence* of departures —
Monday's commute, Wednesday's, a weekend trip — and the planner cannot see past
the next one, so it cannot pre-charge for a deadline that lies behind another
departure, nor account for the SoC the car will lose in between.

**Prerequisite: `ev-soc-state-variables`** (R-93/R-92) lands first, giving the EV
MILP per-slot SoC variables and a list of charging obligations instead of one
scalar deadline. This change is then purely about the session layer.

The gap is purely in the obligation layer. The EV's own trip generator
(`VEN/src/assets/ev_schedule.rs`) is already stateless, deterministic and
multi-trip-capable: `availability_per_slot` and `soc_drop_frac_per_slot` already
project *any number* of away-windows and return-drops across a planning horizon.
Only the *storage* of the goal is pinned to a single slot, so the simulated-usage
producer must throw away everything but the nearest trip
(`sync_plan_ahead_session`, `VEN/src/tasks/sim_tick/usage_sim_plan_ahead.rs:62`)
even though the planner it feeds can, after the prerequisite change, hold several.

## What Changes

- **BREAKING** (internal API only, no wire or HTTP contract change):
  `HemsState.ev_session: Option<EvSession>` becomes an ordered queue of
  non-overlapping sessions. `AppState::ev_session()` / `set_ev_session()` are
  replaced by queue-aware accessors; all three producers, the expirer and the
  canceller move onto them.
- `EvSession` gains an explicit **charging window start** (the instant the car
  becomes available for this session). Today a session's window is implicitly
  "from now until `departure_time`", which is only meaningful for the one
  session that is current. A queued session needs its own start, and the gap
  between one session's departure and the next session's start is exactly where
  the car is away and loses charge.
- One **overlap authority**: a single shared predicate deciding whether two
  sessions conflict, plus the insertion rule that keeps the queue ordered and
  non-overlapping. Used by every producer in this change, and reused unchanged
  by the follow-up change that adds human conflict resolution.
- Each queued session inside the planning horizon becomes one entry in the
  **obligation list** the prerequisite change introduced — its departure step and
  its target state of charge. The solver already chains state of charge across
  departures, so pre-charging for a deadline behind an intervening trip needs no
  new planner mechanism here. The comfort curve keeps pricing the head session's
  energy; queued sessions behind it carry firm targets.
- The **simulated-usage producer** becomes the first consumer: instead of
  writing the nearest trip only, it maintains a rolling **7-day** window of
  simulated sessions, topped up each tick from repeated
  `ev_schedule::next_trip_after` calls.
- **Session expiry** (`arbiter_glue::resolve_overlay_enabled`) and
  **cancellation** (`AppState::cancel_request`) become queue operations: expiry
  drops passed sessions from the head, cancellation removes the one session the
  cancelled request owns by id instead of clearing the slot wholesale. The
  latter is a correctness fix the single slot made impossible.
- UI surface for the queue (`ui-transparency`): the EV session list is visible,
  not just the current session.

### Explicitly out of scope

Human-facing conflict resolution — what `POST /user-requests` does when a user
submits a session overlapping one they already have (reject, or prompt
"replace that one?") — is a **separate follow-up change**. It builds on this
change's queue storage and overlap predicate and touches only the route layer
and the UI; it changes neither storage nor the planner. Until it lands, the
user-request producer keeps today's externally observable behaviour — a submitted
session is accepted and displaces any session it overlaps — but it now does so
explicitly: it asks the queue what the submission conflicts with, removes exactly
those, then inserts. That is today's silent overwrite made visible at one site,
and it is the single line the follow-up change replaces with a refusal.

## Capabilities

### New Capabilities
- `ev-session-queue`: an EV asset holds an ordered queue of non-overlapping
  charging sessions; how sessions are inserted, ordered, expired and removed,
  and what the queue guarantees to its consumers.
- `ev-session-queue-planning`: how the queue reaches the planner — one obligation
  per queued session, attributable back to it, and what the plan guarantees and
  reports per session.
- `ev-simulated-session-schedule`: the simulated-usage producer maintains a
  rolling 7-day schedule of simulated sessions derived from the EV's own trip
  generator.

### Modified Capabilities
<!-- openspec/specs/ is empty — this project deletes specs once implemented
     (workflow rule 3), so there is no existing capability to delta against. -->

## Impact

Code:
- `VEN/src/entities/device_session.rs` — `EvSession` window start; queue type.
- `VEN/src/state/mod.rs` — storage, accessors, `cancel_request`.
- `VEN/src/tasks/sim_tick/usage_sim_plan_ahead.rs` — rolling 7-day producer.
- `VEN/src/tasks/poll_signals.rs` — VTN (`CHARGE_STATE_SETPOINT`) producer.
- `VEN/src/routes/hems/sessions.rs` — user-request producer; `GET /user-requests`
  per-request session resolution; `VEN/src/routes/hems/ev.rs` — `GET /ev-session`.
- `VEN/src/tasks/sim_tick/arbiter_glue.rs` — head expiry, `paused_by_active_session`.
- `VEN/src/assets/ev_session_context.rs` — one obligation per queued session
  (the obligation list and its constraints come from the prerequisite change).
- `VEN/src/assets/ev.rs` — `EvCharger.departure_time` tick override; `VEN/src/tasks/planning/cycle_state.rs`, `VEN/src/tasks/sim_tick/context.rs` — carriers.
- `VEN/ui/src/api/types.ts`, `VEN/ui/src/components/sessions/` — queue surface.

APIs: `GET /ev-session` and `GET /user-requests` response shapes gain the queue;
`POST /user-requests` request shape is unchanged in this change.

Dependencies: none added. Hard prerequisite: `ev-soc-state-variables`.
