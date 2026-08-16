# Test Report: CO2-aware comfort bidding + BL-17 closeout + PV embodied carbon

2026-08-16. `feat/co2-comfort-bidding`. Originated from a coverage-gap-driven
unit test on `controller/user_request.rs` that proved `ComfortRateInput`
hardcoded `max_marginal_co2: 0.0` — no user-facing way to express a CO2
preference existed anywhere in the codebase. Investigating the believed
blocker (BL-17, "grid CO2-intensity forecast ingestion") found it was already
~95% implemented via the existing generic OpenADR rate-event mechanism; the
real gap was staleness-policy parity and missing tests, not new ingestion.
Three phases followed: **A** closed out BL-17, **B** built the CO2 comfort-bid
reward, **C** added PV's own embodied-carbon number as reporting-only ledger
data.

Full VEN suite: 1074 tests (up from 1052 pre-branch), all passing. `cargo fmt
--check`, `clippy --all-targets --all-features -- -D warnings`,
`scripts/audit_file_sizes.py`, and the four architecture-invariant greps
(`ven-architecture` rule) all clean on Node2. VEN UI: 585 tests (`npm test`),
`eslint` 0 errors, `npm run build` clean. E2E/BDD: 277 scenarios, 0 failed, on
Node2 (`run_all_tests.sh --e2e`).

## Phase A — BL-17 closeout (`controller/milp_planner/`)

- `test_parse_rate_snapshots_ghg_multi_interval`
  (`openadr_interface.rs`) — a 3-interval GHG event parses into the same
  shape as `PRICE`/`EXPORT_PRICE` siblings, confirming the shared
  `collect_interval_groups` pipeline genuinely has no GHG-specific gap.
- `test_co2_coverage_independent_of_import_coverage`
  (`tests/stale_rates.rs`) — CO2 and import tariff data can end at different
  times; each raises its own independent stable-text warning
  (`co2_stale_rate_warning` / `stale_rate_warning`), not a shared one.
- **`battery_arbitrage_driven_by_ghg_intensity_alone`** (`tests/solver.rs`,
  the critical test): flat tariff (no price signal at all), varying grid CO2
  intensity across 4 slots, `w_ghg = 5.0`. Proves the battery arbitrages
  purely on the carbon signal — charges in the clean slots, discharges in the
  dirty ones — i.e. `g_imp_kgco2_kwh × w_ghg` is load-bearing in the solved
  allocation, not just present in the objective expression.

One pre-existing test (`solve_ven3_heater_three_tier_zones_feasible`) had its
warning filter broadened from an exact tariff-specific string match to the
shared "ends before the planning horizon" suffix both messages now use — the
new independent CO2 warning was a correct new signal the old filter didn't
anticipate, not a regression.

## Phase B — CO2 comfort bidding (`entities/asset.rs`, `assets/heater_milp.rs`, `assets/ev_milp.rs`, `assets/ev_comfort.rs`, `controller/user_request.rs`)

**Curve interpolation** (`entities/asset.rs`): `value_at_fill`'s test set
(exact breakpoint, midpoint interpolation, clamp outside range, single-point
curve) mirrored exactly for the new `co2_value_at_fill`, both now sharing one
`interpolate_at_fill` helper.

**Request API** (`controller/user_request.rs`): `ComfortRateInput` gained
`co2: Option<f64>`. The prior report's
`comfort_rates_explicit_override_replaces_asset_default_and_zeroes_co2_bid`
test's premise ("no user-facing CO2 field exists") became false once this
field was added; it was rewritten into two tests reflecting the new correct
behavior —
`comfort_rates_explicit_override_replaces_asset_default_and_co2_defaults_to_zero_when_omitted`
and `..._co2_bid_is_passed_through_when_given` — rather than silently
weakened, per this repo's test-failure rule.

**Per-asset reward wiring** (`heater_milp.rs`, `ev_milp.rs`,
`ev_comfort.rs`): `from_state_sources_comfort_full_co2_reward_from_curve`
(Heater) and `non_empty_curve_sources_price_and_co2_from_fill_0_and_1` (EV,
`ev_comfort.rs`) hand-verify the monetization arithmetic (e.g. 200 gCO2/kWh ×
0.5 EUR/kgCO2 = 0.10 EUR/kWh) at construction time — the objective itself
only ever sees €, never gCO2/kWh. `test_comfort_full_co2_reward_phase1_objective_unaffected`
/ `..._shapes_phase2_objective` confirm the same phase-gating already proven
for the price reward (BL-34) — zeroed in Phase 1, active in Phase 2 only.

