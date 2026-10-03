# Proposal

## Why

The VEN is told about future EV trips but never asked to plan for them, so the 48 h
forecast understates EV demand by every trip but the first. Live evidence from ven-12
(2026-10-03, 48 h horizon, two predicted trips): the plan charges once overnight to
0.800, then holds flat through the Sunday trip to 0.698 and through the Monday trip to
0.570, with no charging in either gap. Every one of the 12 EV-bearing fleet profiles
declares `usage_forecast`, and that class emits exactly **one** `EvObligation` — from
`next_trip_after`, the next departure only. Later trips in the horizon get a truthful
availability mask and a projected SoC drop, but no charging goal, so nothing motivates a
recharge. Forecasting the next 48 h of energy use with simulated EV trips was the entire
point of the usage classes; today it is a one-trip forecast with a decorative tail.

Separately, a user's stated trip consumption is both invented and mistimed. When the user
states no distance, `EvCharger::expected_trip_drop(None)` substitutes a configured
`default_trip_distance_km` and projects a drop nobody asked for. When the user *does*
state one, it is placed at the **next** session's `window_start` — the next plan's
"Available from" read as this trip's return time — because the UI never collects a return
time. Via `sessions.windows(2)` that means a lone session's distance does nothing at all,
and the last session of any queue has its distance silently ignored. The field appears to
work and mostly does not.

## What Changes

- **One primitive for a series of trips.** A single function turns a series of
  *(charge window, target at departure, consumption at return)* entries into the three
  quantities the MILP consumes — `a_ev`, `soc_drops`, `obligations`. The session queue and
  the trip generator become its two callers instead of each deriving all three
  independently (`one-concept-one-function`; today the session path has its own
  `availability_from_sessions` / `obligations_from_sessions` /
  `trip_drops_between_sessions` and the forecast path its own
  `availability_per_slot` / `soc_drop_frac_per_slot` / single-obligation assignment).
- **Every known departure in the horizon binds its own target.** `usage_forecast` walks all
  predicted trips in the horizon rather than only the next, so each departure gets an
  obligation and the gaps between trips become plannable recharge windows. The mechanism
  already exists — `obligations` is a list and `soc_ev[t]` is a per-slot variable — so this
  is a loop over an existing contract, not a new one.
- **A user's trip estimate is optional, complete, and self-timed.** `EvSession` gains an
  expected return time alongside its expected distance. The two are a *pair*: both given →
  the drop is projected at the stated return; neither given → **no drop is projected at
  all**, and the plan holds the SoC flat until the real return is measured. One without the
  other is refused at the route boundary with a typed `DomainError`, because a distance with
  no return time is energy with nowhere to land.
- **BREAKING (profile):** `default_trip_distance_km` is **removed**. With no silent
  fallback there is nothing to default to. `consumption_kwh_per_km` stays — it is the km→kWh
  conversion and is still required.
- **Removed:** `ExpectedTripDrop.defaulted`, the provenance flag that existed only to
  surface the fallback being deleted, and the `any_defaulted` plumbing that carried it.
- **Removed:** the `sessions.windows(2)` coupling. Each session dates its own trip, so the
  lone-session and final-session holes disappear by construction rather than being patched.
- **UI:** the EV plan dialog gains an optional "Back by" input beside "Trip after departure
  (km)", presented as one optional estimate rather than two independent fields.
- **Documentation is rewritten, not annotated.** The sections describing the two usage
  classes assert that the forecast class lets "the solver plan the recharge the *return*
  makes possible". The solver *may* (the mask permits it) but is never *asked* to. That
  claim and its neighbours are replaced wholesale so no future reader can inherit the old
  model from surviving prose.

Explicitly **out of scope**: the `usage_sim` / `usage_forecast` naming, which inverts
intuition (the class named "sim" is the one that writes sessions; the class named "forecast"
writes none). Recorded as debt, renamed separately — a rename touching every profile, doc and
test would bury this change's behaviour in noise.

