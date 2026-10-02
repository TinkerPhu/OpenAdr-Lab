# Tasks

Branch: `048-ev-soc-state-variables`. Resolves R-93 and R-92.

Test-first throughout. Run Rust locally via WSL with the lock held
(`bash scripts/wsl_lock.sh acquire -m "048: cargo check/test" -l 30`), `-j 2`, and
check free RAM first per the `memory-budget` rule. Prefer
`DOCKER_HOST=Node2 bash run_all_tests.sh ...` for suite runs.

## 1. The obligation type, shared by both layers

- [ ] 1.1 Write a failing test that one `EvObligation { deadline_step, target_soc, session_id }` type is what both `EvMilpContext` and `MilpInputs` carry (a compile-level test: construct a `MilpInputs` from an `EvMilpContext`'s obligations with no field-by-field translation), and confirm it fails
- [ ] 1.2 Add `EvObligation` and replace `EvMilpContext.t_dead_step`/`.e_required_kwh` with `obligations: Vec<EvObligation>` (`controller/milp_planner/asset_port.rs`); verify `cargo check -p ven-app` names every site that must change
- [ ] 1.3 Replace `MilpInputs.t_ev_dead_step`/`.e_ev_required_kwh` with the same `Vec<EvObligation>` (`milp_planner/types.rs`, filled at `inputs.rs:404-407`), so the scalar pair exists in neither layer (design Decision 7); verify 1.1 passes
- [ ] 1.4 Update `milp_planner/envelopes.rs:65` to read the obligation list instead of `e_ev_required_kwh`, and verify the existing envelope tests pass unchanged
- [ ] 1.5 Have `assets/ev_session_context.rs::from_state` and `assets/ev_usage_forecast.rs::target_next_predicted_departure` emit at most one obligation each — today's behaviour in the new shape — and verify every existing EV MILP test still passes

## 2. SoC variables and the balance constraint

- [ ] 2.1 Write failing tests in `assets/ev_milp.rs`: `declare_vars_pins_soc_at_the_live_reading` (index 0 fixed to `soc_init`), `soc_ev_has_n_plus_one_variables`, `soc_ev_is_bounded_by_the_floor_and_full`; confirm they fail
- [ ] 2.2 Add `soc_ev: Vec<Variable>` (length `n+1`) to `EvMilpVars` and declare it following `battery_milp.rs:30`'s `e_bat` construction (design Decision 1); verify 2.1 passes
- [ ] 2.3 Write a failing solver test that the projected SoC rises by exactly the charged energy over the pack size (`charging_raises_projected_soc_by_the_energy_charged`), then add the balance equality per design Decision 2; verify it passes
- [ ] 2.4 Write failing tests for the drop: `a_predicted_return_lowers_projected_soc_by_the_expected_amount` and `a_drop_larger_than_the_pack_settles_at_the_floor_not_infeasible`; then add the `drop_unmet` slack bounded by `drop_frac[t]` with its objective penalty (design Decision 3) and verify both pass
- [ ] 2.5 Write a failing test that a drop keeping SoC above the floor is applied in full (`drop_unmet` is zero), and verify it passes — this is the guard against the slack dodging a drop (design Risks)
- [ ] 2.6 Verify `MustNotRun` still emits zero-width power variables and that the SoC series is flat at `soc_init` across the horizon for an unplugged EV

## 3. Obligations as SoC bounds, with shortfall slack

- [ ] 3.1 Write failing tests: `a_firm_target_is_met_at_its_deadline_step`, `energy_charged_after_the_deadline_does_not_meet_the_target`, `an_obligation_beyond_the_horizon_constrains_nothing`; confirm they fail
- [ ] 3.2 Emit one `soc_ev[deadline_step] + shortfall_soc_k >= target_soc_k` constraint plus its penalised slack per obligation (design Decision 4), and verify 3.1 passes
- [ ] 3.3 Delete `EvMilpContext::reachable_energy_kwh` and the `min(e_required_kwh, reachable_energy_kwh)` pre-cap (`ev_milp.rs:170`); verify `solve_ev_must_run_required_energy_beyond_what_the_available_slots_can_deliver` (`milp_planner/tests/solver.rs:249`) still produces a plan rather than an infeasibility, updating it to assert the slack-reported shortfall
- [ ] 3.4 Write a failing test that a *reachable* target is never traded for slack (slack is exactly zero), then calibrate the shortfall penalty above the largest comfort bid and tariff spread (design Decision 4) and verify it passes
- [ ] 3.5 Write a failing test that the reported shortfall equals the kWh actually missed, then make `milp_planner/ev_diagnostics.rs::firm_shortfall` read `shortfall_soc_k * battery_kwh` and name the obligation's `session_id`; verify it passes
- [ ] 3.6 Make the whole-horizon band-accounting equality change (design Decision 5), delete `energy_expr`, and verify the full arm-by-arm EV test matrix in `assets/ev_milp.rs` and `milp_planner/tests/solver.rs` passes with unchanged totals — any arm whose numbers move is a finding to explain to the user, not to re-baseline

## 4. Readback, and deleting the reconstruction

- [ ] 4.1 Write a failing test that `EvSolOutput` carries the solved SoC series read from the variables (mirroring `battery_milp.rs:135`'s `e_bat` readback), and confirm it fails
- [ ] 4.2 Add `soc_ev: Vec<f64>` to `EvSolOutput`, read it from the solution, and verify 4.1 passes
- [ ] 4.3 Delete `asset_port::ev_soc_trajectory` and move every caller to the solved series; verify `cargo check -p ven-app` is clean and no integrator remains (this also discharges R-73's remaining EV half)
- [ ] 4.4 Rewrite the `ev_soc_trajectory` unit tests (`assets/ev.rs:887-955`) as solver-level tests asserting the same properties — monotonic rise under charging, the floor, drop timing at the return slot — and verify they pass (design Decision 6; do not delete them)
- [ ] 4.5 Verify the plan's EV SoC series keeps its existing JSON field name and shape so the UI needs no change, with a test asserting the serialised plan shape is unchanged

## 5. Multi-obligation capability (R-92 proper)

- [ ] 5.1 Write a failing solver test `two_targets_at_two_deadlines_are_each_met_at_their_own_step`, and confirm it fails
- [ ] 5.2 Verify it passes with no further production change — the obligation list from section 1 plus the SoC bounds from section 3 should already deliver it; if it does not, fix the model rather than the test
- [ ] 5.3 Write and verify `charge_is_carried_across_an_intervening_absence`: a later target met only by charging before the departure, to the extent the charge survives the trip (spec: "Charge is carried across an intervening absence")
- [ ] 5.4 Write and verify `a_recharge_after_a_predicted_return_is_planned` and `no_charging_is_scheduled_while_away` — the capability R-93 names as missing
- [ ] 5.5 Verify a single-obligation plan is identical to the pre-change plan for the same inputs (spec: "A single target behaves as before"), comparing solved power schedules

## 6. Verification and close-out

- [ ] 6.1 Run the full gate and verify green: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo audit`, `python scripts/audit_file_sizes.py`, the four VEN Rust pyramid layers, both UI suites
- [ ] 6.2 Verify the `ven-architecture` grep invariants are still empty (no `use crate::profile` in `entities/`/`controller/`/`routes/`, no `use crate::assets::` in `milp_planner` or `entities/`)
- [ ] 6.3 Measure EV phase-1 and phase-2 solve time against the R-97 battery+EV baseline and record the delta in `docs/history/project_journal.md` (design Risks: solve time); if the regression is material, report it to the user before merging rather than absorbing it
- [ ] 6.4 Run `DOCKER_HOST=Node2 bash run_all_tests.sh` and verify every suite passes (isolate the module if R-21's heap flake hits)
- [ ] 6.5 Verify in the running app that a plan's EV SoC curve still renders correctly on the VEN UI, and that a predicted trip shows the drop
- [ ] 6.6 Close R-93 and R-92 in `docs/reference/TECHNICAL_DEBTS.md`, and note R-73's remaining EV half as discharged by task 4.3
- [ ] 6.7 Wave the implemented behaviour into current-state docs per `workflow` rule 3: the EV MILP model into `docs/architecture/VEN_ARCHITECTURE.md`, post-return recharge into the relevant `docs/use-cases/*.md`, pointing at test files by name
- [ ] 6.8 Write the journal entry and add durable lessons to `docs/reference/KEY_LEARNINGS.md`
- [ ] 6.9 Delete `openspec/changes/ev-soc-state-variables/` once implemented and tested, and verify `ev-session-queue-foundation` still reads correctly as the next change
