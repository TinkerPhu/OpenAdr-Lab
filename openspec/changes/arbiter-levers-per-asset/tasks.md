# Tasks

Branch: `refactor/arbiter-levers-per-asset`.

## 1. Declarations

- [x] 1.1 Test first: battery, EV and heater declare their lever kind and rated range; PV and base
      load declare none; the snapshot carries both and the roster index.
- [x] 1.2 `ArbiterLeverKind`, `Asset::arbiter_lever`, `Asset::rated_power_kw`;
      `AssetSnapshot::{arbiter_lever, rated_import_kw, rated_export_kw, roster_index}`.

## 2. Levers per asset

- [x] 2.1 Test first: a battery named `home-battery` is the lever; two batteries tie → roster
      order; the second takes the rest.
- [x] 2.2 `LeverId`; candidates per declaring asset; apply functions take the asset id; carried
      levers, release and lag bridge per asset; lever wording `<kind>:<asset_id>`.

## 3. EVs and heaters per asset

- [x] 3.1 Test first: two EVs share a surplus in roster order; one EV's setting off; two heaters,
      one curtailed.
- [x] 3.2 `ArbiterTick::ev_overlay` per EV; `resolve_overlay_enabled` per EV; heater emergency
      mode and EV departure per asset through `TickOverrides`.

## 4. Dispatch override and comms loss

- [x] 4.1 Test first: dispatch with a battery named otherwise; comms loss clamps two EVs.
- [x] 4.2 `DISPATCH_POLICY` through `apply_ranked_levers`; comms-loss bounds from the declared
      rated range of every lever asset.

## 5. Checks, docs, merge

- [x] 5.1 Lever-name assertions and UI fixtures; BDD for a renamed battery as lever;
      `audit_step_definitions.py`.
- [ ] 5.2 All checks and suites on Node2.
- [ ] 5.3 Docs (VEN_ARCHITECTURE arbiter section, use case), journal, wiki.
- [ ] 5.4 Delete this change; merge; clean up.
