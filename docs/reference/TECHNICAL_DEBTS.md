# Technical Debts Register

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
| R-18 | The EV `e_ev_extra` reward is structurally inert for MustRun/MayRun sessions: the only coupling is `ev_energy ≤ e_core + e_ev_extra` (upper bound), so the solver banks the reward by maxing the slack without charging an extra kWh — `v_ev_extra_eur_kwh` never influences allocations, only shifts the reported objective. The *cap* role still works; OPPORTUNISTIC/`*_FREE` modes use a per-slot reward instead (`free_only` branch). Fix: couple it (`ev_energy ≥ e_core + e_ev_extra` when rewarded) or move the legacy reward to per-slot form. | `VEN/src/assets/ev_milp.rs`, `VEN/src/controller/milp_planner/solver_phase2.rs` | Small | Behavioural (objective accounting) | 🟡 | Low-Medium — fixes a real accounting bug, but dispatched behavior (the cap role) is already correct |
| R-21 | `cargo test` intermittently crashes with heap corruption (SIGABRT, varying malloc messages) around the two heaviest HiGHS tests (`run_planner_n48_full_horizon`, `solve_ven3_heater_three_tier_zones_feasible`). Same tests pass clean in isolation every time; also crashes with `--test-threads=1`, so it is allocator/heap-state-dependent in the native HiGHS library, not a plain data race. Test-infra only — no production path. Workaround: run the affected module in isolation when the full suite crashes. | `VEN/src/controller/milp_planner/` (HiGHS FFI via `good_lp`), test harness only | Medium | Low (flake) | 🟡 | Medium — CI/test-suite trust, no production impact |
| R-23 | `AssetMilpContext` trait is defined in the infra ring (`controller/milp_planner/asset_port.rs`) but referenced by domain-level `solver_port.rs` (`SolveRequest` holds `Vec<Box<dyn AssetMilpContext>>`) — a domain→infra type dependency. Move the trait definition into the domain ring; milp_planner and assets/ implement/consume it. | `VEN/src/controller/solver_port.rs`, `VEN/src/controller/milp_planner/asset_port.rs` | Small | Mechanical | 🟡 | Low — architecture correctness, no behavior change |
| R-25 | `CreateUserRequestBody` (HTTP DTO for POST /requests) is defined in domain-ring `controller/user_request.rs` and imported by services and routes. Move the DTO to routes/ (or an api-types module); the domain function takes domain params. | `VEN/src/controller/user_request.rs`, `VEN/src/routes/hems/`, `VEN/src/services/user_request.rs` | Small | Mechanical | 🟡 | Low — architecture correctness, no behavior change |
| R-26 | Six task files (poll_programs, poll_reports, poll_events, obligation, state_persist, progress_ticker) repeat the `tokio::time::interval` + `loop { tick().await; … }` scaffold; poll_programs vs poll_reports are 0.80 similar. Extract a shared periodic-spawn helper — also centralizes supervision. | `VEN/src/tasks/` | Small | Low | 🟡 | Low — maintenance/consistency only |
| R-29 | ~24 `unwrap()/expect()` calls in VEN production paths (milp_interactions.rs ×4, common/mod.rs ×4, services/planning.rs ×3, user_request.rs ×2, routes/hems/sessions.rs ×2, openadr_interface.rs ×2, heater/ev/battery_milp.rs ×2 each, sim_tick/tick.rs, services/hems.rs, milp_planner/inputs.rs ×1 each). Triage each: convert to Result or add a safety-justifying comment. | `VEN/src/` | Small | Low | 🟡 | Medium — real panic/crash risk if any untriaged call sees an unexpected input |
| R-31 | VTN BFF flattens every upstream error to `502 BAD_GATEWAY` with a stringified anyhow chain — VTN 4xx validation/conflict errors surface to the UI as 502. Propagate the upstream status class where known (current behaviour is pinned by a unit test in `error.rs`). | `VTN/bff/src/error.rs`, `VTN/bff/src/vtn_client.rs` | Small | Low | 🟡 | Medium — user/ops-facing error diagnosis quality |
| R-33 | UI test gaps: `VTN/ui/src/pages/Metrics.tsx` is the only untested page in either UI; `JsonDialog.tsx` is byte-identical in both UIs (50 lines — accept the copy with a twin-note header, or fold into a shared package if one materializes). | `VTN/ui/src/pages/Metrics.tsx`, `*/ui/src/components/JsonDialog.tsx` | Small | Low | 🟡 | Low — test-coverage gap |
| R-34 | Up to ~112 of 417 behave step definitions look unused (crude static match, false positives likely). Run `behave --dry-run` in the Node1 test container for the authoritative list, then delete dead steps. | `tests/features/steps/` | Small | Low | 🟡 | Low — repo hygiene only |
| R-41 | Full-E2E-run degradation (observed 2026-07-17, 18 scenario failures): under the complete suite, VEN-1 progressively stops showing new (esp. targeted) events/programs and its report submissions fail, while the identical feature sequence passes in isolation. Correlates with a VTN warn-storm: before-feature cleanup (`environment.py`) hard-deletes programs/events while VEN caches still hold them, so auto/obligation reporters churn 409s (`report_report_name_uindex`) every tick. Investigate: does sim_tick/publish report churn delay event-cache refresh; add VEN cache invalidation for upstream-deleted objects; consider cleanup draining VEN caches. Note: this VTN fork maps FK violations to 409 too (openleadr-rs error.rs) — VEN error paths must always surface the problem body (done, `fix/report-upsert-409-transparency`). | `tests/features/environment.py`, `VEN/src/tasks/sim_tick/publish.rs`, `VEN/src/tasks/poll_events.rs` | Medium | Medium (E2E reliability) | 🟠 | Medium-High — blocks reliable full-suite E2E confidence, 18 scenario failures observed |
| R-42 | `reports_steps.py` submits reports with the fixed `reportName` "TELEMETRY_USAGE" (an OpenADR payload-type constant, not a name). `report_name` is globally unique on the VTN (`report_report_name_uindex`), so the fixed name collides across scenarios/clients and exercises the upsert path unintentionally. Switch to per-scenario unique names (needs sign-off: changes test fixtures). | `tests/features/steps/reports_steps.py` | Trivial | Low | 🟡 | Low — test-fixture correctness only |
| R-43 | `entities/history.rs::ReportSent` + `HistoryPort::append_report_sent` and the `GET /history/reports` route are fully wired end-to-end but no production call site ever invokes `append_report_sent` (only exercised in `history_store` unit tests) — found while implementing WP-T5. `GET /history/reports` therefore always returns empty. Wire it into the real report-submission call sites: `tasks/sim_tick/publish.rs::run_measurement_reports`, `services/obligation.rs`, and `routes/reports.rs`. | `VEN/src/tasks/sim_tick/publish.rs`, `VEN/src/services/obligation.rs`, `VEN/src/routes/reports.rs`, `VEN/src/history_store/mod.rs` | Small | Low (silent gap, no incorrect behaviour) | 🟡 | Medium — a whole user-visible endpoint (`/history/reports`) is currently silently empty |
| R-58 | Unconfirmed whether `PlanTrigger::CapacityChange`/`Alert` are actually wired to asset-level faults (thermal derate, BMS fault, breaker trip) rather than only tariff/VTN-sourced capacity changes — found during the deviation-scenarios analysis. Needs a verification pass: trace every call site that constructs these variants and confirm at least one covers an asset-originated fault, or extend one to. | `VEN/src/entities/asset.rs`, `VEN/src/services/planning.rs`, `VEN/src/assets/` | Small | Medium (unhandled asset fault may not trigger a replan) | 🟡 | Medium — an unwired asset fault could silently fail to trigger a needed replan |