## Capabilities

### New Capabilities

- `ev-trip-series-planning`: the planner plans for every EV trip it knows about inside its
  horizon — each departure binding its own target SoC, each return opening a recharge
  window, and one shared derivation serving both the stated-session and predicted-trip
  producers.
- `ev-user-trip-estimate`: a user may state what a trip will cost — distance and return time
  together, or neither — and the planner projects a consumption drop only from a complete
  estimate, never from a substituted default.

### Modified Capabilities

None. `openspec/specs/` is empty: implemented capabilities are waved into `docs/` and their
specs deleted (workflow rule 3), so there is no existing spec file to delta against.

## Impact

**Behaviour.** Fleet EV plans change shape: VENs with more than one trip in the horizon will
charge between trips where they previously coasted, raising forecast EV demand and shifting
it into the cheapest inter-trip slots. Plans for user sessions that stated a distance but
have no following session will *lose* a drop they previously showed only when a later
session happened to supply the time; plans for sessions that stated nothing lose an invented
drop. Both are corrections, and both are visible in the Controller EV chart.

**Code — EV asset and planner inputs**
- `VEN/src/assets/ev_schedule.rs` — `expected_trip_drop` takes a required distance; the
  default fallback and `ExpectedTripDrop.defaulted` go.
- `VEN/src/assets/ev_session_context.rs` — the three `*_from_sessions` helpers collapse into
  calls to the shared primitive.
- `VEN/src/assets/ev_usage_forecast.rs` — `target_next_predicted_departure` becomes a walk
  over every trip in the horizon.
- `VEN/src/assets/ev.rs`, `VEN/src/assets/mod.rs` — `EvCharger` field removal.

**Code — entity, validation, routes, state**
- `VEN/src/entities/device_session.rs` — `EvSession` gains the expected return time; the
  both-or-neither invariant.
- `VEN/src/entities/error.rs` — a `DomainError` variant for an incomplete trip estimate.
- `VEN/src/entities/asset_params.rs`, `VEN/src/profile/{schema,defaults,validate}.rs` —
  `default_trip_distance_km` removed from the profile contract.
- `VEN/src/controller/user_request.rs`, `VEN/src/services/user_request.rs`,
  `VEN/src/routes/hems/sessions.rs` — carry and validate the pair.
- `VEN/src/tasks/sim_tick/usage_sim_plan_ahead.rs` — states each simulated trip's real
  generated consumption and return instead of `None` plus a comment claiming otherwise.
- `VEN/src/tasks/{poll_signals,sim_tick/arbiter_glue,vtn_charge_state_session}.rs`,
  `VEN/src/state/mod.rs` — construction sites.

**UI**
- `VEN/ui/src/components/devices/EvCard.tsx`, `VEN/ui/src/api/types.ts`, and the four
  affected suites under `VEN/ui/src/__tests__/`.

**Tests**
- `tests/features/ev_usage_forecast.feature` — a VEN with two trips in 48 h charges before
  **both** (the scenario nothing currently asserts).
- `tests/features/ev_usage_simulation.feature`, `tests/features/ven_user_request.feature` —
  the estimate pair, and the absent-estimate flat hold.
- `VEN/src/controller/milp_planner/tests/*` — the six suites constructing sessions.

**Documentation rewritten**
- `docs/architecture/VEN_ARCHITECTURE.md` — the session→MILP field table and the "EV usage
  simulation" / "EV usage forecast" sections.
- `docs/use-cases/HEMS-USE-CASE-OBSERVATION-MANUAL.md` — UC-17, UC-18 and the usage-class
  observation guide.
- `docs/reference/TECHNICAL_DEBTS.md` — the R-92/R-93 resolution note, which reads as though
  both producers were widened when only the mechanism was; plus new entries for the usage
  class naming inversion and ven-2's horizon-edge knife edge.
- `docs/history/project_journal.md` — the 048/049 entries, rewritten to the corrected model.
