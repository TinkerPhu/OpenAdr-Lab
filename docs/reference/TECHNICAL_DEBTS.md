# Technical Debts Register

> **Next ID: R-134.** Use this number for the next new item, then increment this line. It counts
> every ID ever issued, so never decrement it and never derive it from the highest row present: a
> removed row does not free its number. On a merge keep the higher of the two numbers.
>
> Rows verified against the source on 2026-10-09 (numbers re-measured, paths checked).
>
> **Rule:** Before adding a feature in an affected area, check this file first.
> Refactor the relevant debt before adding new behaviour if effort is Small or Trivial.
> Enforced by `python scripts/audit_debt_gate.py` (tests: `scripts/test_audit_debt_gate.py`):
> a branch that touches a Small/Trivial row's `Affected files` must delete the row or carry a
> `Debt-deferred: R-NN: <reason>` commit line. Keep `Affected files` as backticked paths.
>
> IDs are stable and never reused; gaps in the numbering are resolved items
> (resolutions live in `docs/history/project_journal.md` and git history).
>
> Every row carries **Sev** (S1 wrong output/data/wire or production failure; S2 untrustworthy
> instruments or reliability; S3 slows work or misleads readers; S4 cosmetic/future-proofing),
> **Kind** (weighted in `docs/reference/ISSUE_KINDS.md`), **Cost** (under ~1 hour is fixed, never
> filed) and **Why open** (`needs-decision`, `needs-evidence`, `too-big` with its first step, or
> `FIX` = no blocker, fix it in the next branch). Rows are sorted by Kind weight, then Sev: the
> order is the priority. `docs/BACKLOG.md` (`GB-*`, `BL-*`) uses the same fields.

---

## Open issues — sorted by kind weight, then severity

