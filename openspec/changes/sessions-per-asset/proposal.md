# Proposal

## Why

A user request names its asset (`UserRequest.asset_id`), and the asset declares which kind of
session it takes (`RequestKind`). The session then forgets which asset it was for: `EvSession`
and `HeaterTarget` carry no asset id, and `AppState` holds one EV queue, one heater target and one
EV settings record. Every EV context is handed every EV session, every heater context the one
target, and the simulated usage schedule is installed for whichever asset has the id `"ev"`
(`tasks/sim_tick/usage_sim_plan_ahead.rs`, `find_asset(ASSET_EV)`). With one EV and one heater per
profile this is invisible; it is the part of W4 ("sessions, settings, usage simulation and user
requests per asset id") that keeps a session tied to its asset.

## What Changes

- `EvSession` and `HeaterTarget` carry the `asset_id` their request named.
- `AppState` holds EV sessions, heater targets and EV settings per asset id. An EV's queue
  conflicts only with its own sessions.
- The planner hands each asset only its own sessions or target (`MilpBuildInputs`).
- The simulated usage schedule is installed for every EV that predicts one, into its own queue.
- **BREAKING (VEN API, UI migrated in the same change):** `GET/PUT /ev-settings` becomes
  `GET/PUT /assets/{asset_id}/ev-settings`; the EV card passes its own asset id.
- Request cancel and settle find a session in its own asset's queue.

## Capabilities

### New Capabilities
- `sessions-per-asset`: a session, target or settings record belongs to the asset its request
  named, and only that asset plans with it.

### Modified Capabilities
<!-- none under openspec/specs/ -->

## Impact

- **VEN Rust:** `entities/device_session.rs`, `entities/ev_settings.rs`, `state/{mod,ev_sessions,
  user_requests}.rs`, `services/{user_request,request_submission,ev_usage_plan}.rs`,
  `tasks/sim_tick/{usage_sim_plan_ahead,arbiter_glue}.rs`, `tasks/planning/{cycle_state,
  assemble}.rs`, `simulator/plan_context.rs`, `assets/capability_traits.rs`, routes `hems/ev.rs`,
  `hems/sessions.rs`, `routes/mod.rs`.
- **UI:** generated types, `api/client.ts`, `api/hooks.ts`, `EvCard.tsx`.
- **BDD:** steps that read or write `/ev-settings`; one scenario for the per-asset route.
- **Not affected:** plans (goldens hold), the wire to the VTN, persisted state (sessions,
  targets and settings are in memory only).
- **Out of scope:** the arbiter's EV lever and dispatch reading the EV by fixed id (W5), several
  EV or heater cards in the UI (W6), allowing two assets of a kind in a profile (W7).
