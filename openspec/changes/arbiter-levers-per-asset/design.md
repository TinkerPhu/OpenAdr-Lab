# Design

## Inventory: where "which asset is the lever" is decided today

| Concept | Today | After |
|---|---|---|
| Lever candidates | `battery_lever`, `ev_lever`, `heater_pause_lever`, `heater_emergency_lever` each `sim.assets.get(ids::ASSET_*)` | one candidate per asset, from `AssetSnapshot::arbiter_lever` (declared by `Asset::arbiter_lever`), in roster order |
| Applying a lever | `apply_battery_lever`/`apply_ev_lever`/`apply_heater_pause_lever` write `ids::ASSET_*` | take the lever's asset id |
| Lever identity | `Lever.id: &'static str` = `LEVER_*`; `active_lever: Option<&'static str>` | `LeverId { kind, asset_id }`; reported as `<kind>:<asset_id>`, site levers as `<kind>` |
| Carried levers | `release::CARRIED_LEVERS = [battery, ev]`; `reconcile` seeds the same two ids | every asset whose lever kind is storage or surplus charging |
| Lag bridge | `drop_last_bridge` on `ids::ASSET_BATTERY`; one `battery_bridge_kw` | bridge per storage asset (`bridge_kw_by_asset`) |
| Opportunistic EV | `apply_ev_lever_opportunistic` on `ASSET_EV`, one `plan_has_ev_allocation`/`overlay_enabled` | every surplus-charging asset in roster order, each with its own allocation and settings (`ArbiterTick::ev_overlay`) |
| Overlay setting | `arbiter_glue::resolve_overlay_enabled` on `ASSET_EV` | per EV, for every asset whose `request_kind` is a charge session |
| Heater emergency | `ArbiterOutcome.heater_emergency_mode: Option<(bool, bool)>`, one `TickInputs` flag pair for all heaters | per asset map; `TickOverrides` carries it per asset |
| EV departure | `ctx.ev_session` of `ASSET_EV` → `TickInputs::ev_departure_time` for all EVs | per asset map |
| Dispatch override | `apply_dispatch_override` sets `ASSET_BATTERY` absolutely | `apply_ranked_levers` under `DISPATCH_POLICY` (storage only, objective ignored) |
| Comms-loss bounds | table of three ids with `.val("max_charge_kw")` keys | every lever asset, from its declared rated range (`AssetSnapshot::rated_import_kw/rated_export_kw`) |
| Measured feeds | `measured_kw_for` table on `ASSET_PV`/`ASSET_BASE_LOAD` | unchanged here (W5b) |

Reused: `rank_levers` (unchanged; its stable sort keeps roster order on equal cost),
`apply_ranked_levers` (now also the dispatch override's engine), `current_setpoint_kw`,
`drawn_kw`, `setpoint_shift_kw`, `projected_net_kw_except`, `dispatcher::plan_allocates`.

## Decisions

- **D1 Ranking (user: A).** One lever per asset, ranked by marginal cost; ties by roster order.
  Today every storage lever's cost is the slot's site marginal cost (wear is a site planner
  parameter, `c_bat_wear_eur_kwh`), so two batteries tie and roster order decides. A per-asset
  cost is a later change on the lever's cost, not on this structure.
- **D2 Dispatch override (user: A).** The window's setpoint is a target; deviation =
  projected net − setpoint, filled by `apply_ranked_levers` under `DISPATCH_POLICY` whose only
  admitted kind is storage and which ignores `MaxRevenue`. With one battery the result equals
  today's absolute assignment (`current − deviation`, clamped to the battery's range).
- **D3 Opportunistic order (user: A).** Roster order. Each EV takes
  `power_when_command_lands_kw(remaining surplus)`, the next EV sees the surplus minus that.
- **D4 Comms loss (user: yes).** Every asset declaring a lever, clamped to
  `pct × rated` both ways (`rated_export_kw` is 0 for a load). Rated range is a new declared
  pair, not the live capability: the live ceiling drops to 0 near full and would change today's
  clamp.
- **D5 Roster order** travels as `AssetSnapshot::roster_index`, set where the snapshot is built
  (`SimState::to_sim_snapshot`). Lever candidates are collected sorted by it.
- **D6 Lever kinds** are an enum in `entities/asset.rs` (`ArbiterLeverKind { Storage,
  SurplusCharge, PausableLoad }`), next to `EmergencyWhatIfs`; assets declare, the arbiter maps
  each kind to its candidate/apply pair as data (`declare-dont-branch`).
- **D7 Wording.** `<kind>:<asset_id>` keeps the kind first, so the UI text stays readable and
  greppable from `LEVER_*`. `limit::SWITCHING_LEVERS` matches on the kind.

## Risks

- The live arbiter is behaviour-critical. Single-asset parity is pinned by the existing
  arbiter, limit and tick tests (only lever-name assertions change), and checked on a live VEN
  after merge.
