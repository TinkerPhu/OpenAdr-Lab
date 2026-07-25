## 1. MILP decision variable

- [ ] 1.1 Add `p_pv_used: Vec<Variable>` to `GridMilpVars` (`VEN/src/controller/milp_interactions.rs`)
- [ ] 1.2 Declare `p_pv_used` and add the `p_pv_used[t] <= inputs.p_pv_kw[t]` constraint in
      `solver_phase1.rs`; replace `inputs.p_pv_kw[t]` with `p_pv_used[t]` in the balance equation
- [ ] 1.3 Mirror the same declaration/constraint/balance change in `solver_phase2.rs`
- [ ] 1.4 Unit tests: uncurtailed slot keeps full forecast; export-cap-forces-curtailment; no
      controllable assets present (per spec scenarios)

## 2. Plan reporting

- [ ] 2.1 Add `pv_used_kw: f64` to `PlanSlot` (`VEN/src/entities/plan.rs`)
- [ ] 2.2 Populate `pv_used_kw` from the solved `p_pv_used[t]` in `results.rs`'s success path
- [ ] 2.3 Set `pv_used_kw = pv_forecast_kw` in `results.rs`'s `fallback_plan` path
- [ ] 2.4 Unit tests: both fields present and `pv_used_kw <= pv_forecast_kw`; fallback sets them equal

## 3. Runtime wiring (fixes the pre-existing dead export_limit_kw path)

- [ ] 3.1 Add `pv_export_limit_override: Option<f64>` parameter to `SimState::tick()`
      (`VEN/src/simulator/mod.rs`); apply it to `PvInverter.export_limit_kw` in the existing
      `AssetConfig::Pv` match arm
- [ ] 3.2 Add a resolver (in `controller/dispatcher.rs` or a sibling helper) that computes the
      more-restrictive-magnitude of `capacity.export_limit_kw` and the current slot's
      `pv_used_kw`-derived cap (sign-converted to the asset's negative-export convention)
- [ ] 3.3 Wire the resolved value through `tasks/sim_tick/helpers.rs` and `tasks/sim_tick/tick.rs`
      into the `sim_guard.tick(...)` call
- [ ] 3.4 Unit/integration tests: VTN capacity limit alone curtails simulated output; plan-driven
      curtailment alone curtails simulated output; tighter-of-two-limits wins; no active limit
      leaves PV unclamped
- [ ] 3.5 Extend the existing UC2 (`EXPORT_CAPACITY_LIMIT`) BDD scenario (or add a physics-level
      scenario) asserting PV output is actually reduced, not just that the event is received

## 4. VEN UI

- [ ] 4.1 Add `pv_used_kw` to the `PlanSlot`/`Plan` TypeScript type (`VEN/ui/src/api/types.ts`)
- [ ] 4.2 Show the curtailed amount (or no indicator when `pv_used_kw == pv_forecast_kw`) on the
      plan-facing PV chart/panel

## 5. Documentation & backlog

- [ ] 5.1 Update `docs/plans/deviation-scenarios-analysis.md` §2/§7: mark PV-export decision
      variable task done, describe what shipped
- [ ] 5.2 Update `DOCUMENTATION.md` (asset/plan field tables) for `pv_used_kw`
- [ ] 5.3 Append `docs/history/project_journal.md` entry and `docs/reference/KEY_LEARNINGS.md`
      lessons (notably: the dead `export_limit_kw` wiring found during scoping)

## 6. Verification

- [ ] 6.1 `wsl cargo check` / `wsl cargo test -p ven-app` locally (wsl_lock)
- [ ] 6.2 `cargo fmt --check` and `cargo clippy --all-targets --all-features -- -D warnings`
- [ ] 6.3 `scripts/audit_file_sizes.py` — confirm no file exceeds its cap
- [ ] 6.4 Pi4 E2E + resilience suites green (pi4_lock) before merge
