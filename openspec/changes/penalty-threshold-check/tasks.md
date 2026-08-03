## 1. Entity & config types

- [x] 1.1 Add `PenaltyRuleParams { rule_id, threshold_kw, measurement_window_s, penalty_eur_per_kw }` to `entities/planner_params.rs`
- [x] 1.2 Add `penalty_rules: Vec<PenaltyRuleParams>` to `PlannerParams` (`entities/planner_params.rs`), default `vec![]` in its `Default` impl
- [x] 1.3 Add `c_peak_penalty_eur: f64` to `CostBreakdown` (`entities/plan.rs`)
- [x] 1.4 Add `penalty_rules_active: Vec<{rule_id, threshold_kw}>` (or equivalent) to `Plan` for UI consumption

## 2. Profile config + validation

- [x] 2.1 Add `#[serde(default)] pub penalty_rules: Vec<PenaltyRuleParams>` to the planner section of `profile/schema.rs`, reusing the entity type directly (same pattern as `PlanZone`)
- [x] 2.2 Write unit tests (red first): `measurement_window_s` not a multiple of `plan_step_s`, non-positive `threshold_kw`, duplicate `rule_id` → each rejects (DEVIATION: via the existing `Profile::validate() -> Result<(), Vec<String>>` mechanism used by every other profile invariant, e.g. `plan_zones`, not `DomainError::ProfileInvalid` — that variant is reserved specifically for a not-yet-built hot-reload path per its doc comment in `error.rs`; startup validation already has its own established, tested mechanism and duplicating it would be wrong. See design.md correction note.)
- [x] 2.3 Implement profile-load validation until 2.2 is green
- [x] 2.4 Wire `profile.planner.penalty_rules` into `PlannerParams` construction in `main.rs`

## 3. Fix ad-hoc `PlannerParams` constructors

