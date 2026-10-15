# Design

## Inventory

| Concept | Where today | Becomes |
|---|---|---|
| Which asset a session is for | Lost after `services/user_request.rs::create_ev` / `create_heater` | `EvSession.asset_id`, `HeaterTarget.asset_id`, set from the request |
| EV session store | `AppState.hems.ev_sessions: EvSessionQueue` (one) | `BTreeMap<String, EvSessionQueue>` per EV |
| Heater target store | `AppState.hems.heater_target: Option<HeaterTarget>` | `BTreeMap<String, HeaterTarget>` per heater |
| EV settings | `AppState.hems.ev_settings: EvSettings` | `BTreeMap<String, EvSettings>` per EV, default when absent |
| Planner hand-over | `build_asset_contexts(.., ev_sessions, heat_tgt, ..)` hands every asset all of them | `MilpBuildInputs` gets the asset's own sessions / target, looked up by `entry.id` |
| Simulated usage | `usage_sim_plan_ahead::sync_plan_ahead_session`, `find_asset(ASSET_EV)` | every roster asset's `planned_usage_sessions`, installed under its own id |
| Cancel / settle | `state/user_requests.rs` removes from the one queue / clears the one target | removes from the request's own asset |
| Routes | `GET /ev-session`, `GET/PUT /ev-settings`, `GET /sessions` | sessions listed with their asset id; settings at `/assets/{asset_id}/ev-settings` |
| Tick | `arbiter_glue::resolve_overlay_enabled` reads the one settings record and `current_ev_session` | reads the EV lever's asset (`ids::ASSET_EV`, as the lever itself does until W5) |
| UI | `api/client.ts` `evSettings()`, `hooks.ts`, `EvCard.tsx` | per asset id |

## Decisions

**D1. Per-asset maps in `AppState`, not one queue with an id filter.** A queue's conflict rule is
"one car cannot be in two sessions at once"; per-asset maps make that rule per EV by construction.
`BTreeMap` for a stable listing order.

**D2. `/assets/{asset_id}/ev-settings` replaces `/ev-settings`.** The old route answered for "the"
EV; keeping it would be a second, implicit answer (`no-half-built-features`). The UI moves in
the same change. An asset that is not an EV gets 404.

**D3. The arbiter and dispatch stay on the profile's EV by id (`ids::ASSET_EV`).** Their per-asset
form is W5; this change only gives them the per-asset store to read from.

**D4. No UI multi-card rendering (W6).** The EV card already knows its asset id; it uses it.

**D5. No migration.** Sessions, targets and settings are in memory only (not in
`state/persistence.rs`), so nothing stored lacks the new field.

## Verification

Plans and goldens unchanged (one asset per kind). New unit tests per spec scenario, the BDD
scenario for the settings route, all suites on Node2.
