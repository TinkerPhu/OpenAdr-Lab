## ADDED Requirements

### Requirement: Base-load tick value follows a 3-tier precedence
`SimState::tick`'s `BaseLoad` arm and `peek_base_load_kw` SHALL resolve each
tick's natural (pre-override) base-load power in this precedence order:
1. A fresh real-measurement reading (`base_load_measured_kw`, `Some` only
   when a reading has been received and is not stale, per
   `MEASUREMENT_STALENESS_THRESHOLD`).
2. The site's learned base-load heuristic, sampled at the tick's `now` via
   `AssetHeuristics::sample_kw`, when a heuristic has been learned for
   `ids::ASSET_BASE_LOAD` (i.e. `learn_asset_heuristics` has cleared its
   cold-start gate at least once).
3. The synthetic spike model
   (`bl.baseline_kw_profile + bl.appliance_noise_kw(now)`), used only when
   neither tier 1 nor tier 2 is available.

#### Scenario: Fresh measurement wins over both other tiers
- **WHEN** a tick runs with `base_load_measured_kw = Some(2.3)` and a learned
  heuristic is also present for `base_load`
- **THEN** the resolved natural base-load power is `2.3`, not the
  heuristic's or the synthetic model's value

#### Scenario: Stale/absent measurement with a learned heuristic falls back to the heuristic
- **WHEN** a tick runs with `base_load_measured_kw = None` and a learned
  `AssetHeuristics` is present for `ids::ASSET_BASE_LOAD` at the tick's `now`
- **THEN** the resolved natural base-load power equals
  `AssetHeuristics::sample_kw(now)` for that heuristic, not the synthetic
  spike-model formula

#### Scenario: No measurement and no learned heuristic falls back to the synthetic model (cold start)
- **WHEN** a tick runs with `base_load_measured_kw = None` and no
  `AssetHeuristics` entry exists for `ids::ASSET_BASE_LOAD`
- **THEN** the resolved natural base-load power equals
  `bl.baseline_kw_profile + bl.appliance_noise_kw(now)`, matching prior
  (pre-change) behavior exactly

### Requirement: `SimState::tick` and `peek_base_load_kw` resolve the heuristic tier identically for the same tick
Both functions SHALL be given the same heuristic-sampled value for a given
`now`, resolved once (in `resolve_tick_context`, before the simulator lock is
taken) and passed to both call sites — not independently queried by each
function.

#### Scenario: Preview and committed tick agree during a dropout
- **WHEN** `peek_base_load_kw` is called with the same `now`, override state,
  and resolved heuristic value that `SimState::tick` is about to be called
  with in the same tick cycle, and no fresh measurement is available
- **THEN** `peek_base_load_kw`'s returned value equals the `base_load` asset
  entry's `last_power_kw` after `SimState::tick` runs, within floating-point
  tolerance — matching the existing parity guarantee that already holds when
  a measurement is fresh or absent-with-no-heuristic