- [x] 3.1 Add the new field (or switch to `..Default::default()`) at each non-spreading `PlannerParams { ... }` site: `services/planning.rs` (4 sites, ~1097/1150/1194/1238), `controller/milp_planner/tests/solver.rs:24` (checked: all 4 `services/planning.rs` sites already use `..PlannerParams::default()` spread — no change needed there; only `tests/solver.rs`'s manual `MilpInputs` literal needed the new `penalty_rules: vec![]` field, added)
- [x] 3.2 `cargo build`/`cargo test`/`cargo clippy`/`cargo fmt` all clean (found 14 more `Plan { .. }`/`PlannerConfig::default()` literal sites outside the planner module needing the two new fields — fixed all; see compile-check note below)

## 4. MILP formulation

- [x] 4.1 Write unit test (red first) `window_index_buckets_correctly`: 24h horizon, 5-min steps, `measurement_window_s=1800` → 48 slots map to 8 windows of 6 (`penalty.rs::tests::num_windows_buckets_correctly` + `window_index_*`)
- [x] 4.2 Create `controller/milp_planner/penalty.rs` with `fn window_index(...)`; implement until 4.1 is green
- [x] 4.3 Write unit test (red first) `add_penalty_constraints_splits_load_below_threshold`: 10 kW threshold, 12 kW flexible demand shiftable across two slots → no slot's `net_import_kw` exceeds 10 kW, all energy still delivered (`tests/penalty.rs`)
- [x] 4.4 Implement `penalty_constraints` in `penalty.rs` (DEVIATION: one shared per-window slack `s_penalty[rule][window] >= 0` directly bounding `p_imp[t] <= threshold_kw + s_penalty[window_of(t)]` for every `t` in that window — no separate `p_peak` variable. Mathematically equivalent to the design doc's two-variable sketch since minimizing the objective always pushes `s_penalty` to the window's true peak overage; one fewer variable per window) until 4.3 is green
- [x] 4.5 Add `penalty_rules: Vec<PenaltyRuleParams>` to `MilpInputs` (`controller/milp_planner/types.rs`) and populate it in `inputs.rs:409`
- [x] 4.6 Call `penalty_constraints`/`penalty_objective` from `solver_phase1.rs`; objective term added alongside the existing `s_imp_viol`/`s_exp_viol` terms (once per window, not per slot — see design.md D3)
- [x] 4.7 Write unit test (red first) `penalty_rule_disabled_by_default_adds_no_slack_and_matches_unmodified_plan`: empty `penalty_rules` → `s_penalty_kw` empty (`tests/penalty.rs`)
- [x] 4.8 Confirm 4.7 green and full existing `controller/milp_planner/tests/*` suite still passes unmodified (120/120 milp_planner tests pass; full crate: 897/897 tests pass, `cargo fmt --check` clean, `clippy --all-targets --all-features -- -D warnings` clean, `scripts/audit_file_sizes.py` clean)
- [x] 4.9 Thread `penalty_rules` through `solver_phase2.rs` (added to `phase1_cap_expr`, not `friction_obj` — it's an economic cost) and `solver_duals.rs` (added to its objective, mirroring phase1; `add_model_constraints`'s shared constraint-adding function now takes `penalty_vars` as one new parameter, threaded through all 3 call sites)
- [x] 4.10 Write unit test (red first) `add_penalty_constraints_accepts_penalty_when_reallocation_impossible`: demand with a hard single-slot deadline and no alternative slot → `s_penalty > 0`, `CostBreakdown.c_peak_penalty_eur > 0` (`tests/penalty.rs`)
- [x] 4.11 Implement the results-side cost summation in `results.rs` (`translate_to_plan`) until 4.10 is green

## 5. Plan warnings

- [x] 5.1 Write unit test (red first): a window with `s_penalty > 0` produces a `PlanWarning` naming the rule, window, peak, threshold, accepted cost (`tests/penalty.rs::translate_to_plan_emits_warning_and_cost_when_penalty_accepted`)
- [x] 5.2 Implement warning emission in `results.rs` until 5.1 is green
- [x] 5.3 Populate `Plan.penalty_rules_active` from the plan's configured rules in `results.rs` (also populated in `fallback_plan`, for UI consistency even on solver failure)

## 6. VEN UI

- [x] 6.1 Add `penalty_rules_active` to `Plan` in `api/types.ts` (SCOPE TRIM: `CostBreakdown`/`cost_breakdown` is not exposed to the UI at all today — grepped, zero references anywhere in `VEN/ui/src` — so adding just `c_peak_penalty_eur` to a type nothing reads would be dead code; the "penalty accepted" cost is already visible via the `PlanWarning` text, satisfying the spec's UI-visibility requirement without it)
- [x] 6.2 Add "Peak demand" row to `PlanDecisionMatrix.tsx`, rendered only when `plan.penalty_rules_active` is non-empty, colored per-slot against the tightest active threshold, tooltip naming all active rules; legend updated
- [x] 6.3 Add/extend a UI unit test asserting the row is absent when no rules are active and present with correct over/under coloring when they are (`PlanDecisionMatrix.test.tsx`) — 18/18 pass, full UI suite 437/437 pass, eslint clean, `npm run build` clean
- [x] 6.4 Manually verify `PlanHeaderBar`'s existing warnings list renders the new penalty `PlanWarning` correctly (no code change expected — confirm only). Confirmed by code inspection: `PlanHeaderBar.tsx:148-173` renders every `plan.warnings[]` entry generically (severity chip + message + suggested_action) with no allow-list or special-casing by message content, so the new `results.rs`-emitted penalty warning renders through this path unmodified; full end-to-end confirmation happens in the BDD scenario (group 7) and the manual walkthrough (task 9.4)

## 7. BDD

- [x] 7.1 Create fixture profile `VEN/profiles/penalty_test.yaml` (DEVIATION: `measurement_window_s: 3600`/no `plan_step_s` override, not `300` — `test.yaml`'s `planner.plan_step_s` is irrelevant because it sets `plan_zones` with `step_s: 3600`, which overrides it per `effective_step_s()`; also found and fixed a real bug this surfaced — `profile/validate.rs`'s window-multiple check was validating against the ignored raw `plan_step_s` instead of `effective_step_s()`, which would have silently misvalidated any profile using `plan_zones`, i.e. every real fleet profile)
- [x] 7.2 Add step def `Then no plan slot's net_import_kw exceeds "{kw}" within the horizon` to `tests/features/steps/planner_steps.py`
- [x] 7.3 Add the new scenario to `tests/features/ven_planner.feature`. Also required standing up a **new 5th VEN test container** (`test-ven-penalty` in `tests/docker-compose.test.yml`, mirroring the existing `test-ven-no-pv` pattern) since BDD "profile" scenarios route to distinct pre-built, already-running containers rather than hot-swapping YAML — wired `VEN_PENALTY_TEST_BASE_URL` through `api_client.py`, `entity_model_steps.py`'s `profile_urls` map, and `test-runner`'s `depends_on`. This is real infra addition beyond the original tasks.md wording, not scope creep — no cheaper way to run a profile-scoped BDD scenario exists in this repo today.
- [x] 7.4 Run the e2e suite on Node2 until green. DEVIATION: ran the raw `docker compose -f tests/docker-compose.test.yml run --build --rm test-runner` commands directly (detached via `nohup ... & disown` on Node2, not `run_all_tests.sh`'s own invocation) after discovering Node2's `/srv/docker/openadr_lab` is a separate git clone that `run_all_tests.sh --e2e` updates via `git pull` on whatever branch is currently checked out there (`main` by default) — it does **not** see local uncommitted changes or automatically switch to a feature branch. First attempt silently tested stale `main` for 26 minutes before this was caught (`test-ven-penalty` container simply never existed). Committed+pushed to `043-penalty-threshold-check`, manually `git fetch && checkout && pull`'d that branch on Node2, then re-ran. RESULT: **54 features passed, 270 scenarios passed, 1535 steps passed, 0 failed** (1 whole feature skipped — pre-existing tag-gated resilience feature, expected in a non-`@resilience` run), including the new `Planner reschedules load to stay under a peak-demand penalty threshold` scenario (`Status.passed`, 70.6s) against the new `test-ven-penalty` container. `EXIT_CODE=0`.

## 8. Docs & backlog bookkeeping

- [x] 8.1 Remove the BL-09 entry from `docs/BACKLOG.md` (both the short table row/entry and a second, more detailed "Implementation Task List" section further down the same file that also referenced BL-09; also corrected BL-35's and `docs/plans/strategic_roadmap.md`'s false claim that BL-09 would unblock BL-35 — it doesn't, BL-35 needs separate Stage-5 tier/SIMPLE-fallback machinery this change never built)
- [x] 8.2 Add a short penalty-threshold subsection to `docs/architecture/VEN_ARCHITECTURE.md` (new §2.3.2)
- [x] 8.3 Mark WP6.3 done in `docs/plans/roadmap/phase-6-fidelity-and-cert.md` — removed the WP6.3 subsection (WP6.1/6.2/6.4 and Tracks B/C are still open, so the file stays; per the "remove just the done part" workflow rule)
- [x] 8.4 Add an entry to `docs/history/project_journal.md`
- [ ] 8.5 Run `/wiki-sync` — deferred until after 7.4 (e2e) confirms green, so the wiki reflects verified behavior

## 9. Final verification

- [x] 9.1 `cargo fmt --check` and `cargo clippy --all-targets --all-features -- -D warnings` clean (confirmed during 3.2/4.8; one clippy lint fixed — `needless_range_loop` in `penalty.rs`'s constraint-building loop)
- [x] 9.2 `scripts/audit_file_sizes.py` clean (confirmed during 4.8 — `penalty.rs` split kept `solver_phase1.rs` under the ceiling)
- [x] 9.3 All four test suites green (UI unit, Rust unit+integration, E2E BDD, resilience). Rust: 897/897. UI: 437/437. E2E: 54 features/270 scenarios/1535 steps passed, 0 failed. Resilience (`--tags=@resilience`, Node2): 5/5 scenarios passed, 0 failed. All on Node2 against `043-penalty-threshold-check` (commit `6ea458e`).
- [x] 9.4 Manual walkthrough on Node2 (`test-ven-penalty` brought up standalone under the lock, then torn down). DEVIATION: verified via direct API calls (`curl` against the VEN's own endpoints), not a browser — no browser tooling available in this environment. Injected `ev_soc=0.5`, POSTed an EV session (`target_soc=0.90`, departure in 12h) → triggered a `USER_REQUEST` replan. Confirmed: `plan.penalty_rules_active == [{"rule_id":"peak-10kw","threshold_kw":10.0}]` (exactly what the new Decision Matrix row reads to render), every slot's `net_import_kw` ≤ 5.92 kW (well under the 10 kW threshold, 0 slots over), and `cost_breakdown.c_peak_penalty_eur` showed a ~0.02 numerical-tolerance residual (solver MIP-gap noise, ~0.004 kW slack) that correctly did **not** produce a `PlanWarning` — confirms the `slack_kw > 0.01` filter in `results.rs` suppresses solver noise rather than surfacing spurious warnings for it.
