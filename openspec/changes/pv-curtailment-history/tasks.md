## 1. Profile schema: inverter AC capability

- [ ] 1.1 Add `inverter_max_kw: Option<f64>` to `PvConfig` (`VEN/src/profile/schema.rs`), `#[serde(default)]`
- [ ] 1.2 Add `inverter_max_kw: f64` to `PvParams` (`VEN/src/entities/asset_params.rs`); resolve at
      the `AssetProfile::Pv(c) => AssetParams::Pv(PvParams { ... })` conversion site
      (`VEN/src/profile/schema.rs`) as `c.inverter_max_kw.unwrap_or(c.rated_kw)`, mirroring the
      heater `temp_safety_max_c` pattern
- [ ] 1.3 Add `inverter_max_kw: f64` to `PvInverter`; set in `from_params`
- [ ] 1.4 Audit all `PvInverter { ... }` / `PvParams { ... }` literal construction sites
      (`grep -rn "PvInverter {" "PvParams {"`) for the new field
- [ ] 1.5 Add profile validation rejecting `inverter_max_kw <= 0.0` (wherever PV profile validation
      currently lives, e.g. `VEN/src/profile/validate.rs`)
- [ ] 1.6 Unit tests: unset `inverter_max_kw` resolves to `rated_kw`; explicit value is respected;
      zero/negative value fails validation

## 2. Physics: clamp DC potential to inverter capability

- [ ] 2.1 `PvInverter::step_inner`: clamp DC potential (`rated_kw × irradiance`, or the
      weather-sourced value) to `inverter_max_kw` before applying any commanded `export_limit_kw`
- [ ] 2.2 Apply the same `.min(inverter_max_kw)` in `forecast_kw_at`, `capability_trajectory`, and
      `build_milp_context` (`VEN/src/assets/pv.rs`)
- [ ] 2.3 Add `inverter_max_kw` to `state_values()` alongside `rated_kw` (always present, live)
- [ ] 2.4 Unit tests: default (`inverter_max_kw == rated_kw`) produces identical output to before
      this change at several irradiance levels; an explicit lower `inverter_max_kw` clips DC
      potential in `step_inner`, the forecast functions, and the MILP input, independent of any
      `export_limit_kw`; a commanded `export_limit_kw` at or above `inverter_max_kw` has no
      additional effect beyond the hardware clamp

## 3. Per-tick limit + source, correctly historical

- [ ] 3.1 Add `export_limit_kw: Option<f64>` and `curtailment_source: PvCurtailmentSource` (new
      small enum: `None | Plan | Capacity`) to `PvState`; set both in `step_inner`
      (`export_limit_kw` from `self.export_limit_kw` as today; `curtailment_source` threaded in —
      see task 4)
- [ ] 3.2 Update `state_values()` to read `state.export_limit_kw`/`state.curtailment_source`
      instead of `self.export_limit_kw`, fixing the historical-inaccuracy bug described in
      design.md Decision 3
- [ ] 3.3 Unit test: a historical `PvState` snapshot with a different `export_limit_kw` than the
      live `PvInverter` reports its own (historical) value via `state_values()`, not the live one

## 4. Tag curtailment source at resolution time

- [ ] 4.1 Change `resolve_pv_export_limit_kw`'s return type (`VEN/src/controller/dispatcher.rs`)
      to carry both the resolved limit and which source won (plan vs. capacity vs. none) — e.g. a
      small `ResolvedPvExportLimit { limit_kw: Option<f64>, source: PvCurtailmentSource }` struct
- [ ] 4.2 Thread the source tag from `tasks/sim_tick/tick.rs` into `SimState::tick()`'s existing
      `pv_export_limit_override` parameter (extend it to carry the source alongside the value) and
      into the `AssetConfig::Pv` match arm in `simulator/mod.rs`
- [ ] 4.3 Unit tests: plan limit strictly tighter → source `Plan`; capacity limit strictly tighter
      → source `Capacity`; equal → `Plan` (plan takes precedence on ties, since it's the
      anticipated case); neither active → `None`

## 5. Long-term persistence

- [ ] 5.1 Add `export_limit_kw: Option<f64>` and `curtailment_source: Option<String>` (or a small
      numeric code) to `TickSample` (`VEN/src/entities/history.rs`)
- [ ] 5.2 Add schema v5 (`VEN/src/history_store/schema.rs`): `ALTER TABLE tick_samples ADD COLUMN
      export_limit_kw REAL; ALTER TABLE tick_samples ADD COLUMN curtailment_source TEXT;`; bump
      `SCHEMA_VERSION` to 5; add the `if version < 5` branch in `migrate()`
      (`VEN/src/history_store/mod.rs`)
- [ ] 5.3 Update `TickSampleRow`, the INSERT statement, and the row-mapping closure for the two new
      columns
- [ ] 5.4 Add accumulation for `export_limit_kw`/`curtailment_source` to `AssetAcc`
      (`VEN/src/tasks/history_sampler/accumulator.rs`) via `snap.val(...)`. Not a mean like
      `soc_pct`: track the highest-priority source seen in the window (capacity > plan > none) and
      the tightest limit value observed for that category (see design.md Decision 7)
- [ ] 5.5 Unit tests: a flushed window with an active limit persists the value and source; an
      uncurtailed window persists null for both; a window containing both a plan-sourced and a
      capacity-sourced sample persists the capacity source, not the plan source or a last-value
      pick; migration test asserts `SCHEMA_VERSION == 5`

## 6. Fix the future-PV plotting inconsistency

- [ ] 6.1 In `controller/timeline.rs`'s PV branch, change `-slot.pv_forecast_kw` to
      `-slot.pv_used_kw` for future points
- [ ] 6.2 Test asserting the future PV timeline point reflects `pv_used_kw` when it differs from
      the forecast

## 7. UI: three-state shading

- [ ] 7.1 In `AssetTimelineChart.tsx`, derive zones from `data`: hardware-capped (neutral) when
      `power_kw ≈ inverter_max_kw` and no tighter commanded limit; imposed curtailment (amber/red
      by source) when `power_kw ≈ export_limit_kw` and `export_limit_kw < inverter_max_kw`;
      rendered via the existing `ReferenceArea` mechanism
- [ ] 7.2 Pass through in `AssetMidSection.tsx` only for `assetId === "pv"`
- [ ] 7.3 UI unit tests covering all three states plus the uncurtailed case

## 8. Documentation & backlog

- [ ] 8.1 Update `DOCUMENTATION.md` (PV profile parameters, history schema, timeline response
      shape)
- [ ] 8.2 Append `docs/history/project_journal.md` entry and `docs/reference/KEY_LEARNINGS.md`
      lessons (notably: rejecting the simulator-only delta model, and the historical-inaccuracy
      bugfix in `state_values()`)

## 9. Verification

- [ ] 9.1 `wsl cargo check` / `wsl cargo test -p ven-app` locally (wsl_lock)
- [ ] 9.2 `cargo fmt --check` and `cargo clippy --all-targets --all-features -- -D warnings`
- [ ] 9.3 `scripts/audit_file_sizes.py`
- [ ] 9.4 VEN UI unit tests + `npm run build`
- [ ] 9.5 Pi4 E2E + resilience suites green (pi4_lock) before merge
