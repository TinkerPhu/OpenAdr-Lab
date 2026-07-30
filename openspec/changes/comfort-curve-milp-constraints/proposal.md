## Why

The comfort-curve override path is fully live end to end — VEN UI sliders, `POST/GET/DELETE
/hems/comfort` routes, `services/comfort.rs` validation and `SettingsPort` persistence, and
per-session resolution via `effective_comfort_rates()` into `AssetRequestSlice.comfort_rates`.
But `controller/user_request.rs::create_from_body` resolves the curve into a local
`_comfort_rates: Vec<ComfortRate>` and then never uses it — `UserRequest` has no field to carry
it, so it's discarded. Downstream, the MILP context builders (`EvMilpContext::from_state`,
`HeaterMilpContext::from_state`) never receive it either. The result: every comfort-curve
slider in the UI is a fully silent no-op — the user can move it, save it, and the plan the
solver produces is identical to using `default_comfort_rates()`. This is a broken user-facing
feature, not a nice-to-have, and BL-34 is the highest-gain unresolved backlog item.

## What Changes

- Add a `comfort_rates: Vec<ComfortRate>` field to `UserRequest`, `EvSession`, and
  `HeaterTarget` so the resolved curve survives from the HTTP route through to the planning
  cycle instead of being dropped in `create_from_body`.
- Thread `comfort_rates` through `simulator/plan_context.rs::build_asset_contexts` →
  `AssetConfig::build_milp_context` → `EvMilpContext::from_state` /
  `HeaterMilpContext::from_state`, adding a `comfort_rates: Vec<ComfortRate>` field to both
  MILP context structs.
- Replace the fixed, planner-wide reward constants that currently stand in for user intent —
  the heater's `w_tier_penalty_eur` bias between `z_heat_mid`/`z_heat_full`, and the EV's
  `v_ev_core_eur_kwh`/`v_ev_extra_eur_kwh` — with per-session values derived from the
  resolved `ComfortRate` curve's `max_marginal_price` at each curve breakpoint, inside each
  asset's existing `objective()` implementation (`heater_milp.rs`, `ev_milp.rs`). No new MILP
  variables or constraints; the existing tier/core/extra structure is reused, only the reward
  coefficients that feed it change source.
- Sessions with no curve override continue to fall back to `default_comfort_rates()`
  (unchanged behavior — this is the existing fallback in `effective_comfort_rates()`, not a
  new mechanism).

## Non-goals

- The legacy direct routes `POST /ev-session` (`routes/hems/ev.rs`) and `POST /heater-target`
  (`routes/hems/heater.rs`), which build `EvSession`/`HeaterTarget` without going through
  `create_from_body` at all: they were already curve-blind before this change (no
  `comfort_rates` concept in their request bodies), and their corresponding UI hooks
  (`usePostEvSession`, `usePostHeaterTarget`) are unused by any page/component — the VEN UI's
  only session-creation path is `usePostRequest` → `POST /user-requests`. Leaving the direct
  routes unchanged is not a regression.
- Battery, PV, and base-load assets: they have no session-intent path through
  `create_from_body`/`UserRequestService` (no `create_battery`/equivalent exists) — their
  `default_comfort_rates()` implementations are unused dead code today and stay that way;
  wiring them in is out of scope for this change.
- No new MILP variables, tiers, or constraints — this change only changes where existing
  reward coefficients come from (session-derived vs. fixed `PlannerParams`), matching the
  BACKLOG item's own framing ("no solver-objective *structure* change").
- No changes to `services/comfort.rs` validation, persistence, or the `/hems/comfort` routes
  — that path is already correct and tested; this change only fixes what happens to the
  curve once a session is created.
- No changes to openleadr-rs or any OpenADR 3.1 spec surface — this is entirely internal VEN
  planning logic, not an OpenADR-facing capability.

## Capabilities

### New Capabilities
- `session-comfort-curve-planning`: the resolved per-session `ComfortRate` curve shapes the
  MILP planner's tier/reward coefficients for EV and heater sessions, so different curves
  produce different allocations for otherwise-identical sessions.

### Modified Capabilities
(none — no existing `openspec/specs/` capability currently documents this planner behavior)

## Impact

- **Affected code**: `VEN/src/controller/user_request.rs`, `VEN/src/entities/user_request.rs`,
  `VEN/src/entities/device_session.rs` (`EvSession`, `HeaterTarget`), `VEN/src/simulator/plan_context.rs`,
  `VEN/src/controller/milp_planner/asset_port.rs` (`EvMilpContext`, `HeaterMilpContext`),
  `VEN/src/assets/heater_milp.rs`, `VEN/src/assets/ev_milp.rs`.
- **Affected containers**: VEN only (no VTN, BFF, or UI changes — the UI already sends/reads
  comfort curves correctly today).
- **Dependencies**: none new.
- **Tests**: new/updated unit tests in `VEN/src/controller/milp_planner/tests/` (planner-level:
  two identical sessions with different curves produce different allocations) and in
  `heater_milp.rs`/`ev_milp.rs`'s existing `#[cfg(test)]` modules (objective-expression-level,
  following the existing `format!("{obj:?}")` comparison pattern used for terminal-reward
  tests).