**`heater_co2_comfort_bid_shapes_phase2_full_tier_usage`** (`tests/solver.rs`,
the critical test — the exact class of bug the BL-34 postmortem in
`KEY_LEARNINGS.md` warns about, "syntactically correct and semantically
inert"): two otherwise-identical Phase 2 solves differing only in
`comfort_full_co2_reward_eur_kwh` (0.0 vs 0.50 EUR/kWh), with a small nonzero
`w_tier_penalty_eur` (0.05) so the zero-reward baseline has a real reason to
stay off full tier rather than landing on an arbitrary LP tie. Asserts the
reward measurably shifts full-tier usage from 0 slots to materially more —
proving the CO2 bid actually reaches the solved plan, not just the objective
expression.

**BDD** (`tests/features/steps/comfort_steps.py`,
`tests/features/ven_comfort_curve.feature`): `_parse_points` extended to
accept an optional `:co2` segment (default 0.0, every existing scenario
unchanged). New `@use_case` scenario mirrors the existing BL-34 price
scenario but isolates the CO2 axis — price bid held at 0.0 throughout, an
extreme gCO2/kWh bid (needed because the default `w_ghg` planner weight is
tiny) flips EV charging commit on/off purely via CO2.

**UI**: `ComfortCurveCard.test.tsx` gained a CO2-field render + save-payload
test; new `CurveChart.test.tsx` (4 tests) covers the price+CO2 dual-Y-axis
generalization, including a caller-overridden series list.

## Phase C — PV embodied carbon, reporting only (`entities/asset_params.rs`, `profile/`, `controller/monitor.rs`)

`monitor.rs`'s `record_tick` gained 3 new tests:

- `pv_generation_accumulates_embodied_co2_when_pv_co2_g_kwh_is_set` — 5 kWh
  generated × 40 gCO2/kWh = 200 g, hand-verified.
- `pv_generation_leaves_co2_g_at_zero_when_pv_co2_g_kwh_is_unset` — no
  behavior change from before this feature when the coefficient is 0.0.
- `non_pv_exporting_asset_does_not_accumulate_embodied_co2` — the term is
  keyed on `asset_id == ASSET_PV` specifically; a discharging battery (also
  negative power) must not pick it up. This is the test that would catch a
  "any exporting asset" bug instead of the intended PV-only one.

`profile/validate.rs` gained `validate_fails_for_negative_pv_co2_g_kwh` /
`validate_passes_when_pv_co2_g_kwh_unset_or_zero`, matching the existing
`inverter_max_kw` validation pattern exactly.

No new UI component: `GET /ledger`'s existing generic per-asset table
(`Dashboard.tsx`, `entries.map((l) => ...)`) already renders every asset's
`co2_g` uniformly with no PV-specific branching — checked directly before
concluding no UI change was needed.

## What was deliberately not tested

- BL-17's staleness-parity tests exercise the `apply_stale_rate_policy`
  machinery directly; they don't re-prove every `StaleRatePolicy` variant's
  behavior for CO2 specifically, since that logic is now shared verbatim with
  the already-tested import-tariff path (same function, different inputs).
- The BDD CO2-axis scenario's extreme gCO2/kWh magnitude is deliberately
  unrealistic (needed to compensate for the tiny default `w_ghg` weight,
  documented in the scenario's own comment) — it proves the axis is wired
  end-to-end, not that a plausible real-world CO2 bid produces this outcome.
- Phase C's `pv_co2_g_kwh` threading through `main.rs` → `spawn_sim_tick` →
  `tick_once` → `publish_sim_tick_result` → `record_tick` is exercised
  end-to-end only via `record_tick`'s own unit tests plus the full E2E suite
  staying green; there is no test asserting the specific value resolved from
  a YAML profile survives the whole chain unchanged, since every link is a
  plain by-value `f64` parameter with no transformation in between.