## Low priority (🔵) — by topic

### Architecture & type placement

| ID | Description | Affected files | Effort | Risk | Gain |
|----|-------------|----------------|--------|------|------|
| R-28 | `VEN/src/models.rs` is a 34-line grab-bag (`SensorSnapshot`/`SensorInput`) predating the ring layout. Fold into entities/ (or a simulator-owned module) and delete. | `VEN/src/models.rs` + 5 importers | Trivial | Mechanical | Low — repo hygiene only |
| R-39 | `state/mod.rs` mixes app wiring (`AppState`) with domain-ish value types (`EvSettings`, `HemsState`). Decide whether the two value types move to entities/ (as `AssetLedgerEntry` did) or stay — record the conclusion either way. | `VEN/src/state/mod.rs` | Trivial | Mechanical | Low — architecture clarity, no behavior change |
| R-47 | `AppState` keeps accumulating flat diagnostic fields (VTN connection status, storage-ok flag, per-task status map, etc.) added ad hoc per WP (T1/T3). No grouping/namespacing, so it will keep growing linearly with every future observability WP. Consider a `diagnostics: DiagnosticsState` sub-struct. Found during the WP-T1/T3/T5/T7 combined code review (2026-07-18). | `VEN/src/state/mod.rs` | Small | Low | Low-Medium — prevents compounding maintenance debt on every future observability WP |