| ID | Sev | Kind | Cost | Why open | Affected files | Description |
|----|-----|------|------|----------|----------------|-------------|
| R-94 | S3 | bug | Medium | too-big: first step: a latched-minimum stored-energy constraint in heater_milp.rs derived from thermostat_delta_c | `VEN/src/assets/heater_milp.rs`, `VEN/src/assets/heater_thermostat.rs` | The MILP does not model the heater's thermostat deadband, so the asset can refuse planned dispatch. `heater_milp.rs` is at 500/500 production lines, so the fix has to move code out first. Details below. |
| R-32 | S3 | duplication | Medium | too-big: first step: a shared OAuth client crate | `VTN/bff/src/vtn_client.rs`, `VEN/src/vtn.rs` | `VTN/bff/src/vtn_client.rs` duplicates `VEN/src/vtn.rs`'s OAuth token + 401-retry + get/put-JSON plumbing (~300 lines each). Separate crates — extraction needs a shared workspace crate; record only, don't force. |
| R-110 | S3 | architecture | Medium | too-big: first step: split `state/mod.rs` by concern (requests/sessions, plan, controller trace, settings) behind the same `AppState` handle | `VEN/src/state/mod.rs`, `VEN/src/simulator/mod.rs`, `VEN/src/simulator/capacity_headroom.rs` | God objects: `state/mod.rs` has about 100 public items; `simulator/mod.rs` 54, plus a 1366-line `capacity_headroom.rs` holding domain-ring computations inside the infra ring. (The `AppCtx` part is done: no handler takes the whole of it since R-110's first step, audit rule 11.) |
| R-118 | S3 | architecture | Medium | too-big: first step: split `build_milp_inputs` into its named stages (tariffs and limits, per-asset scalars, penalty and capacity inputs) | `VEN/src/controller/milp_planner/inputs.rs`, `VEN/src/controller/milp_planner/results.rs`, `VEN/src/profile/validate.rs` | **23 production functions are over 100 lines** (`scripts/audit_duplication_baseline.json` lists them), because `audit_file_sizes.py` measures files, not functions, so a file can stay under 500 lines while one function in it does everything. The worst on 2026-10-09: `build_milp_inputs` 394, `translate_to_plan` 366, `profile::validate` 357, `ev_session_context::from_state` 232, `tick_once` 182, `build_measurement_report_for_obligation` 175, `build_plan_envelopes` 163. R-40 records that cap-driven file splits moved code without simplifying it. The ratchet in `scripts/audit_duplication.py` now refuses a new or longer one, so the count can only fall; these 23 remain to be split. |
| R-129 | S3 | architecture | Small | too-big: first step: put `VtnPort` (as `Arc<dyn VtnPort>`) in `AppCtx` beside the client and move `post_reports` / `put_report` onto it, with a unit test through `services/test_support/mock_vtn.rs` | `VEN/src/routes/reports.rs`, `VEN/src/routes/system.rs`, `VEN/src/app_ctx.rs` | **The report routes bypass `VtnPort` and call the concrete `VtnClient`**, so `post_reports` / `put_report` cannot be unit-tested (a comment in `routes/reports.rs` says so); `/health` and `/vtn/status` reach the concrete client too (`token_expires_at`). Sweep #3. |
| R-131 | S4 | architecture | Small | too-big: first step: move `SensorSnapshot` / `SensorRaw` / `SensorInput` to `entities/` (no behaviour change), then translate `VtnHttpError` to a `DomainError` variant in `vtn.rs` | `VEN/src/simulator/snapshot.rs`, `VEN/src/state/mod.rs`, `VEN/src/routes/events.rs`, `VEN/src/services/obligation.rs` | **Infrastructure types used inside.** `SensorSnapshot` / `SensorInput` are defined in `simulator/` but used by `state/` and `routes/`; `services/obligation.rs` downcasts to `vtn::VtnHttpError` instead of receiving a `DomainError` translated at the boundary (`docs/guidelines/ERROR_HANDLING.md`). Sweep #5. |
| R-87 | S3 | wire-contract | Medium | too-big: first step: make intervalPeriod.start Option upstream | `openleadr-rs/openleadr-wire/src/interval.rs`, `openleadr-rs/openleadr-wire/src/event.rs`, `lab-core/src/event_timing.rs` | **This stack is stricter than the 3.1 schema about `intervalPeriod.start`.** The schema gives `intervalPeriod` no `required:` list at all (`1_OpenADR_3.1.0_20250801.yaml`, ~line 2154), so a conformant peer may omit `start`; `openleadr-wire` makes it a non-`Option` `DateTime<Utc>` and carries its own `// FIXME field not required, though, it's unclear how to interpret it if it's missing` (`interval.rs:43`). Since 3.1b the VEN parses events with those types, so it refuses such an event -- which `wire-contracts` explicitly forbids ("never reject a peer for omitting what the spec lets it omit"). Not newly introduced by the VEN: our **VTN** already parses incoming events with the same types, so it rejects them at the API boundary first, and no such event can reach a VEN in this lab. Fixing it properly means making the field `Option` upstream (their FIXME) and teaching `EventRequest::ends_at()` -- our own P-1 patch -- to cope with an unanchored period; that is an upstream PR, not a local workaround. Until then the deviation is on the VTN's side of the wire and is documented rather than hidden. |
| R-133 | S4 | wire-contract | Medium | too-big: first step: increment 2, the plan and planner types (`Plan`, `PlanTimeSlot`, `AssetAllocation`, `PlanSummary`, `SolveStatus`, `WarningKind`, `PlannerObjective` (hand-written `impl Serialize`, needs `#[ts(type)]`), `PlanPhaseReport`, `PlanHistorySample`, `PlannerEvent`, `FlexibilityEnvelope`) into `VEN/src/ui_types.rs` | `VEN/ui/src/api/types.ts`, `VEN/src/ui_types.rs` | **Most of the VEN UI's API types are still a hand-written copy of the Rust ones.** Increment 1 (user requests and sessions) is generated by ts-rs and guarded by `generated_ui_types_are_current`; it found real drift (`EvSession.origin`, `comfort_rates` on sessions and requests, missing in TS). Left, by area: plan/planner; simulator/assets; system, history, weather, measurement, notifications, arbiter, EV settings, tariffs and capacity plus the request bodies (`#[ts(optional_fields)]`); OpenADR objects (`Oadr*`, aliased) and the 21 `json!()` route bodies, which first need a typed response struct. Sweep #9. |
| R-65 | S4 | ui-transparency | Medium | too-big: first step: read the achieved gap past good_lp's solve path | `VEN/src/controller/milp_planner/types.rs`, `VEN/src/controller/milp_planner/solver_phase1.rs` | Narrowed by GB-31 (2026-08-19): `Plan.solve_status` now reads `good_lp`'s real `Solution::status()` (`Optimal`/`TimeLimit`/`GapLimit`, new `SolveStatus` variants) instead of being hardcoded, so an operator can at least see when a plan wasn't certified optimal. What's still missing: the achieved gap as a *number* — `good_lp`'s public `Solution` trait exposes only that coarse status, not the underlying `highs::SolvedModel::mip_gap()` float; reaching it means bypassing `good_lp`'s solve path (which drops the `SolvedModel` after extracting the solution) and reimplementing its private `Variable`→column-index mapping by hand — confirmed by reading `good_lp` 1.15.2's and `highs` 2.4.0's source, not assumed. `Plan.mip_gap_target` therefore still persists only the *configured* tolerance, not the achieved value — and since GB-40 made that tolerance a per-profile setting (`planner.mip_gap_target`, default `0.02`) rather than a const, the missing achieved-gap number is now the *only* way to tell what a given gap setting actually bought, which has to be measured offline instead (`milp_planner/tests/solve_cost.rs::bench_mip_gap_quality_sweep`). |
| R-95 | S4 | ui-transparency | Small | needs-decision: an earlier attempt was reverted; decide whether the extra series is wanted | `VEN/ui/src/components/controller/charts/SiteHeadroomChart.tsx`, `VEN/ui/src/components/controller/charts/CapacityForecastChart.tsx`, `VEN/src/entities/capacity_curve.rs` | No sustained-commitment series exists alongside the capability curves. |
| R-21 | S2 | test-infra | Medium | needs-evidence: minimise a standalone repro of the HiGHS heap corruption | `VEN/src/controller/milp_planner/` (HiGHS FFI via `good_lp`), test harness only | `cargo test` intermittently crashes with heap corruption (SIGABRT, varying malloc messages) around the two heaviest HiGHS tests (`run_planner_n48_full_horizon`, `solve_ven3_heater_three_tier_zones_feasible`). Same tests pass clean in isolation every time; also crashes with `--test-threads=1`, so it is allocator/heap-state-dependent in the native HiGHS library, not a plain data race. Test-infra only — no production path. Workaround: run the affected module in isolation when the full suite crashes. |
| R-97 | S2 | performance | Medium | too-big: first step: treat the slack-poor heater VENs specifically (a looser `mip_gap_target` or coarser far zones per profile, a wider thermostat band where the installation allows), measured with `bench_phase1_vs_tank_slack` | `VEN/src/assets/heater_milp.rs`, `VEN/src/controller/milp_planner/solver_phase1.rs`, `VEN/src/controller/milp_planner/solver_phase2.rs` | **MILP solve time sits close to its own timeout on heater VENs** (GB-40 was the same subject and is merged here, 2026-10-09). Conclusions below; every measurement is in `docs/reference/R97_PLANNER_BENCHMARKS.md`. |
| R-35 | S4 | docs | Small | needs-decision: a graph checker exists; is a generator still wanted | `scripts/` | No script regenerates the module dependency graph — the SESSION_START.md quarterly check is manual. Add `scripts/gen_module_graph.py` emitting Mermaid from `use crate::` imports (test code excluded). |
| R-62 | S4 | test-infra | Medium | needs-decision: mock the simulator's weather input or drop the trend assertions | `VEN/ui/src/__tests__/pv_irradiance_one_shot.test.ts` | `pv_irradiance_one_shot.test.ts` (opt-in live-VEN integration test, skipped when unreachable) assumes "natural irradiance" is roughly static across its ~10 s window, but the VEN it runs against (`VITE_VEN_URL`, no default) is fed by live real-time weather data — cloud cover/sun-angle can shift the natural value mid-test faster than the injected offset's decay, so the after-decay assertion intermittently fails even after fixing two real bugs found alongside it. Needs either mocking the simulator's weather input for this test or dropping the trend assertions in favor of only the one-shot-consumed check. |
| R-103 | S4 | test-infra | Small | needs-decision: shrink the horizon (and with it what the test proves) or accept the runtime | `VEN/src/controller/milp_planner/tests/heater.rs` | `solve_ven3_heater_three_tier_zones_feasible` runs over 60 s in a debug `cargo test` (split from R-36 (d) when the rest of R-36 was fixed on `052-ev-plugged-band`). A smaller horizon would make it fast but would no longer exercise ven-3's real three-tier zone layout, which is what the test is named for, so this is a choice about the test's purpose and is not made silently. |
| R-66 | S4 | test-infra | Trivial | needs-evidence: calibrate the pre-flight against a real degraded run | `run_all_tests.sh` | `run_all_tests.sh`'s GB-24 pre-flight capacity check (`MIN_AVAILABLE_MEM_MB=800`) is a first-pass heuristic from one live `ssh Node2 "free -m"` observation (2026-08-14: 3794 MB total, 2482–2919 MB available with the resident fleet running), not empirically calibrated against an actual degraded run's memory profile. Same class as R-27 (hard-coded tuning constants). May need tuning if it proves too strict (blocks a run that would've been fine) or too loose (still lets a degraded run through). |
| R-120 | S4 | test-infra | Medium | too-big: first step: one `MilpInputs` test builder, grown from `penalty.rs::base_inputs` and `capacity_schedule.rs::inputs_with`, used by the eight files that spell the literal out | `VEN/src/controller/milp_planner/tests/penalty.rs`, `VEN/src/controller/milp_planner/tests/solver.rs`, `VEN/src/controller/milp_planner/tests/mod.rs` | **The planner tests repeat their fixtures.** The 2026-10-07 scan found the heaviest test-only duplication in `milp_planner/tests/` (`solve_cost.rs` repeats itself 89 times at 8 lines; `penalty.rs`/`solver.rs` share 32; `mod.rs`/`planner.rs` 28; `cost_sign.rs`/`heater.rs`/`planner.rs` 26). The `MilpInputs` literal itself is hand-built in `capacity_schedule.rs`, `gb41_soft_deadline_core.rs`, `mod.rs`, `penalty.rs`, `pv.rs`, `soc_balance.rs`, `solver.rs` and `stale_rates.rs`, so a new field means editing each of them, and fixtures that hand-build an asset's capability are the same violation the asset-competence rule names. |
| R-71 | S4 | style | Medium | too-big: first step: a parameter object for build_solve_request | `VEN/src` (run `grep -rn "#\[allow(" VEN/src --include="*.rs" | grep -v tests/ | grep -v "// "`) | **Re-scoped 2026-10-03 after a measured pass: 50 unjustified `#[allow(...)]` sites down to 42, and the whole `unused_imports` class is gone.** The original entry listed 9 sites in the assets/simulator area; a repo-wide count found 50, of which 13 were `unused_imports`. Those were not a hygiene problem: `milp_planner/mod.rs`'s ten were suppressing a correct dead-code warning (most of the names were used by nothing — see the 2026-10-03 journal entry), and the rest became `#[cfg(test)]` re-exports, which makes the claim checkable instead of asserted. Three `dead_code` allows covered genuinely dead items, now deleted (`asset_max_power`, `HvacService`) or wired up (`UserRequestService::create_shiftable`). **What remains (re-measured 2026-10-09): 36 sites, 23 of them `clippy::too_many_arguments`** — i.e. mostly a parameter-list problem, not a comment problem. `Simulator::tick` (25 params) and `spawn_sim_tick` (21) are discharged via `simulator::TickInputs` and `boot::World`; the biggest left are `MilpParticipant::build_milp_context` (18, trait-mandated and justified on its line in most impls) and `build_solve_request` (23, in `services/planning/mod.rs`). Fix the signature where there is a real parameter object to name, and add the same-line justification only where the breadth is genuinely trait-mandated. |
| R-77 | S4 | style | Medium | too-big: first step: triage the 53 files, which rename and which keep the OpenADR sense | `VEN/src` (53 files, see grep for `envelope\|Envelope`) | Project-wide "envelope" naming audit, deferred from the 2026-09-07 site-headroom fix (`naming-envelope-vs-headroom` in `.claude/CLAUDE.md`): a grep finds 53 `VEN/src` files (2026-10-09) referencing "envelope"/"Envelope". This session renamed only the two files it was already touching (`envelope.rs`→`site_headroom.rs`, `capacity_envelope.rs`→`capacity_headroom.rs`). Remaining candidates needing individual triage (not a blanket rename): `entities::plan::FlexibilityEnvelope` (per-device-session envelope, built by `milp_planner/envelopes.rs::build_plan_envelopes`) and `SiteFlexibilityEnvelope`/`SiteFlexibilitySample` (this fix's own structs, kept as-is since renaming a `Serialize`d struct mirrored by a TypeScript type in `VEN/ui/src/api/types.ts` is a larger, separately-reviewable change) are likely internal-HEMS concepts that should rename to "headroom"; `entities/capacity.rs`'s "Dynamic Operating Envelope" is a genuine OpenADR-boundary use that should keep "envelope" (`reporter.rs`'s reservation-capacity handling was listed here too; R-76 removed it — those arms no longer read an envelope at all, they read `entities::reservation_request`); `milp_planner/envelopes.rs`'s own mathematical scheduling-constraint sense of "envelope" is ambiguous and needs its own judgment call. |
| R-125 | S4 | style | Medium | too-big: first step: find the unit of the MILP's `soc_ev` / `soc_ev_init` (fraction or kWh) in `controller/milp_planner/` and suffix them | `VEN/src/controller/milp_planner/inputs.rs`, `VEN/src/controller/milp_planner/model_skeleton.rs`, `VEN/src/routes/sim.rs`, `VEN/src/simulator/inject.rs` | SoC-shaped identifiers outside the list R-115 renamed still name no unit: `soc_ev` (about 50 uses), `soc_ev_init`, `battery_soc`, `ev_soc`, `current_soc`, `soc_drops`, `soc_target_profile`, `new_soc` and smaller ones. Not a blind `_frac` sweep: `soc_ev` in the MILP may be kWh, and `battery_soc`/`ev_soc` are also JSON keys of `/sim/inject` (keep them with `#[serde(rename)]`, as R-115 did). Each needs its unit checked. |
| R-121 | S4 | style | Medium | too-big: first step: add `@lab/ui` as a second alias for the same directory in both UIs' `vite.config.ts` and `ui-charts/tsconfig.base.json`, migrate the 48 importing files with a codemod, then rename the directory and update the two Dockerfiles' `COPY` lines | `ui-charts/tsconfig.base.json`, `VEN/ui/Dockerfile`, `VTN/ui/Dockerfile` | **`ui-charts/` no longer holds only charts.** It is the one directory both UIs share, so it also carries the Prometheus parser, `RefreshControls`, `debugLog`, `JsonDialog`, the value-or-dash formatters and the TypeScript/ESLint base configs, and a name that says 'charts' sends a reader to the wrong place (`naming-transparency`). Renaming touches 48 importing files, both Dockerfiles, two aliases and the docs, so it was not folded into the duplication work. |
| R-126 | S4 | style | Medium | too-big: first step: rename the profile-side enum (`profile/schema.rs::AssetProfile`, about 33 uses) to `AssetSpec` and let the compiler list the rest | `VEN/src/profile/schema.rs`, `VEN/src/entities/design_vocabulary.rs` | **Two different types are both called `AssetProfile`**: the YAML enum in `profile/schema.rs` and a struct in `entities/design_vocabulary.rs`. A reader following one lands in the other (`naming-transparency`). Was a note here since 2026-07; the `AssetConfig` half of that note no longer exists. |
| R-132 | S4 | style | Small | too-big: first step: drop the stale `#[allow(dead_code)]` on `MilpParticipant` / `RequestResolvable` (6 implementations exist) and let the compiler list what is really unused | `VEN/src/assets/capability_traits.rs`, `VEN/src/entities/design_vocabulary.rs`, `VEN/src/controller/settings_port.rs` | **Dead code hidden from the compiler: 40 `#[allow(dead_code)]`** (re-counted 2026-10-10, after BL-29 removed three sketch enums). The capability traits still say "no implementor yet"; `entities/design_vocabulary.rs` keeps 10 unreferenced "design sketches" in the domain ring (one with an untyped `serde_json::Value`); `controller/settings_port.rs` silences the whole module. Sweep #7. |

---

### Watch-list (not violations)

| ID | Description | Gain |
|----|-------------|------|
| R-40 | **File-size near-cap watch (production lines, re-measured 2026-10-09).** `scripts/audit_file_sizes.py` is the authority; run it and do not trust this row. At the cap edge today (cap 500): `assets/heater_milp.rs` **500**, `assets/shiftable_load.rs` 492, `controller/arbiter.rs` 483, `assets/pv.rs` 478, `services/planning/mod.rs` 477, `assets/ev_milp.rs` 477, `milp_planner/results.rs` 474, `assets/ev.rs` 469, `simulator/mod.rs` 464, `entities/asset.rs` 463, `state/mod.rs` 461, `entities/plan.rs` 460, `profile/schema.rs` 457; in `tasks/` (cap 200): `history_sampler/mod.rs` 182, `poll_events/mod.rs` 177, `sim_tick/tick.rs` 176, `poll_signals.rs` 172. **The lesson of the 2026-10-03 sweep belongs in this row:** a cap is met the right way by moving out what the ring does not own, and the wrong way by slicing orchestration thinner (names like `helpers`, `context`, `post_lock` say *when* code runs, not what it does). `heater_milp.rs` at the cap means R-94 and R-97 both start by moving code out of it. | N/A — monitoring only, not an actionable fix until a cap is crossed |

---

## R-21 — Investigate the intermittent `cargo test` heap-corruption crash

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

## R-94 — the MILP does not model the heater's thermostat deadband

**Where:** `VEN/src/assets/heater_milp.rs` (tank model) vs
`VEN/src/assets/heater_thermostat.rs` (the deadband), introduced with
`thermostat_delta_c` (2026-09-27).

The MILP bounds tank energy at `temp_max_c` (`heater_milp.rs`'s `e_max`) but knows nothing
about "once at the ceiling, stay off until `temp_max_c - thermostat_delta_c`". A plan that
wants to absorb PV surplus into a nearly-full tank can therefore be refused by the asset,
which surfaces as a plan/actual deviation rather than as an infeasibility.

**Why it is debt rather than a bug:** the divergence window is exactly the region where the
tank has almost no absorbing capacity left to plan for, and the deviation arbiter already
handles a setpoint the asset does not follow. Nothing observed on the fleet yet — recorded
because it is a new divergence class, not because it has bitten.

**To resolve:** give the heater's MILP context a "may not run below this stored-energy level
once latched" constraint derived from the same `thermostat_delta_c`, so the planner stops
proposing dispatch the asset will refuse. Pairs with the existing switching-penalty work
(`lambda_heat_sw_eur`), which is the economic half of the same anti-chatter concern.

## R-95 — no sustained-commitment series exists alongside the capability curves

**Where:** `VEN/ui/src/components/controller/charts/SiteHeadroomChart.tsx` and
`CapacityForecastChart.tsx`, fed by `CapacityCurve::steps`.

**Partially resolved (2026-09-27):** the mislabel is fixed. The series now read "Import/Export
capability", the export line's tooltip says "net import — nothing left to export" when
positive (`charts/capabilityFormat.ts`), and
`docs/use-cases/HEMS-USE-CASE-OBSERVATION-MANUAL.md` has a "How to read the Site Headroom
panel" section. Found because ven-3's export curve pointed upward at night and looked like a
sign bug; it was the site having no battery and a thermostat forcing reheats.

**What remains:** there is still no series answering "what could the site *hold* for this
window" — energy ÷ duration — which is a real and different question from instantaneous
capability.

**To resolve, if it is ever wanted:** add such a series *alongside* the capability curves,
never replacing them. The attempt that replaced them (`4459a4ab`, reverted in `e47ae50d`)
destroyed the panel's near-term forecasting value — a cumulative average carries the whole
history since the anchor, so a real −11 kW EV-departure cliff smeared into a slow decay — and
sampled the average only at the instantaneous curve's sparse breakpoints, which `stepAfter`
then held flat, overstating capability by 11 kW for two hours. Any such series must be densely
sampled and drawn as an interpolated line, not a step.

## R-97 — MILP solve time sits close to its own timeout on heater VENs

**Where:** `VEN/src/assets/heater_milp.rs` (the heater's stage integers) and the two-phase solve in
`VEN/src/controller/milp_planner/` (`solver_timeout_s` 60 s for phase 1,
`phase2_solver_timeout_s` 15 s for phase 2). Evidence, tables and the full investigation log:
`docs/reference/R97_PLANNER_BENCHMARKS.md` (and `GB40_MIP_GAP_BENCHMARK.md` for the gap sweeps).

**What is established**

- **The heater is the cost.** The same site solves in 0.19 s without its heater and hits the time
  limit with it. Heater VENs hit `TIME_LIMIT` about nine times as often as the rest of the fleet.
- **Phase 1's difficulty follows the tank's thermal slack**, a property of the installation: little
  slack (small tank, narrow band) narrows the feasible corridor until the stage integers bind.
  ven-3 (200 L, 15 K) takes 11-46 s where ven-2 (2000 L, 40 K) takes about 0.3 s; ven-20 (450 L,
  35 K) never hits a limit. Switch count does not predict it.
- **A phase-1 `TIME_LIMIT` is a quality risk**, not only CPU: the incumbent's gap is unknown and
  not observable (R-65). ven-5 and ven-17 still reach 60 s on some instances at `mip_gap_target` 0.30,
  so the gap setting does not fix it.
- **A timed-out plan can violate a capacity limit**, because the limit is a penalised slack. The
  arbiter's limit enforcement (GB-47) corrects it at execution; the planner side is still open.
- **Phase 2 works and is bounded**: it halves heater switching, its cost cap means it cannot return
  a dearer plan than phase 1, and its budget is a staircase with 15 s on the plateau (nothing below
  about 5 s, the same result at 5, 10 and 20 s). Its `TIME_LIMIT` on heater VENs is not worth chasing. The startup penalty is a per-asset-mix setting
  (`VEN/profiles/README.md`, "Planner smoothing by asset mix").
- **Heater + battery sites pulse** (ven-5, ven-17: single 5-minute heater runs through the PV
  hours). It is not a tie: it is a policy question (what a heater switch is worth in EUR) plus
  phase 2's search.
- **Three reformulations were tried and rejected**: one integer per stage (no speed-up, kept for
  other reasons), a continuous power variable bounded by the stage (fast but unsound, about 25 %
  below the true optimum) and minimum up/down times (no speed-up, worse plans).
- **Method rules**: production wall-clock cannot compare VENs (17 solvers share 4 cores), so
  compare on the bench; use a varying tariff for any phase-2 measurement.

**Open**

- Per-VEN levers that are not reformulations, which is where the last finding points: a wider
  thermostat band where the installation allows it (200 L at 15 K to 40 K roughly halves phase 1), a
  looser gap or coarser far zones only on slack-poor VENs.
- A feasibility-first step so a timed-out incumbent respects a capacity limit.
- No reformulation tested so far removes the difficulty without changing the physics: the
  relaxation is weak because the discreteness is real. R-94 (the thermostat deadband) touches the
  same model and would add to it, so cost it against this row first.
- ven-19 is a 10x outlier in its own class (battery + EV); `round_trip_efficiency` 0.93 is the
  suspect, untested.
