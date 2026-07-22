## 1. Sim-inject field (data model)

- [ ] 1.1 Add `pv_export_limit_kw: Option<f64>` to `SimInjectState`
      (`VEN/src/entities/sim_inject.rs`), defaulting to `None`, following
      the exact shape of `grid_export_limit_kw` in the same struct.
- [ ] 1.2 Add `pv_export_limit_kw` to `PostSimInjectBody`
      (`VEN/src/routes/sim.rs`), wire it through the existing `merge_f64!`
      macro.
- [ ] 1.3 Unit test: merging a `pv_export_limit_kw` value into
      `SimInjectState` sets/clears the field correctly (mirror the
      existing `grid_export_limit_kw` merge test if one exists, else add
      alongside it).

## 2. Replan trigger

- [ ] 2.1 Add `pv_export_limit_kw` to the sim-inject replan-trigger
      condition in `routes/sim.rs` (the check currently at `sim.rs:232`
      that includes `body.grid_export_limit_kw.is_some()`).
- [ ] 2.2 Unit/integration test: POSTing a changed `pv_export_limit_kw`
      triggers an out-of-cycle replan, same assertion shape as the
      existing `grid_export_limit_kw` replan-trigger test.

## 3. PV physics wiring (the core fix)

- [ ] 3.1 Thread `pv_export_limit_kw` from sim-inject through
      `tasks/sim_tick/tick.rs` into `SimState::tick(...)`'s argument list,
      alongside how `weather_power_kw`/`pv_irradiance_override` are
      already threaded.
- [ ] 3.2 In `simulator/mod.rs`, assign the threaded value onto
      `AssetConfig::Pv(pv).export_limit_kw` each tick (same block that
      currently sets `pv.weather_power_kw`, `~simulator/mod.rs:252-262`).
      Apply the sign conversion (positive magnitude in sim-inject/API →
      `≤ 0` internally) at this boundary, matching the convention at
      `assets/grid.rs:47-49` / `tasks/sim_tick/helpers.rs:200-201`.
- [ ] 3.3 Unit test in `assets/pv.rs`: with `weather_power_kw` or
      `irradiance` producing output beyond the ceiling, `step_inner`
      output is clamped to the ceiling (this already has a similar test
      at `pv.rs:371-378` for the field directly — add one exercising the
      new sign-converted entry point instead of setting the field
      directly).
- [ ] 3.4 Integration test in `simulator/mod.rs` or `simulator/tests.rs`:
      a full `tick()` call with a `pv_export_limit_kw` sim-inject value
      produces clamped PV output in the resulting snapshot, and clearing
      it restores natural output on the next tick.

## 4. Remove dead dispatcher clamp

- [ ] 4.1 Remove the PV export-limit clamp block in
      `controller/dispatcher.rs::build_setpoints` (the `if let
      Some(export_cap) = capacity.export_limit_kw { ... }` block, ~lines
      84-93) since PV physics never consumed the setpoint it produced.
- [ ] 4.2 Remove/update any dispatcher tests that exercised the now-removed
      block; confirm no other test depended on its (nonexistent) effect.
- [ ] 4.3 Run the full dispatcher test suite to confirm no regressions
      from the removal.

## 5. PV control schema (backend)

- [ ] 5.1 Add a third `ControlDescriptor` for `pv_export_limit_kw` to
      `PvInverter::control_schema()` (`assets/pv.rs`, alongside
      `pv_irradiance`/`pv_irradiance_alpha`), unit `kW`, min `0`, no fixed
      max (bounded by `rated_kw` at the UI layer).
- [ ] 5.2 Unit test: `control_schema()` includes the new descriptor with
      correct `ControlKind`/bounds.

## 6. VEN UI — control + display

- [ ] 6.1 Add `pv_export_limit_kw` to `PostSimInjectBody` type
      (`VEN/ui/src/api/types.ts`), alongside the existing
      `grid_export_limit_kw` optional field.
- [ ] 6.2 Wire a persistent-override control for `pv_export_limit_kw` in
      `AssetRightSection.tsx`, following the `heater_temp_min_c`/
      `heater_temp_max_c` pattern (not the decaying `DECAY_CONTROLS`
      pattern used for `pv_irradiance`).
- [ ] 6.3 Confirm (manual check, no code change expected) that
      `Dashboard.tsx`'s existing "Export limit" PV display
      (`Dashboard.tsx:326`) now shows real values once the backend sets
      `export_limit_kw` in PV `state_values()`.
- [ ] 6.4 UI unit test: setting/clearing the control calls the sim-inject
      API with the expected payload (mirror existing heater temp-limit
      control tests).

## 7. Verification

- [ ] 7.1 `wsl cargo check` / `wsl cargo test -p ven-app` locally (acquire
      `wsl_lock.sh` first per repo convention) — confirm all four Rust
      test-pyramid layers stay green.
- [ ] 7.2 `cargo fmt --check` and
      `cargo clippy --all-targets --all-features -- -D warnings`.
- [ ] 7.3 `cd VEN/ui && npm test` and `npm run build` (ESLint clean).
- [ ] 7.4 `scripts/audit_file_sizes.py` — confirm `assets/pv.rs` and
      `controller/dispatcher.rs` stay within the production-line budget
      after the additions/removal.
- [ ] 7.5 Manual verification: run the VEN UI locally/on Pi4, set a
      `pv_export_limit_kw` below current PV output via the new Controller
      control, confirm the Dashboard "Export limit" display updates and
      PV power in the live status drops to the ceiling within one tick.
- [ ] 7.6 Full suite on Pi4 (`bash run_all_tests.sh`, acquire
      `pi4_lock.sh` first) before merge, per repo testing guide.
