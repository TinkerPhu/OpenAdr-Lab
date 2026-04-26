## 1. Profile and Plan struct changes

- [x] 1.1 Add `phase2_epsilon_eur: f64` to `PlannerConfig` in `VEN/src/profile.rs` with `serde(default)` and default function returning 0.02; add `default_phase2_epsilon()` alongside existing defaults
- [x] 1.2 Add `friction_eur: f64` field to `Plan` struct in `VEN/src/entities/plan.rs` (default 0.0); update `Plan::default()` and any constructors
- [x] 1.3 Verify `cargo build` compiles cleanly after struct changes

## 2. Split MilpWeights into Phase1Weights and Phase2Weights

- [x] 2.1 In `VEN/src/controller/milp_planner.rs`, rename `MilpWeights` to `Phase1Weights`; remove `c_bat_startup_eur`, `c_bat_ramp_eur_kw`, `c_ev_startup_eur`, `c_ev_ramp_eur_kw` from it; keep `c_bat_wear_eur_kwh` and `c_bat_ev_coexist_eur_kwh`
- [x] 2.2 Add new `Phase2Weights` struct with fields: `c_bat_startup_eur`, `c_bat_ramp_eur_kw`, `c_ev_startup_eur`, `c_ev_ramp_eur_kw`, `lambda_heat_sw_eur`, `w_tier_penalty_eur`
- [x] 2.3 Replace `build_milp_weights()` with `build_phase1_weights(profile, objective) -> Phase1Weights` and `build_phase2_weights(inputs) -> Phase2Weights`; `Phase2Weights` reads directly from `MilpInputs` fields (lambda_heat_sw_eur, w_tier_penalty_eur) and `PlannerConfig` penalty fields
- [x] 2.4 Verify all existing callers of `build_milp_weights` are updated; `cargo build` clean

## 3. Implement solve_phase1

- [x] 3.1 In `milp_planner.rs`, extract `solve_phase1(inputs: &MilpInputs, weights: &Phase1Weights) -> Result<Phase1Result, _>` as a private function; `Phase1Result` holds `phase1_cost_eur: f64` and the full `SolveOutput` (heater/battery/EV schedule for fallback)
- [x] 3.2 Phase 1 objective: grid energy terms + battery wear + GHG + grid penalty + import penalty + violation penalties + BatEvCoexist interaction; NO switching/startup/ramp/tier terms
- [x] 3.3 All existing constraints (power balance, asset constraints, interaction constraints) remain identical to current `solve_milp`
- [x] 3.4 `Phase1Result.phase1_cost_eur` = `solution.eval(&phase1_cost_expression)` where `phase1_cost_expression` is the Phase 1 objective expression (stored separately from the solver objective variable for reuse in Phase 2 constraint)
- [x] 3.5 Add unit tests for `solve_phase1`: heater-only inputs produce schedule without switching cost distortion; battery-only inputs include wear cost

## 4. Implement solve_phase2

- [x] 4.1 Add `solve_phase2(inputs: &MilpInputs, weights: &Phase2Weights, c_star: f64, epsilon: f64) -> Result<SolveOutput, _>` as a private function
- [x] 4.2 Rebuild full variable set and all constraints identically to Phase 1 (same power balance, asset constraints, interaction constraints)
- [x] 4.3 Add cost-cap constraint: `phase1_cost_expression <= c_star + epsilon` using the same expression structure as Phase 1
- [x] 4.4 Phase 2 objective: heater switching (`lambda_sw × sw[t]`), heater tier penalty (`w_tier × z_heat_full[t]`), battery startup/ramp (via `BatteryMilpContext::objective` with Phase2Weights), EV startup/ramp (via `EvMilpContext::objective` with Phase2Weights); NO energy/wear/GHG terms
- [x] 4.5 Add unit tests for `solve_phase2`: given Phase 1 cost, returns schedule with fewer switches than an unconstrained single-pass solve on the same inputs

## 5. Initial heater mode pinning

- [x] 5.1 Add `initial_z_mid: f64` and `initial_z_full: f64` fields to `HeaterMilpContext`
- [x] 5.2 In `HeaterMilpContext::from_state()`, read `AssetState::Heater(s).actual_power_kw` and set `initial_z_mid = 1.0` if `actual_power_kw ≈ p_mid_kw`, `initial_z_full = 1.0` if `actual_power_kw ≈ p_full_kw`, else both 0.0 (use tolerance of 0.1 kW)
- [x] 5.3 In `HeaterMilpContext::constraints()`, add `constraint!(v.z_heat_mid[0] == self.initial_z_mid)` and `constraint!(v.z_heat_full[0] == self.initial_z_full)` after C1 (initial tank energy pin)
- [x] 5.4 Update the fallback path in `build_milp_inputs()` (no live heater in sim) to set `initial_z_mid = 0.0`, `initial_z_full = 0.0`
- [x] 5.5 Add unit tests: heater-on initial state produces `z_heat_full[0] == 1`; heater-off produces both == 0

