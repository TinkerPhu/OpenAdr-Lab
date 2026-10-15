# Tasks

Branch: `refactor/asset-sessions-per-asset`.

## 1. Sessions carry their asset

- [ ] 1.1 Test first: a request's EV session and heater target carry its asset id.
- [ ] 1.2 `EvSession.asset_id`, `HeaterTarget.asset_id`; set in `create_ev` / `create_heater` and
      by the EV's predicted usage sessions.

## 2. Stores per asset

- [ ] 2.1 Test first: sessions of two EVs do not conflict; cancel removes from its own asset only.
- [ ] 2.2 `AppState` maps for EV sessions, heater targets, EV settings; accessors take the asset
      id; cancel/settle, `current_ev_session`, listings.

## 3. Planner and simulated usage

- [ ] 3.1 Test first: two EVs, one session → only its EV's context has it; the usage schedule of
      an EV named otherwise is installed under its id.
- [ ] 3.2 `build_asset_contexts` hands each asset its own; `cycle_state`/`assemble` carry the maps;
      `sync_plan_ahead_session` installs for every EV.

## 4. Routes and UI

- [ ] 4.1 `/assets/{asset_id}/ev-settings` (404 for a non-EV); old route removed; sessions listed
      with asset ids.
- [ ] 4.2 UI client, hook and EV card per asset id; generated types; npm test, lint, build.
- [ ] 4.3 BDD: the settings route; `audit_step_definitions.py`.

## 5. Checks, docs, merge

- [ ] 5.1 All checks and suites on Node2.
- [ ] 5.2 Docs (VEN_ARCHITECTURE, INTERFACES/API, use case), journal, wiki.
- [ ] 5.3 Delete this change; merge; clean up.
