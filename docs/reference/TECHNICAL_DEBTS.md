# Technical Debts Register

> **Next ID: R-75.** Use this number for the next new item filed, then increment this
> line to R-75. When resolving and removing an item — even the current highest ID —
> do NOT decrement this line: it tracks every ID ever issued, not the count of rows
> currently in the table, so a removed row never frees its number for reuse. This is
> the single source of truth for the next ID; do not derive it by scanning for the
> highest `R-NN` currently present, since a removed top item would make that scan
> under-count and hand out a number that was already used once (see R-66/R-68 below,
> caused by exactly that kind of drift before this line existed).
>
> Verified against code 2026-07-16. Detailed diagnostics for large refactors:
> `docs/plans/refactoring_backlog.md`.
>
> **Rule:** Before adding a feature in an affected area, check this file first.
> Refactor the relevant debt before adding new behaviour if effort is Small or Trivial.
>
> IDs are stable and never reused; gaps in the numbering are resolved items
> (resolutions live in `docs/history/project_journal.md` and git history).
>
> Gain is the value of fixing the item — independent of Effort/Risk — rated
> High/Medium-High/Medium/Low-Medium/Low/None. Unlike `docs/BACKLOG.md`'s Gain
> (new capability delivered), most debt items don't change behavior at all, so
> Gain here usually reflects reduced risk, friction, or maintenance cost instead.

Priority legend: 🔴 High / 🟠 Medium-High / 🟡 Medium / 🔵 Low (deferred)

---

## Priority queue (🟠 / 🟡) — work these first, top down

| ID | Description | Affected files | Effort | Risk | Priority | Gain |
|----|-------------|----------------|--------|------|----------|------|
| R-69 | Battery round-trip efficiency modeled inconsistently: the simulator (`assets/battery.rs::step_inner`/`forecast`) puts all loss on the charge leg only; the MILP planner (`assets/battery_milp.rs`, `eff_ch=eff_dis=sqrt(rte)`) splits it symmetrically. Both agree on full-cycle totals but diverge on intermediate SoC for any partial cycle — the normal case under the 5-min rolling replan. Found during 2026-09-03 architectural audit. Tracked for resolution in `openspec/changes/battery-efficiency-model-reconciliation/` (deleted once implemented — see that change's design.md for the two candidate resolutions). | `VEN/src/assets/battery.rs`, `VEN/src/assets/battery_milp.rs` | Small | Medium (silent SoC drift under partial cycles, no visible symptom until it biases a SoC-gated decision) | 🟠 | Medium-High — real correctness risk, currently invisible to tests |
| R-72 | `capacity_forecast.rs`'s top-level `cap_kw` clamps its achievable-power report to `snapshot.grid.import_limit_kw`/`export_limit_kw` — a VTN-imposed, revocable directive (per `GridSnapshot`'s own doc comment: "maximum site import/export power currently allowed by an active VTN capacity event... NOT a fixed hardware/interconnection rating"), not the genuine physical/interconnection rating. Every `IMPORT_CAPACITY_LIMIT`/`EXPORT_CAPACITY_LIMIT` event therefore ceilings the number reported back to the VTN as the VEN's own achievable power — the VTN reading its own restriction back as newly-observed VEN capability, with no way to tell "nothing left" from "more, hidden behind my own directive." Found via a design.md analysis of `capacity_forecast.rs`/`envelope_forecast.rs` (originally in `openspec/changes/capacity-envelope-unification/`, abandoned as a container in favor of `docs/plans/asset-max-power-forecast-master-plan.md`, but this specific finding is real and independent of that unification question). Fix: clamp against the genuine physical/interconnection rating (`phys_imp`/`phys_exp`, the fallback `milp_planner/inputs.rs` already uses when no VTN limit event is active) instead — requires threading that rating into `SimSnapshot`/`GridSnapshot` or as a new parameter to `compute_capacity_curve`. | `VEN/src/controller/capacity_forecast.rs`, `VEN/src/controller/simulator_port.rs` (`GridSnapshot`) | Small | Medium (a VTN cannot currently distinguish "VEN genuinely has nothing left" from "VEN has more, hidden behind the VTN's own active limit") | 🟠 | Medium-High — real correctness/protocol-semantics risk in a report already sent to the VTN today |
| R-21 | `cargo test` intermittently crashes with heap corruption (SIGABRT, varying malloc messages) around the two heaviest HiGHS tests (`run_planner_n48_full_horizon`, `solve_ven3_heater_three_tier_zones_feasible`). Same tests pass clean in isolation every time; also crashes with `--test-threads=1`, so it is allocator/heap-state-dependent in the native HiGHS library, not a plain data race. Test-infra only — no production path. Workaround: run the affected module in isolation when the full suite crashes. | `VEN/src/controller/milp_planner/` (HiGHS FFI via `good_lp`), test harness only | Medium | Low (flake) | 🟡 | Medium — CI/test-suite trust, no production impact |
| R-33 | UI test gaps: `VTN/ui/src/pages/Metrics.tsx` is the only untested page in either UI; `JsonDialog.tsx` is byte-identical in both UIs (50 lines — accept the copy with a twin-note header, or fold into a shared package if one materializes). | `VTN/ui/src/pages/Metrics.tsx`, `*/ui/src/components/JsonDialog.tsx` | Small | Low | 🟡 | Low — test-coverage gap |

