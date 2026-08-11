## Why

When ven-1's real MQTT base-load feed goes stale, `SimState::tick` falls all
the way back to the synthetic `baseline_kw_profile + appliance_noise_kw(now)`
spike model — an invented curve unrelated to the site — and that fallback
value gets written into `tick_samples` with no origin marker, so the next
`learn_asset_heuristics` run re-learns synthetic-shaped behavior into the
site's own EWMA profile for up to 42 days per dropout (BL-40). Separately,
the learned heuristic itself never checks how wrong its own past predictions
were against measured actuals (R-60). Both gaps sit in the same file
(`services/heuristics.rs`, `AssetHeuristics`) and the same conceptual area —
how the base-load heuristic is used and validated against reality — so this
change addresses both in one pass.

## What Changes

- Add a third fallback tier to `SimState::tick`'s `BaseLoad` arm and to
  `peek_base_load_kw`: measured (fresh) → learned heuristic (`sample_kw`, once
  cold-start has cleared) → synthetic spike model (true last resort, only
  before any heuristic has ever been learned). This mirrors the existing
  measured → weather → sin-model 3-tier precedence already used for PV.
- Thread a new `base_load_heuristic_kw_now: Option<f64>` value through
  `resolve_tick_context` (`TickContext`) and into both `SimState::tick`'s and
  `peek_base_load_kw`'s parameter lists, sourced from
  `state.asset_heuristics().await.get(ids::ASSET_BASE_LOAD).map(|h| h.sample_kw(now))`.
  Both call sites must receive the same value so the existing
  `peek_base_load_kw_matches_tick_output_for_same_now`-style invariant keeps
  holding during a dropout, not just when a measurement is fresh.
- (Stretch, may be split out — see design.md) Give `AssetHeuristics` a
  conservative error-feedback signal against measured actuals, so a
  heuristic's own recent prediction error informs its confidence/weighting,
  rather than the heuristic only ever backfilling blind from history.

## Capabilities

### New Capabilities
- `base-load-heuristic-fallback`: the 3-tier precedence (measured → learned
  heuristic → synthetic) that a live tick's base-load value follows when the
  real-measurement feed is stale or has never been configured.
- `base-load-heuristic-feedback`: the learned base-load heuristic tracking
  its own recent prediction error against measured actuals (stretch scope;
  design.md determines final shape, may be deferred to a follow-up change).

### Modified Capabilities
(none — no existing `openspec/specs/` capabilities are defined for this repo
yet; both capabilities above are net-new)

## Impact

- `VEN/src/simulator/mod.rs` (`SimState::tick`'s `BaseLoad` arm) — 2-tier to
  3-tier fallback chain.
- `VEN/src/simulator/base_load_preview.rs` (`peek_base_load_kw`) — same
  3-tier chain, kept in lockstep with `tick` per the existing
  preview/tick-parity test.
- `VEN/src/tasks/sim_tick/context.rs` (`resolve_tick_context`, `TickContext`)
  — one new async read (`state.asset_heuristics()`), one new field.
- `VEN/src/tasks/sim_tick/tick.rs` — passes the new context field into both
  `peek_base_load_kw` and `sim_guard.tick(...)` call sites.
- `VEN/src/services/heuristics.rs` (`AssetHeuristics`) — stretch: new
  error-feedback field/method (R-60), scoped conservatively in design.md.
- `docs/architecture/real_measurement_mqtt.md` ("Indirect path into the
  forecast") — update once implemented to describe the 3-tier fallback and,
  if in scope, the feedback signal; the "no measured/synthetic provenance
  tag" caveat there is explicitly NOT resolved by this change (still no
  origin marker on `tick_samples` rows) and stays documented as-is.
- No schema, profile, or public-API changes. No new crates.
