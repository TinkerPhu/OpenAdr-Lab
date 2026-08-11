## ADDED Requirements

### Requirement: Learned base-load heuristic tracks its own recent prediction error
`learn_asset_heuristics` SHALL compute, alongside the existing
`daytime_profile_kw` and `seasonal_factor`, a recency-weighted mean absolute
error between each tick's actual `power_kw` and what the *previously
learned* heuristic (the one in effect at that tick's time, if any) would
have predicted for it, and store the result on `AssetHeuristics` as an
additive field. This SHALL NOT change `daytime_profile_kw`, `seasonal_factor`,
or `sample_kw`'s output for any existing input — purely additive
instrumentation, not a behavior change to any current consumer.

#### Scenario: Stable pattern yields low recent error
- **WHEN** `learn_asset_heuristics` runs over 4+ weeks of synthetic backfill
  generated from a stationary power model (same shape used to seed the
  previous run's profile)
- **THEN** the computed recent-error field is low (near zero, within the
  synthetic model's own noise floor)

#### Scenario: A shifted pattern yields a higher recent error than a stable one
- **WHEN** `learn_asset_heuristics` runs over history where the most recent
  portion's actual power diverges materially from what a heuristic learned
  on the older portion alone would have predicted (e.g. a step change in
  baseline load)
- **THEN** the computed recent-error field is measurably higher than in the
  stable-pattern scenario above, using the same window and config

#### Scenario: Feedback field is additive and does not change existing sampling behavior
- **WHEN** `AssetHeuristics::sample_kw` is called on a heuristic that now
  carries the new recent-error field
- **THEN** its return value is identical to what it would have been before
  this field existed, for the same `daytime_profile_kw`, `seasonal_factor`,
  and `slot_t` inputs