## Low priority (🔵) — by topic

### Architecture & type placement

| ID | Description | Affected files | Effort | Risk | Gain |
|----|-------------|----------------|--------|------|------|
| R-39 | `state/mod.rs` mixes app wiring (`AppState`) with domain-ish value types (`EvSettings`, `HemsState`). Decide whether the two value types move to entities/ (as `AssetLedgerEntry` did) or stay — record the conclusion either way. | `VEN/src/state/mod.rs` | Trivial | Mechanical | Low — architecture clarity, no behavior change |
| R-47 | `AppState` keeps accumulating flat diagnostic fields (VTN connection status, storage-ok flag, per-task status map, etc.) added ad hoc per WP (T1/T3). No grouping/namespacing, so it will keep growing linearly with every future observability WP. Consider a `diagnostics: DiagnosticsState` sub-struct. Found during the WP-T1/T3/T5/T7 combined code review (2026-07-18). | `VEN/src/state/mod.rs` | Small | Low | Low-Medium — prevents compounding maintenance debt on every future observability WP |

### Code & repo hygiene

| ID | Description | Affected files | Effort | Risk | Gain |
|----|-------------|----------------|--------|------|------|
| R-27 | Hard-coded tuning constants: task intervals (`state_persist.rs:8` 15 s, `progress_ticker.rs:15` 1 s). Name them and/or expose via config/PlannerParams. **The MILP-tolerance half is discharged (2026-08-28)**: `with_mip_gap` at all three solve sites now reads the per-profile `planner.mip_gap_target` (default 0.02) carried on `MilpInputs`, and the `MIP_GAP_TARGET` constant is gone. It was briefly configurable in August and reverted (5b8923c3) for line-count reasons rather than on its merits; restored here once `PlannerConfig` moved to its own module and the quality cost was actually measured (GB-40). | tasks/ | Trivial | Low | Low — config flexibility only |
| R-36 | Lint/doc hygiene bundle: (a) module-wide `#![allow(dead_code)]` without justification in `entities/capacity.rs:5`, `entities/design_vocabulary.rs:7`; (b) 12 eslint warnings (exhaustive-deps, mixed exports); (c) ~~eslint lints the generated `VTN/ui/coverage/` dir~~ — fixed 2026-08-05: turned out worse than lint noise, the whole generated dir (27 files, 423K) was actually committed to git because `VTN/ui/.gitignore` was missing the `coverage/` line `VEN/ui/.gitignore` already had; untracked and added; (d) `solve_ven3_heater_three_tier_zones_feasible` runs >60 s in debug `cargo test` — consider a smaller horizon variant; (e) "Stage 5 —" phase labels in `entities/user_request.rs` / `controller/user_request.rs` doc comments — drop the prefixes. | entities/, VEN/ui, VTN/ui, milp_planner/tests | Small | Low | Low — mostly cosmetic; (d) has a small developer-friction upside (faster test runs) |
| R-71 | `#[allow(...)]` without a same-line justification (this repo's own linting rule requires one), found in the assets/simulator area: `assets/mod.rs:350`, `assets/ev_milp.rs:226`, `assets/heater.rs:138`, `simulator/mod.rs:181`, `simulator/plan_context.rs:59`, `simulator/tests.rs:272`, plus `assets/asset_trait.rs:18,32,121` (justified only by a paragraph above the attribute, not on its line). The pattern works correctly right next door in `assets/heater_milp.rs:172,229` and `controller/simulator_port.rs:118,131`, which do have same-line justifications — so the convention is known, just inconsistently applied. A repo-wide grep for just `too_many_arguments` (excluding tests) finds 32 instances total, so this is a fraction of a wider pattern. Found during 2026-09-03 architectural audit. Two of the unjustified sites (`build_milp_context`'s 14 params, `tick`'s 19 params) are candidates for either a same-line justification or a params-struct refactor. | `VEN/src/assets/`, `VEN/src/simulator/` (listed above) | Trivial | Low | None — pure hygiene, mechanical |
| R-38 | (a) `VEN/Cargo.toml` carries blueprint-era comments (commented-out `openleadr-client` etc.); (b) verify `VTN/data/db` (runtime artifact) is gitignored. | `VEN/Cargo.toml`, `VTN/data/` | Trivial | Low | None — pure hygiene |
| R-44 | `/health` handler (`routes/system.rs::health`) deep-clones the full `VtnConnectionStatus` and active `Plan` on every poll just to read a couple of fields. Cheap today but grows with `Plan` size; consider a narrower state accessor. Found during the WP-T1/T3/T5/T7 combined code review (2026-07-18). | `VEN/src/routes/system.rs` | Trivial | Low | Low — cheap today, future-proofing only |
| R-73 | Pre-existing dead code, unrelated to the Spec A `AssetConfig`-to-trait-object refactor that found it: `Battery::future_state_values`, `EvCharger::soc_trajectory`/`future_state_values_at`, `Heater::future_state_values` are never called — `controller/milp_planner/asset_port.rs` has separate, actually-used "Mirrors X" reimplementations (`battery_future_state`, `ev_soc_trajectory`, `ev_future_state_at`, `heater_future_state`). Confirmed pre-existing (not introduced by this refactor) via `git stash` + grep showing only self-test and doc-comment references before the refactor's own changes. Currently `#[allow(dead_code)]`'d with a same-line note pointing here. Fix: either delete the four dead methods, or delete `asset_port.rs`'s duplicates and make callers use the trait methods directly (the two implementations may have already drifted — diff them before choosing). | `VEN/src/assets/battery.rs`, `VEN/src/assets/ev.rs`, `VEN/src/assets/heater.rs`, `VEN/src/controller/milp_planner/asset_port.rs` | Small | Low | Low-Medium — dead code plus a possible silent duplication/drift risk between the two implementations |
| R-74 | Found while adding `shiftable_load` as a new asset type (`shiftable-load-as-asset`): `VEN/ui/src/pages/Dashboard.tsx`'s "Simulation" card dispatches per asset by hardcoded presence checks (`"ev" in sim.data.assets`, `"heater" in ...`, `"pv" in ...`) with no case — and no generic fallback — for `battery`, `base_load`, or the new `shiftable_load`. Pre-existing gap (Battery/BaseLoad were already invisible there before this change), not introduced by it; not fixed as part of that change since it's a UI-layer refactor unrelated to the backend asset-dispatch work. Contrast with `Controller.tsx`/`AssetSpecsTable.tsx`'s `deriveAssetSummaries`, which already has a generic `HARDCODED_IDS`-exclusion fallback loop covering any asset_id it doesn't special-case. Fix: give `Dashboard.tsx`'s Simulation card the same generic fallback (or iterate `sim.data.assets` by `asset_type` rather than a fixed id list). | `VEN/ui/src/pages/Dashboard.tsx` | Small | Low | Low — one dashboard card under-displays some asset kinds; the same data is already visible via Controller/Devices | 
| R-66 | `run_all_tests.sh`'s GB-24 pre-flight capacity check (`MIN_AVAILABLE_MEM_MB=800`) is a first-pass heuristic from one live `ssh Node2 "free -m"` observation (2026-08-14: 3794 MB total, 2482–2919 MB available with the resident fleet running), not empirically calibrated against an actual degraded run's memory profile. Same class as R-27 (hard-coded tuning constants). May need tuning if it proves too strict (blocks a run that would've been fine) or too loose (still lets a degraded run through). | `run_all_tests.sh` | Trivial | Low | Low — config-flexibility/accuracy concern only, not a functional defect |

### UI performance

| ID | Description | Affected files | Effort | Risk | Gain |
|----|-------------|----------------|--------|------|------|
| R-48 | `useAssetCapabilities`/`useAssetForecasts` (WP-T6) fire one HTTP request per asset in parallel rather than a single batched endpoint; fine at lab scale (few assets) but won't scale. Found during the WP-T1/T3/T5/T7 combined code review (2026-07-18). | `VEN/ui/src/api/hooks.ts` | Small | Low | Low — fine at current (lab) scale, future-proofing only |
| R-49 | `Reports.tsx::latestSubmissionFor` recomputes its scan over all submissions on every render (not memoized) — fine at current volumes, revisit if submission history grows large. Found during the WP-T1/T3/T5/T7 combined code review (2026-07-18). | `VEN/ui/src/pages/Reports.tsx` | Trivial | Low | Low — fine at current volumes, future-proofing only |

### Weather forecast plugin (docs/architecture/weather_forecast.md)

| ID | Description | Affected files | Effort | Risk | Gain |
|----|-------------|----------------|--------|------|------|
| R-53 | Horizon/shading obstructions, the Perez/HDKR diffuse-sky model (vs. the current isotropic-on-zenith simplification), and module degradation over time are known, deliberately deferred accuracy gaps in `entities::solar`'s clear-sky transposition — see `docs/architecture/weather_forecast.md`. | `VEN/src/entities/solar.rs` | Medium | Low | Low-Medium — PV forecast accuracy improvement, deliberately deferred until it's the dominant error source |
| R-54 | The Mosquitto broker in this project's existing deployment (Node1) allows anonymous connections on its plaintext 1883 listener — anyone on the local network can publish to the weather topics today. Acceptable for a lab on a trusted LAN; revisit (password file already exists at `/srv/docker/mosquitto/config/pwfile`, unused) before any exposure beyond the local network. | Node1 `mosquitto` deployment | Small | Low | Low today (LAN-only lab) — would become High if this deployment is ever network-exposed |
| R-55 | Snow-cover model's initial state (`PvSnowState` at the start of a forecast trajectory) only has the forecast-only fallback implemented — no cross-check against live PV telemetry deviation (`AssetState.power_deviation_kw`) to detect "actually covered right now" the way `docs/architecture/weather_forecast.md` describes as the preferred source. | `VEN/src/entities/pv_snow.rs` | Small | Low | Low — accuracy improvement for a specific, infrequent edge case |

### Cross-crate duplication

| ID | Description | Affected files | Effort | Risk | Gain |
|----|-------------|----------------|--------|------|------|
| R-32 | `VTN/bff/src/vtn_client.rs` duplicates `VEN/src/vtn.rs`'s OAuth token + 401-retry + get/put-JSON plumbing (~300 lines each). Separate crates — extraction needs a shared workspace crate; record only, don't force. | `VTN/bff/src/vtn_client.rs`, `VEN/src/vtn.rs` | Medium | Low | Low-Medium — real duplication (bug fixes must be applied twice), but deliberately not forced until a shared crate is worth the indirection |

### Tooling & test infrastructure

| ID | Description | Affected files | Effort | Risk | Gain |
|----|-------------|----------------|--------|------|------|
| R-35 | No script regenerates the module dependency graph — the SESSION_START.md quarterly check is manual. Add `scripts/gen_module_graph.py` emitting Mermaid from `use crate::` imports (test code excluded). | `scripts/` | Small | Low | Low — removes manual toil from a quarterly check |
| R-61 | `timeline_grid.feature :: Each asset array contains a now-point between history and future` is timing-dependent — observed to fail intermittently ("now-point at index 120 is not between history and future (array length 121)") on a Node1 E2E run where it had passed cleanly on an earlier run the same day, no code changes to the timeline/grid path in between. Likely an off-by-one at the exact boundary when "now" lands on the last grid slot. | `tests/features/timeline_grid.feature`, `tests/features/steps/timeline_grid_steps.py` | Small | Low (flake) | Medium — an E2E flake that could intermittently fail unrelated PRs' CI runs |
| R-62 | `pv_irradiance_one_shot.test.ts` (opt-in live-VEN integration test, skipped when unreachable) assumes "natural irradiance" is roughly static across its ~10 s window, but Node1's simulator is fed by live real-time weather data — cloud cover/sun-angle can shift the natural value mid-test faster than the injected offset's decay, so the after-decay assertion intermittently fails even after fixing two real bugs found alongside it (fixed 2026-07-31: hostname `Node1` isn't a real DNS/hosts entry so `getaddrinfo` flaked on Windows — switched to the LAN IP; injected offset was a fixed `+0.6` that violates the server's `[0,1]` clamp near solar noon — made it direction-aware based on headroom). Needs either mocking the simulator's weather input for this test or dropping the trend assertions in favor of only the one-shot-consumed check. | `VEN/ui/src/__tests__/pv_irradiance_one_shot.test.ts` | Medium | Low (flake, opt-in test only) | Low-Medium — an intermittently-flaky opt-in integration test, not a CI gate |
| R-65 | Narrowed by GB-31 (2026-08-19): `Plan.solve_status` now reads `good_lp`'s real `Solution::status()` (`Optimal`/`TimeLimit`/`GapLimit`, new `SolveStatus` variants) instead of being hardcoded, so an operator can at least see when a plan wasn't certified optimal. What's still missing: the achieved gap as a *number* — `good_lp`'s public `Solution` trait exposes only that coarse status, not the underlying `highs::SolvedModel::mip_gap()` float; reaching it means bypassing `good_lp`'s solve path (which drops the `SolvedModel` after extracting the solution) and reimplementing its private `Variable`→column-index mapping by hand — confirmed by reading `good_lp` 1.15.2's and `highs` 2.4.0's source, not assumed. `Plan.mip_gap_target` therefore still persists only the *configured* tolerance, not the achieved value — and since GB-40 made that tolerance a per-profile setting (`planner.mip_gap_target`, default `0.02`) rather than a const, the missing achieved-gap number is now the *only* way to tell what a given gap setting actually bought, which has to be measured offline instead (`milp_planner/tests/solve_cost.rs::bench_mip_gap_sweep`). | `VEN/src/controller/milp_planner/types.rs`, `VEN/src/controller/milp_planner/solver_phase1.rs` | Medium — real fix needs bypassing `good_lp`'s ergonomic layer for the solve step | Low | Low — diagnostic-quality gap, not a functional defect; `SolveStatus` already covers the more actionable half |

### Watch-list (not violations)

| ID | Description | Gain |
|----|-------------|------|
| R-40 | File-size near-cap watch (production lines, 2026-07-16): `simulator/mod.rs` 470/500, `milp_planner/results.rs` 415/500, `tasks/poll_events.rs` 162/200, `tasks/planning.rs` ~198/200. Split proactively when next touched; `scripts/audit_file_sizes.py` is the authority. (`state/mod.rs` crossed the cap 2026-08-10 while adding the capacity-limit envelope and was split — its tariff/capacity/alert/SIMPLE/dispatch-window `AppState` accessors moved to `state/grid_signals.rs`, following the existing `state/obligations.rs`/`state/arbiter.rs` split-impl pattern. `services/planning.rs` crossed the cap 2026-08-23 during R-29's `solve_plan` panic-fallback fix and was split the same way — its `impl PlanningService` block moved to `services/planning/service.rs`. `tasks/sim_tick/tick.rs` sits exactly at the 200/200 cap as of R-59 — two lines were hoisted into `tasks/sim_tick/context.rs` to make room for the new comms-loss params; any future addition to the tick pipeline needs a consolidation pass or a split before it can land. `profile/schema.rs` sits at 499/500 as of GB-40's `mip_gap_target` field, which briefly pushed it to 507 until its doc comment was trimmed — the next field added to any config struct there needs a split first, most naturally `PlannerConfig` into its own module.) | N/A — monitoring only, not an actionable fix until a cap is actually crossed |

---

## Notes

- `AssetProfile` (YAML, `profile.rs`) and `AssetConfig` (runtime physics, `assets/mod.rs`)
  share variant names but hold different inner types. Consider renaming `AssetProfile` →
  `AssetSpec` to avoid newcomer confusion.
- `SimInjectState` mixes three injection behaviours in one flat struct. A tagged `InjectBehaviour`
  enum per field would clarify intent. Track here if promoted to a formal debt item.
- 2026-07-15 recalibration (Part D, following WP5.2): simulated appliance spikes
  (`assets/base_load.rs`) switched from Gaussian pulses (`amplitude × sigma_h × √(2π)`
  energy, uncontrollable tails) to trapezoidal pulses (`amplitude × (duration_h − ramp_h)`,
  directly tunable to real appliance draw), roughly halving ven-1's daily spike energy
  (8.97 kWh/day → ~3.9 kWh weekday / ~4.9 kWh weekend). `AssetHeuristics.daytime_profile_kw`
  was restructured from one 24-hour curve + a `weekday_weights[7]` scalar multiplier to
  `[Vec<f64>; 2]` (weekday/weekend), and profiles now carry weekday-conditional spikes
  (brunch replacing coffee+lunch, dinner shifted earlier on Sat/Sun).
- 2026-08-30 revisit: the weekday/weekend limit above was revisited and lifted.
  `AssetHeuristics.daytime_profile_kw` is now `[Vec<f64>; 7]`
  (`chrono::Weekday::num_days_from_monday()`-indexed — Mon=0..Sun=6), so individual
  weekdays (e.g. Friday-evening routines vs. Tuesday) are distinguishable, not just
  weekday-vs-weekend. The sample-starvation concern the old note raised was addressed two
  ways rather than by capping bucket count: (1) `HeuristicsConfig`'s
  `rolling_window_days`/`ewma_halflife_days` moved from 42/14 to 56/28 — widened enough to
  give each day-of-week bucket a comparable effective sample size to the old weekend
  bucket's, but deliberately kept well under the ~91-day season boundary so the existing
  30-day-vs-window `seasonal_factor` mechanism still holds; (2) the learner's discrete
  zero/nonzero fallback (a bucket/hour cell with no ticks used the flat `overall_mean`, any
  data at all got 100% trust) was replaced with a continuous shrinkage blend
  (`shrinkage_k_days`, default 2.5) that leans a thin bucket toward `overall_mean` in
  proportion to how much data it actually has, rather than an all-or-nothing cliff. All
  three knobs, plus `min_samples_for_confidence`, are now profile-configurable
  (`profile/heuristics.rs`, `Profile.heuristics`). Known residual limitation:
  `min_samples_for_confidence` remains a *global* cold-start gate, not per-bucket — a
  specific day-of-week bucket can in principle be far sparser than the overall average
  without tripping it (thin buckets are protected by shrinkage instead, not by this gate).

---

## Implementation Task List — Gain: High or Medium Items

Scope: every item currently rated Gain exactly **High** or **Medium** (no compound levels
like Medium-High/Low-Medium). No item is currently rated plain High, so this is the 1 item
rated Medium: R-21 (R-22, R-52, R-56, R-24, R-08, R-64, R-63, R-43, R-31, and R-29 were also
on this list and are now resolved — see below; R-58 moved to `docs/FEATURE_VISIONS.md`
2026-08-23 — it turned out to require inventing new fault-input plumbing rather than wiring
an existing one, so it isn't a buildable debt-fix task today).

**Why R-21 is last:** its own entry has no concrete fix, only a workaround (root cause is
allocator/heap-state-dependent inside the native HiGHS library via FFI, not this codebase).
Its task below is an investigation, not a code fix.

Each item's tasks follow this repo's test-first convention (`test-first` rule, `CLAUDE.md`):
write the test, confirm it fails, implement until green. Full verification before considering
an item done: `wsl cargo test -j 2 -p ven-app` under `wsl_lock`, `cargo fmt --check`,
`cargo clippy --all-targets --all-features -- -D warnings`, `scripts/audit_file_sizes.py`;
update `docs/history/project_journal.md` and remove the item from this register once resolved.

### 1. R-21 — Investigate the intermittent `cargo test` heap-corruption crash

- [ ] 1.1 Try to minimize a standalone repro isolating `run_planner_n48_full_horizon` and
      `solve_ven3_heater_three_tier_zones_feasible` from the rest of the suite.
- [ ] 1.2 Check for a `good_lp`/HiGHS version bump that might already fix an allocator bug;
      try upgrading in isolation and re-running the full suite several times.
- [ ] 1.3 If still reproducible, file an upstream issue against `good_lp` or HiGHS with the
      minimized repro; link it from this entry.
- [ ] 1.4 If no upstream fix lands, formalize the existing workaround (e.g. a
      `scripts/`-level note or CI retry step) rather than leaving it tribal knowledge.
- [ ] 1.5 This item stays in the register until the crash stops reproducing across several
      full-suite runs — remove only then, not merely once a workaround is documented.
