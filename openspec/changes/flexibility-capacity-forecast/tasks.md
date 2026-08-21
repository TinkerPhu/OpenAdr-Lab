## 1. Confirm OpenADR payload type and open questions

- [x] 1.1 Search VTN UI event/report forms and `scripts/seed_vtn.py` for any existing use of
      `STORAGE_MAX_DISCHARGE_POWER`/`STORAGE_MAX_CHARGE_POWER`/`UP_REGULATION_AVAILABLE`/
      `DOWN_REGULATION_AVAILABLE`; if found, reuse that enum for continuity — no live wiring found
      in VTN UI or `seed_vtn.py`, but `docs/REQUIREMENTS.md:164-168` already lists
      `STORAGE_MAX_CHARGE_POWER`/`STORAGE_MAX_DISCHARGE_POWER` as payload types "used in this lab"
      (part of the documented intended set), and their semantics ("maximum sustainable power that
      can be charged/discharged") match this feature exactly — more specific than the
      REGULATION_AVAILABLE pair, which is framed around ancillary-service regulation, not storage
      capacity
- [x] 1.2 Decision: import-commitment curve → `STORAGE_MAX_CHARGE_POWER`; export-commitment curve
      → `STORAGE_MAX_DISCHARGE_POWER`, both with `readingType: FORECAST`
- [x] 1.3 Decision: one response for both directions (matches `SiteHeadroomChart`'s existing
      up/down pairing convention; single fetch for the new UI chart)

## 2. Domain entities

- [x] 2.1 Write unit tests for a new `CapacityCurve` entity type (direction, ordered
      `(elapsed, power_kw)` step points, cumulative energy total) — confirm they fail
- [x] 2.2 Add `CapacityCurve` (and `CapacityCurveStep`) to `VEN/src/entities/` (new file,
      `entities/capacity_curve.rs`); make tests pass — 4/4 tests green
- [x] 2.3 (discovered during 2.x) `GridSnapshot` (`controller/simulator_port.rs`) did not expose
      the `Grid` asset's `import_limit_kw`/`export_limit_kw` (VTN-announced DOE limits, default
      unbounded) needed for task 4's site-cap clipping — added both fields, populated in
      `simulator/snapshot.rs::to_sim_snapshot` from `self.grid_asset.state`, and updated all 12
      existing `GridSnapshot { .. }` construction sites across `VEN/src/` (mocks/tests default to
      `f64::MAX`/`-f64::MAX`, i.e. unbounded, matching `Grid::new()`'s own default). Full workspace
      `cargo check --all-targets` clean afterward.
- [x] 2.4 (discovered during 5.x, done early since it's a one-line-per-field entity change) EV's
      `state_values()` was missing `max_discharge_kw` and `min_soc` (had `soc_target`/
      `battery_kwh`/`max_charge_kw`/`min_charge_kw` already); added both with a regression test.
      Heater's `state_values()` already exposed everything needed
      (`thermal_mass_kwh_per_c`/`temp_min_c`/`temp_max_c`/`max_kw`/`mid_kw`) — no change needed
      there.

## 3. Per-asset contribution formulas (closed-form, no simulation)

- [x] 3.1 Write unit tests for battery contribution: export energy `(soc-min_soc)*capacity_kwh`,
      import energy `(1-soc)*capacity_kwh`, duration = energy/rated power, efficiency applied to
      charge side only, zero contribution at bound — confirm they fail, then implement
- [x] 3.2 Write unit tests for EV contribution: import energy bounded by `soc_target` (NOT 1.0 —
      regression guard against reusing `AssetConfig::available_storage_kwh` unmodified), export
      energy bounded by `min_soc`, unplugged EV contributes zero to both directions — confirm they
      fail, then implement
- [x] 3.3 **Corrected during design review — heater contributes to both directions, not
      import-only:** write unit tests for heater import contribution (reservoir-bound: energy
      `thermal_mass_kwh_per_c*(temp_max_c-temp_c)`, zero at `temp_max_c`, duration divided by
      applicable stepped tier, steps down to 0 on exhaustion) AND export contribution
      (constant-term: current `power_kw`, held for the whole horizon, no step-down) — confirm they
      fail, then implement
- [x] 3.4 Write unit tests for PV contribution: down = forecast/planned output at elapsed `t`, up =
      forecast ceiling itself at `t` (corrected during design review — NOT ceiling minus current,
      which would under-count already-flowing export), contribution follows forecast shape (no
      exhaustion step-down), sourced via `AssetForecastFrame`s built the same way
      `envelope_forecast.rs` already builds them for PV — confirm they fail, then implement
- [x] 3.5 Write unit tests for shiftable-load placement: single step at earliest valid elapsed time
      within `[earliest_start, latest_end]`, excluded from further placement in the same
      direction's curve once placed, already-run/already-running loads contribute zero — confirm
      they fail, then implement (reuse `envelope_forecast.rs`'s `already_run`/
      `valid_start_exists_at` — make both `pub(crate)`; `is_planned_running_at`/
      `has_later_valid_start` are NOT needed and NOT reused, see scope note below)
      **Scope correction found during design review (not yet coded):** a shiftable load only
      contributes to the **import-commitment (down) curve** — starting it can only ever increase
      draw, never help a sustained-export commitment. The `up`/export-side lever from
      `envelope_forecast.rs` (deferring a load the *plan* already intends to run imminently) is
      NOT modeled here — it depends on `Plan.planned_kw_by_asset`, a dependency this module
      deliberately avoids for asset physics (design.md decisions 1–2), and extending it to
      shiftable-load scheduling specifically would reintroduce that dependency for one asset class
      only. Documented as an explicit gap, not silently dropped. Also: the placed step must revert
      to 0 after `elapsed_start + duration_min` (a shiftable load runs once for a bounded time, not
      indefinitely) — two events (+power_kw at start, -power_kw at end), not one.
- [x] 3.6 **Corrected during design review:** base load is NOT excluded — write a unit test
      confirming it contributes a constant additive term to the import curve and a constant
      subtractive term to the export curve (its forecasted power, unaffected by the commitment) —
      confirm it fails, then implement
- [x] 3.7 Run `scripts/audit_file_sizes.py`; split per-asset formulas into a separate submodule
      file from the merge/clip pipeline if approaching the 500-line cap — audit passed with a
      single file (`capacity_forecast.rs`), no split needed

## 4. Merge and site-cap clipping pipeline

- [x] 4.1 Write unit tests: combined per-asset sum exceeding `Grid.import_limit_kw` /
      `Grid.export_limit_kw` is clipped at the merged-total level, not per-asset — confirm they
      fail, then implement
- [x] 4.2 **Corrected during design review — was "netting," is really one more additive merge
      term:** write unit tests confirming base load's forecasted power (task 3.6) is summed into
      the same event-based merge as every other contributor (additive on import, subtractive on
      export) rather than applied as a separate post-hoc "netting" pass — confirm they fail, then
      implement
- [x] 4.3 Write unit test: computation accepts an arbitrary start time (not only "now") and
      produces a curve anchored there — implemented with a documented limitation (see
      `capacity_forecast.rs` module + function doc comments): `start` anchors the elapsed-time axis
      for PV/shiftable placement, but battery/EV/heater formulas read `snapshot`'s CURRENT SoC/temp
      as-is rather than a forecasted state at a future `start` — exact for `start == now`,
      first-order approximation otherwise. Producing a genuinely forecasted starting state would
      need forward re-simulation, which this module deliberately avoids elsewhere.
- [x] 4.4 Assemble `controller/capacity_forecast.rs` exposing `compute_capacity_curve`; verified no
      `use crate::assets::` anywhere in the file (only `simulator_port`/`capacity_curve`/
      `device_session` ports/entities) — grep confirms empty match, consistent with the port rule.
      20/20 new tests pass; full workspace `cargo test` (1148 tests incl. these) and
      `cargo fmt --check` both clean; `cargo clippy --all-targets --all-features -- -D warnings`
      run pending confirmation.

## 5. AssetSnapshot / state_values field gaps (design.md decision 8)

- [x] 5.1 Check whether `state_values()` already emits every field the new formulas need — done
      early (see 2.4): EV was missing `max_discharge_kw`/`min_soc`; battery was separately found
      missing `round_trip_efficiency` (needed for the import-direction formula) during 3.x
      implementation. Heater already had everything.
- [x] 5.2 Missing fields added with regression tests: EV (`ev.rs`), battery
      (`battery.rs::round_trip_efficiency`)
- [x] 5.3 Confirmed: `AssetConfig::available_storage_kwh` untouched;
      `capacity_forecast.rs::ev_events`/`battery_events` compute headroom independently, reading
      `AssetSnapshot.values` directly rather than calling the shared helper

## 6. OpenADR report wiring

- [x] 6.1 Added `STORAGE_MAX_CHARGE_POWER`/`STORAGE_MAX_DISCHARGE_POWER` match arms in
      `reporter.rs::build_measurement_report_for_obligation`, inserted BEFORE the generic
      `_ if !obligation.historical => build_forecast_intervals(..)` fallback (which reads plan
      slots — wrong data source for this curve). New `capacity_curves: Option<&(CapacityCurve,
      CapacityCurve)>` parameter threaded through; all 13 existing test call sites updated.
- [x] 6.2 New `build_capacity_forecast_intervals` in `report_intervals.rs`: one interval per curve
      step at the curve's own step boundaries (not resampled onto the obligation grid, same
      reasoning as `build_forecast_intervals`), values in Watts, final step uses OpenADR's
      "P9999Y" infinity duration convention since the curve has no defined end. Test confirms
      per-step Watts values and durations (`PT1H` for a 3600s step, `P9999Y` for the open-ended
      last step). `readingType: FORECAST` is handled generically via `obligation.historical`
      already (same mechanism every other forecast-mode payload type uses) — no new plumbing
      needed for that part.
- [x] 6.3 Covered by `capacity_forecast.rs`'s own `compute_capacity_curve` accepting an arbitrary
      `start` (task 4.3) — the report layer just passes `now` through today; wiring an actual
      future `reportDescriptor.startInterval` value into that `start` parameter is not yet done
      (no evidence any `reportDescriptor` in this codebase currently sets a non-zero
      `startInterval` for ANY payload type — would need its own small follow-up once a concrete
      caller needs it, not invented speculatively here).

## 7. Read route for UI

- [x] 7.1 Added `GET /flexibility/capacity` (`routes/hems/sessions.rs::get_capacity_curves`),
      returning `{"import": CapacityCurve, "export": CapacityCurve}` in one response (design.md
      1.3), 204 before the first tick — mirrors `get_flexibility_forecast`'s existing shape/style
      exactly. No bespoke Rust unit test added: confirmed by grep that NO handler in
      `VEN/src/routes/` has one (`find VEN/src/routes -name "*test*"` empty) — this project tests
      routes at the BDD/E2E layer only (task 10.3), consistent with existing precedent, not a gap
      specific to this change.
- [x] 7.2 `routes/hems/sessions.rs` is 500-line-capped (not `tasks/`'s 200-line cap — that only
      applies to files under `VEN/src/tasks/`); file is ~360 lines after this addition, well under.
      `scripts/audit_file_sizes.py` run in task 9.4 covers final confirmation.

## 8. VEN UI chart

- [x] 8.1 New `CapacityForecastChart.test.tsx`: empty-state before first tick, both direction
      series rendered with `type: "stepAfter"`, curve's own step points preserved through the
      merge (not smoothed), cumulative energy totals shown — 3/3 pass.
- [x] 8.2 New `CapacityForecastChart.tsx` (own file, `SiteHeadroomChart.tsx` untouched) — reuses
      the same `TimeSeriesChart`/`mergeSeries`/`axisDomain`/`unitFormat` primitives as every other
      chart in this codebase (`generic-over-bespoke`). New `CapacityCurve`/`CapacityCurveStep`/
      `CapacityCurvesResponse` types in `api/types.ts`; `capacityCurves()` client method (204→null,
      matching the existing `flexibility()` pattern) and `useCapacityCurves()` hook.
- [x] 8.3 New `CapacityForecastPage` under the Diagnostics nav dropdown (`/capacity-forecast`,
      alongside Metrics/Raw Data/Tasks/Event Log/Measurements/Plan History).
- [x] 8.4 `npm test` (VEN/ui): 53 files / 606 tests pass, including the 3 new ones. `npm run lint`:
      0 errors (11 pre-existing warnings, none from new files). `npm run build`: succeeds
      (tsc + vite, 24.5s; the >500kB chunk-size warning is pre-existing, unrelated to this change).
- [ ] 8.5 Manually verify in browser — **blocked**: requires a running dev server against a live
      VEN backend; deferred alongside the Node1/Node2-dependent tasks below (nodes occupied by
      other sessions per user instruction).

## 9. Full verification

- [x] 9.4 `python scripts/audit_file_sizes.py` — FAILED initially (helpers.rs 216/200,
      tick.rs 203/200, state/mod.rs 503/500) after the capacity-curve wiring landed; fixed by two
      structural extractions (not import-golfing, which fought `cargo fmt`'s own reformatting):
      `finalize_tick_outputs` moved from `helpers.rs` to new `tasks/sim_tick/finalize.rs` (mirrors
      `forecast_wiring.rs`'s own prior split, same file for the same reason); the
      `effective_capacity`+`resolve_pv_generation_limit_kw` composition in `tick.rs` extracted to a
      new `helpers::resolve_pv_limit`. `forecast_wiring.rs`'s `compute_tick_forecast`/
      `compute_tick_capacity_curves` also merged into one `compute_tick_forecasts` sharing a single
      `build_forecast_frames` call (a real efficiency win, not just line-count reduction — avoided
      calling it twice per tick). Re-ran after `cargo fmt` (which reformats multi-line imports) to
      confirm the audit still passes post-formatting — it does.
- [x] 9.1 `wsl cargo fmt --check && wsl cargo clippy --all-targets --all-features -- -D warnings` —
      clippy caught the expected dead-code warnings before the module was wired into the
      report/route layer (tasks 6–7); both clean now that it's fully wired and called.
- [x] 9.2 `wsl cargo test` (full workspace, not narrowed to `-p ven-app` — single-crate workspace):
      1152 passed, 0 failed, plus the architecture invariant test, after the capacity-curve module
      and its OpenADR/route/UI wiring landed.
- [ ] 9.3 Run full suite per `docs/guidelines/TESTING.md` (`bash run_all_tests.sh --e2e`/
      `--resilience`, Node2 preferred) — **blocked**: both Node1 and Node2 are occupied by other
      sessions' test runs; cannot acquire `docker_host_lock.sh`/`wsl_lock.sh` for either host right
      now. Deferred until a node frees up — do not merge to main before this runs green.

## 10. Documentation and cleanup

- [x] 10.1 Added a `project_journal.md` entry: what was built, the five design corrections found
      mid-implementation (EV `soc_target`, heater both-directions, base-load net-power term, PV
      export-ceiling formula, shiftable-load import-only scope), the `GridSnapshot` port gap, and
      the file-size-cap extractions.
- [x] 10.2 Added a `KEY_LEARNINGS.md` section (three durable lessons: verify a shared helper's
      contract against real control logic before reuse, "non-flexible" ≠ "excludable from a net
      quantity," and file-size fixes must be real extractions verified post-`cargo fmt`, not
      import-golfing).
- [ ] 10.3 Add a BDD scenario in `tests/features/` exercising the use case end-to-end (VTN requests
      a capacity forecast report, VEN's reported values match the closed-form computation) —
      **not done this session**: writing it without being able to run it against Node1/Node2 risks
      an unverified scenario; deferred to when a node is free, alongside 9.3.
- [ ] 10.4 Wave this capability into current-state docs (`docs/architecture/VEN_ARCHITECTURE.md`
      §on flexibility/envelope, relevant `docs/use-cases/*.md`) once implemented and tested, then
      delete this openspec change per the project's delete-on-completion workflow — **blocked on
      9.3/10.3**: this project's own workflow rule waves a capability into current-state docs and
      deletes the openspec change only once it's "implemented and successfully tested"; E2E is
      the one test tier not yet run, so the change stays open rather than being waved/deleted
      prematurely.