### Code & repo hygiene

| ID | Description | Affected files | Effort | Risk | Gain |
|----|-------------|----------------|--------|------|------|
| R-27 | Hard-coded tuning constants: task intervals (`state_persist.rs:8` 15 s, `progress_ticker.rs:15` 1 s) and MILP solver tolerance `with_mip_gap(0.02)` (`solver_phase1.rs:151`). Name them and/or expose via config/PlannerParams. | tasks/, milp_planner/ | Trivial | Low | Low — config flexibility only |
| R-30 | 32 `console.log` calls in UI production code (`[VEN-UI]`-style debug logging). Strip or gate behind a debug flag/logger utility. | `VEN/ui/src/`, `VTN/ui/src/` | Trivial | Low | Low — hygiene/minor info-leak concern |
| R-36 | Lint/doc hygiene bundle: (a) module-wide `#![allow(dead_code)]` without justification in `entities/capacity.rs:5`, `entities/design_vocabulary.rs:7`; (b) 12 eslint warnings (exhaustive-deps, mixed exports); (c) ~~eslint lints the generated `VTN/ui/coverage/` dir~~ — fixed 2026-08-05: turned out worse than lint noise, the whole generated dir (27 files, 423K) was actually committed to git because `VTN/ui/.gitignore` was missing the `coverage/` line `VEN/ui/.gitignore` already had; untracked and added; (d) `solve_ven3_heater_three_tier_zones_feasible` runs >60 s in debug `cargo test` — consider a smaller horizon variant; (e) "Stage 5 —" phase labels in `entities/user_request.rs` / `controller/user_request.rs` doc comments — drop the prefixes. | entities/, VEN/ui, VTN/ui, milp_planner/tests | Small | Low | Low — mostly cosmetic; (d) has a small developer-friction upside (faster test runs) |
| R-38 | (a) `VEN/Cargo.toml` carries blueprint-era comments (commented-out `openleadr-client` etc.); (b) verify `VTN/data/db` (runtime artifact) is gitignored. | `VEN/Cargo.toml`, `VTN/data/` | Trivial | Low | None — pure hygiene |
| R-44 | `/health` handler (`routes/system.rs::health`) deep-clones the full `VtnConnectionStatus` and active `Plan` on every poll just to read a couple of fields. Cheap today but grows with `Plan` size; consider a narrower state accessor. Found during the WP-T1/T3/T5/T7 combined code review (2026-07-18). | `VEN/src/routes/system.rs` | Trivial | Low | Low — cheap today, future-proofing only |
| R-45 | `routes/reports.rs::post_reports` and `put_report` duplicate the `submission_outcome()` call-and-record logic almost verbatim (WP-T5). Extract a shared helper. Found during the WP-T1/T3/T5/T7 combined code review (2026-07-18). | `VEN/src/routes/reports.rs` | Trivial | Low | Low — maintenance/consistency only |
| R-46 | Ring-buffer eviction (push-and-truncate-to-capacity) is duplicated near-identically in at least 3 places (`state/event_log.rs`, `state/report_submissions.rs`, and a third ring state module). Extract a shared `RingBuffer<T>` helper. Found during the WP-T1/T3/T5/T7 combined code review (2026-07-18). | `VEN/src/state/event_log.rs`, `VEN/src/state/report_submissions.rs` | Small | Low | Low — maintenance/consistency only |
| R-65 | `VEN/src/profile/schema.rs` is 505 production lines, 5 over the 500-line file-size cap — pushed over by `6b80be8` (`sim_inject_enabled`/`SimulatorConfig` additions, 2026-08-07). Split a few of the smaller config structs (e.g. `GridConfig`/`PlannerConfig`) into a new `profile/schema_*.rs` submodule. Found via `scripts/audit_file_sizes.py` while merging `044-forecast-accuracy-tracking` (unrelated branch, didn't touch this file). | `VEN/src/profile/schema.rs` | Trivial | Low | None — pure hygiene, restores the file-size gate |

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

### Deviation/fault handling & forecast feedback

| ID | Description | Affected files | Effort | Risk | Gain |
|----|-------------|----------------|--------|------|------|
| R-59 | No documented fail-safe behaviour on communication loss to the VTN or to an asset controller — found during the deviation-scenarios analysis. Assets appear to hold their last commanded setpoint by default, but this isn't a deliberate design, just the absence of any watchdog. Separate fault-handling/watchdog design, out of scope for the deviation arbiter specifically. | `VEN/src/vtn.rs`, `VEN/src/assets/` | Medium | Medium | Medium-High — comms loss is a real, foreseeable failure mode with currently undefined behavior |
| R-60 | The heuristic base-load forecast (`services/heuristics.rs`, `AssetHeuristics.daytime_profile_kw`) has no error-feedback loop against measured actuals — it only ever backfills from history, never adjusts based on how wrong its own past predictions were. Found during the deviation-scenarios analysis (scenario E, base-load slow drift). | `VEN/src/services/heuristics.rs` | Medium | Low | Low-Medium — forecast accuracy improvement, no correctness risk today |

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

### Watch-list (not violations)

| ID | Description | Gain |
|----|-------------|------|
| R-40 | File-size near-cap watch (production lines, 2026-07-16): `services/planning.rs` 473/500, `simulator/mod.rs` 470/500, `milp_planner/results.rs` 415/500, `state/mod.rs` 412/500, `tasks/poll_events.rs` 162/200, `tasks/planning.rs` ~198/200. Split proactively when next touched; `scripts/audit_file_sizes.py` is the authority. | N/A — monitoring only, not an actionable fix until a cap is actually crossed |

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
  (brunch replacing coffee+lunch, dinner shifted earlier on Sat/Sun). **Deliberate scope
  limit, not an oversight:** the split is weekday-vs-weekend (2 buckets), not one curve per
  day of the week (7 buckets) — chosen because 4 weeks of history gives each weekend bucket
  ~8 days of samples (plenty for a stable mean) while a 7-way split would starve each
  individual weekday bucket to ~4 samples. Revisit if per-weekday granularity (e.g.
  distinguishing Friday-evening routines from Tuesday) is ever wanted — would need a longer
  seeding window before it's statistically meaningful.

---

## Implementation Task List — Gain: High or Medium Items

Scope: every item currently rated Gain exactly **High** or **Medium** (no compound levels
like Medium-High/Low-Medium). No item is currently rated plain High, so this is the 5 items
rated Medium: R-21, R-29, R-31, R-43, R-58 (R-22, R-52, R-56, R-24, R-08, R-64, and R-63 were
also on this list and are now resolved — see below). Ordered by dependency, not by ID — work
top-to-bottom.

**Why this order:**

1. **R-58** — asset-level fault-trigger verification (`entities/asset.rs`,
   `services/planning.rs`, `assets/`).
2. **R-29** — the `unwrap()`/`expect()` triage across all listed call sites, including
   heater/ev/battery_milp.rs (R-08's dispatch-macro refactor turned out to be a pure
   mechanical dispatch/file-organization change unrelated to error handling, so it did not
   fold this triage in as originally planned — those 6 asset-file call sites remain part of
   R-29's own scope, not a separate already-done step).
3. **R-31, R-43** — both fully independent of everything above (R-31 is VTN/bff-only;
   R-43 wires VEN report-submission call sites nothing else on this list touches). Order
   between them doesn't matter; listed in ID order.
4. **R-21** — deliberately last and separate: its own entry has no concrete fix, only a
   workaround (root cause is allocator/heap-state-dependent inside the native HiGHS library
   via FFI, not this codebase). Its task below is an investigation, not a code fix.

Each item's tasks follow this repo's test-first convention (`test-first` rule, `CLAUDE.md`):
write the test, confirm it fails, implement until green. Full verification before considering
an item done: `wsl cargo test -j 2 -p ven-app` under `wsl_lock`, `cargo fmt --check`,
`cargo clippy --all-targets --all-features -- -D warnings`, `scripts/audit_file_sizes.py`;
update `docs/history/project_journal.md` and remove the item from this register once resolved.

### 1. R-58 — Verify `PlanTrigger::CapacityChange`/`Alert` cover asset-level faults

- [ ] 1.1 Trace every call site constructing `PlanTrigger::CapacityChange`/`Alert` in
      `services/planning.rs` and `assets/`.
- [ ] 1.2 Confirm at least one covers an asset-originated fault (thermal derate, BMS fault,
      breaker trip), not only tariff/VTN-sourced capacity changes.
- [ ] 1.3 If none do, write a failing test for the missing case (e.g. a simulated thermal
      derate should emit `CapacityChange`) and extend the relevant call site.
- [ ] 1.4 Full verification; remove R-58 from this register.

### 2. R-29 — Triage the `unwrap()`/`expect()` call sites

- [ ] 2.1 List all call sites: `milp_interactions.rs` ×4, `common/mod.rs` ×4,
      `services/planning.rs` ×3, `user_request.rs` ×2, `routes/hems/sessions.rs` ×2,
      `openadr_interface.rs` ×2, `heater/ev/battery_milp.rs` ×2 each, `sim_tick/tick.rs` ×1,
      `services/hems.rs` ×1, `milp_planner/inputs.rs` ×1.
- [ ] 2.2 For each: convert to `Result` if the panic path is reachable with attacker/user-
      controlled or otherwise fallible input; otherwise add a one-line safety-justifying
      comment explaining why it can't panic in practice.
- [ ] 2.3 Full verification; remove R-29 from this register.

### 3. R-31 — Propagate VTN BFF upstream error status class

- [ ] 3.1 Write a failing unit test in `VTN/bff/src/error.rs` asserting a VTN 4xx
      validation/conflict error surfaces as its own status class, not a flattened 502.
- [ ] 3.2 Implement status-class propagation in `error.rs`/`vtn_client.rs` where the upstream
      status is known; keep 502 only for genuine gateway/connectivity failures.
- [ ] 3.3 Update the existing pinning unit test in `error.rs` to match the new behavior.
- [ ] 3.4 Full verification; remove R-31 from this register.

### 4. R-43 — Wire `append_report_sent` into real report-submission call sites

- [ ] 4.1 Write a failing integration test: submitting a report via the real call path results
      in a row visible through `GET /history/reports`.
- [ ] 4.2 Call `HistoryPort::append_report_sent` from `tasks/sim_tick/publish.rs::run_measurement_reports`.
- [ ] 4.3 Call it from `services/obligation.rs` and `routes/reports.rs`'s submission paths too.
- [ ] 4.4 Confirm `GET /history/reports` returns non-empty results after a real submission
      (BDD scenario, if practical, per this project's E2E conventions).
- [ ] 4.5 Full verification; remove R-43 from this register.

### 5. R-21 — Investigate the intermittent `cargo test` heap-corruption crash

- [ ] 5.1 Try to minimize a standalone repro isolating `run_planner_n48_full_horizon` and
      `solve_ven3_heater_three_tier_zones_feasible` from the rest of the suite.
- [ ] 5.2 Check for a `good_lp`/HiGHS version bump that might already fix an allocator bug;
      try upgrading in isolation and re-running the full suite several times.
- [ ] 5.3 If still reproducible, file an upstream issue against `good_lp` or HiGHS with the
      minimized repro; link it from this entry.
- [ ] 5.4 If no upstream fix lands, formalize the existing workaround (e.g. a
      `scripts/`-level note or CI retry step) rather than leaving it tribal knowledge.
- [ ] 5.5 This item stays in the register until the crash stops reproducing across several
      full-suite runs — remove only then, not merely once a workaround is documented.
