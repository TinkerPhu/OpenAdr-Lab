## 1. Dual LP solve

- [x] 1.1 `add_model_constraints` (`solver_phase1.rs`): switch the per-slot power-balance
      constraint to `model.add_constraint(...)`, returning `(S, Vec<ConstraintReference>)`;
      update the two existing callers (`solve_phase1`, `solve_phase2`) to ignore the new return
      value.
- [x] 1.2 New `solver_duals.rs`: `solve_marginal_costs(inputs, p1w, asset_contexts, winning:
      &SolveOutput, timeout_s) -> Result<Vec<f64>, Box<dyn Error>>`. Landed differently than
      originally planned: fixing a binary via an *extra equality constraint on top of a
      `variable().binary()` declaration* does not work — HiGHS never populates row/column duals
      for a model with any integer-flagged column, pinned or not (confirmed empirically). The
      working approach declares each mode variable directly as continuous with `min == max ==
      winning_value` (bypassing `AssetMilpContext::declare_vars_into_pool`, which always
      hardcodes `.binary()`, and instead re-declaring each asset's variables from `MilpInputs`'
      own scalar fields), then calls the same `constraints()`/`objective()` trait methods against
      that locally-built pool — `good_lp::Variable` is just an opaque id, so those methods don't
      care how the variable was declared.
- [x] 1.3 Unit test `marginal_cost_matches_tariff_when_nothing_binding`: nothing binding — assert
      marginal cost ≈ import tariff (§5.2 worked example row 1).
- [x] 1.4 Unit test `marginal_cost_reflects_binding_import_violation_penalty` (redesigned from the
      original battery-power-bound framing): a battery sitting at its own power bound does *not*
      by itself move the balance row's dual (KKT stationarity only pulls in a constraint's dual
      when the balance row's own variable, `p_imp`, participates in that constraint). Used instead
      a directly-binding import-violation scenario (base load over the contractual cap, non-zero
      `pen_imp_eur_kwh`) — assert marginal cost ≈ tariff + violation penalty, exactly matching the
      hand-derived KKT expectation.
- [x] 1.5 **Not implemented as a forced-failure unit test.** The dual LP re-solves an
      already-known-feasible point (same constraints as the winning solve, mode variables pinned
      to values that solve already satisfied), so it has no natural failure mode to trigger
      deliberately — mirrors the existing precedent of not unit-testing Phase 2's
      failure-falls-back-to-Phase-1 branch either. The fallback code path (§2.1) is still present
      and reviewed by inspection.

## 2. Wiring into Plan

- [x] 2.1 `solve_milp_two_phase` (`solver_phase2.rs`): calls `solve_marginal_costs` with the
      winning solution after Phase 1/2 completes; on `Err`, logs a warning and falls back to
      `inputs.c_imp_eur_kwh.clone()`.
- [x] 2.2 `entities/plan.rs`: added `marginal_cost_import_eur_per_kwh` / `_export_` fields to
      `PlanTimeSlot`, `#[serde(default)]`.
- [x] 2.3 `results.rs::translate_to_plan`: accepts the marginal-cost `Vec<f64>` and sets both new
      fields per slot (same value, per §5.2's documented simplification).
- [x] 2.4 `results.rs::fallback_plan`: sets both new fields to 0.0 (infeasible plan — no
      meaningful shadow price).
- [x] 2.5 Mechanically updated the other ~8 `PlanTimeSlot { .. }` literal construction sites
      (timeline.rs ×2, dispatcher.rs ×2, forecast.rs, routes/timeline.rs ×2, reporter.rs) with the
      two new fields (0.0 — none of those paths run the solver).

## 3. VEN UI surface (ui-transparency)

- [x] 3.1 `VEN/ui/src/api/types.ts`: added both fields to the `PlanTimeSlot` type.
- [x] 3.2 `PlanDecisionMatrix.tsx`: added a "Marginal €" heatmap row below the Tariff row, reusing
      `tariffColor` on its own min/max scale; tooltip shows both the marginal cost and the plain
      tariff for comparison.
- [x] 3.3 Component tests: `matrix-marginal-header` / `marginal-cell-N` render; graceful fallback
      to the plain tariff when the field is absent (pre-change persisted plans).

## 4. Verification and bookkeeping

- [x] 4.1 Full VEN Rust test pyramid — 831/831 + 1 architecture test, `wsl cargo test -j 2 -p
      ven-app`, under `wsl_lock`.
- [x] 4.2 `cargo fmt --check` clean; `cargo clippy --all-targets --all-features -- -D warnings`
      clean (one `#[allow(clippy::type_complexity)]` on `solve_milp_two_phase`'s 4-tuple return,
      justified inline).
- [x] 4.3 `scripts/audit_file_sizes.py` — PASSED.
- [x] 4.4 VEN UI test suite — 417/417 (39 files); `tsc --noEmit` clean; ESLint clean.
- [x] 4.5 Recorded in `docs/history/project_journal.md`; noted the HiGHS-duals-require-continuous-
      columns lesson in `docs/reference/KEY_LEARNINGS.md`.