## 6. Wire two-phase wrapper into run_planner

- [x] 6.1 Add `solve_milp_two_phase(inputs: &MilpInputs, p1w: &Phase1Weights, p2w: &Phase2Weights, epsilon: f64) -> Result<(SolveOutput, f64, f64), _>` returning `(phase2_output, phase1_cost_eur, friction_eur)`; calls `solve_phase1` then `solve_phase2`; on Phase 2 error logs warning `"phase2 infeasible, falling back to phase1"` and returns Phase 1 output with `friction_eur = 0.0`
- [x] 6.2 In `run_planner()`, replace `solve_milp(inputs, weights)` call with `solve_milp_two_phase`; pass `profile.planner.phase2_epsilon_eur`; if `phase2_epsilon_eur == 0.0` skip Phase 2 entirely and use Phase 1 output
- [x] 6.3 Populate `plan.objective_eur = phase1_cost_eur` and `plan.friction_eur = friction_eur` in the output translator (`build_plan_from_solution`)
- [x] 6.4 Verify `cargo build --workspace` clean

## 7. Update plan adoption threshold comparison

- [x] 7.1 In `VEN/src/loops.rs` adoption gate (~line 856), confirm the comparison already uses `plan.objective_eur`; no change needed since `objective_eur` now holds Phase 1 cost exclusively — verify and add a comment
- [x] 7.2 Update `PlannerEvent::PlanReady` emission to log both `objective_eur` (Phase 1) and `friction_eur` (Phase 2) for observability
- [x] 7.3 Add `plan_adoption_decay_s: f64` to `PlannerConfig` in `VEN/src/profile.rs` (default `0.0`); add `#[serde(default)]` and update `Default` impl; add doc comment referencing Decision 8
- [x] 7.4 In `VEN/src/loops.rs` adoption gate, replace raw `threshold` with `effective_threshold`: compute `elapsed_s = (now - current.created_at).num_seconds().max(0) as f64`; if `decay_s > 0.0` apply `decay_factor = (1.0 - elapsed_s / decay_s).max(0.0)`, else `decay_factor = 1.0`; log `effective_threshold_eur` field in the rejection log line

## 8. Update existing unit tests

- [x] 8.1 In `milp_planner.rs` test module, update all tests that previously called `solve_milp` to call `solve_milp_two_phase` or the appropriate phase function
- [x] 8.2 Update any tests that assert on `objective_eur` values — expected values will change since they now reflect Phase 1 cost only
- [x] 8.3 Update `HeaterMilpContext` tests in `heater.rs` to cover the new `initial_z_mid`/`initial_z_full` constraint behaviour
- [x] 8.4 Run `cargo test --workspace` locally and confirm all unit tests pass

## 9. BDD validation on Pi4

- [x] 9.1 SCP updated VEN source files to Pi4-Server for a smoke-test build (`scp -r VEN/src Pi4-Server:/srv/docker/openadr_lab/VEN/`)
- [x] 9.2 Run cargo unit tests on Pi4: 260 passed, 0 failed — `docker build -f VEN/Dockerfile --target builder` + `cargo test --workspace --release`
- [ ] 9.3 Run full BDD suite on Pi4: `docker compose -f tests/docker-compose.test.yml run --build --rm test-runner` [requires `--build`]
- [ ] 9.4 Confirm all existing scenarios pass; investigate any failures before proceeding

## 10. Deploy and observe

- [x] 10.1 Commit all changes with message `feat(planner): two-phase lexicographic MILP — Phase 1 cost, Phase 2 friction`
- [x] 10.2 Push and deploy to Pi4: commits pushed at `fe5147a`; VEN stack rebuild pending
- [ ] 10.3 Monitor planner logs for `"phase2 infeasible"` warnings; confirm none appear in normal operation
- [ ] 10.4 Observe VEN-2 planner: confirm `objective_eur` stabilises (no more 6→7 oscillation) and heater schedule no longer jumps between PV windows
- [ ] 10.5 Update `docs/history/project_journal.md` with implementation summary and key learnings
