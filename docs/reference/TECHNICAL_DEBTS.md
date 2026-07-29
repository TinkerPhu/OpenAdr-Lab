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
| R-24 | Injectable-clock gaps outside the adapter boundary: `entities/site_meter.rs:49` (`ts: Utc::now()`), `controller/openadr_interface.rs:230` (`last_updated`), `simulator/mod.rs:156,367`, `assets/base_load.rs:108`, `assets/battery.rs:142`, `assets/ev.rs:184`, `assets/grid.rs:86`; plus `simulator/power_model.rs::random_voltage()` uses unseeded `rand::thread_rng()`. Classify legitimate live-loop entry points vs violations; thread the tick clock (and a seedable RNG) through the rest. `.claude/CLAUDE.md` documents the simulator/assets gap as R-24. | `VEN/src/entities/site_meter.rs`, `VEN/src/controller/openadr_interface.rs`, `VEN/src/simulator/`, `VEN/src/assets/` | Medium | Low | 🟡 | Medium — closes a real determinism/testability gap in the simulator/assets ring |
| R-25 | `CreateUserRequestBody` (HTTP DTO for POST /requests) is defined in domain-ring `controller/user_request.rs` and imported by services and routes. Move the DTO to routes/ (or an api-types module); the domain function takes domain params. | `VEN/src/controller/user_request.rs`, `VEN/src/routes/hems/`, `VEN/src/services/user_request.rs` | Small | Mechanical | 🟡 | Low — architecture correctness, no behavior change |
| R-26 | Six task files (poll_programs, poll_reports, poll_events, obligation, state_persist, progress_ticker) repeat the `tokio::time::interval` + `loop { tick().await; … }` scaffold; poll_programs vs poll_reports are 0.80 similar. Extract a shared periodic-spawn helper — also centralizes supervision. | `VEN/src/tasks/` | Small | Low | 🟡 | Low — maintenance/consistency only |
| R-29 | ~24 `unwrap()/expect()` calls in VEN production paths (milp_interactions.rs ×4, common/mod.rs ×4, services/planning.rs ×3, user_request.rs ×2, routes/hems/sessions.rs ×2, openadr_interface.rs ×2, heater/ev/battery_milp.rs ×2 each, sim_tick/tick.rs, services/hems.rs, milp_planner/inputs.rs ×1 each). Triage each: convert to Result or add a safety-justifying comment. | `VEN/src/` | Small | Low | 🟡 | Medium — real panic/crash risk if any untriaged call sees an unexpected input |
| R-31 | VTN BFF flattens every upstream error to `502 BAD_GATEWAY` with a stringified anyhow chain — VTN 4xx validation/conflict errors surface to the UI as 502. Propagate the upstream status class where known (current behaviour is pinned by a unit test in `error.rs`). | `VTN/bff/src/error.rs`, `VTN/bff/src/vtn_client.rs` | Small | Low | 🟡 | Medium — user/ops-facing error diagnosis quality |
| R-33 | UI test gaps: `VTN/ui/src/pages/Metrics.tsx` is the only untested page in either UI; `JsonDialog.tsx` is byte-identical in both UIs (50 lines — accept the copy with a twin-note header, or fold into a shared package if one materializes). | `VTN/ui/src/pages/Metrics.tsx`, `*/ui/src/components/JsonDialog.tsx` | Small | Low | 🟡 | Low — test-coverage gap |
| R-34 | Up to ~112 of 417 behave step definitions look unused (crude static match, false positives likely). Run `behave --dry-run` in the Pi4 test container for the authoritative list, then delete dead steps. | `tests/features/steps/` | Small | Low | 🟡 | Low — repo hygiene only |
| R-41 | Full-E2E-run degradation (observed 2026-07-17, 18 scenario failures): under the complete suite, VEN-1 progressively stops showing new (esp. targeted) events/programs and its report submissions fail, while the identical feature sequence passes in isolation. Correlates with a VTN warn-storm: before-feature cleanup (`environment.py`) hard-deletes programs/events while VEN caches still hold them, so auto/obligation reporters churn 409s (`report_report_name_uindex`) every tick. Investigate: does sim_tick/publish report churn delay event-cache refresh; add VEN cache invalidation for upstream-deleted objects; consider cleanup draining VEN caches. Note: this VTN fork maps FK violations to 409 too (openleadr-rs error.rs) — VEN error paths must always surface the problem body (done, `fix/report-upsert-409-transparency`). | `tests/features/environment.py`, `VEN/src/tasks/sim_tick/publish.rs`, `VEN/src/tasks/poll_events.rs` | Medium | Medium (E2E reliability) | 🟠 | Medium-High — blocks reliable full-suite E2E confidence, 18 scenario failures observed |
| R-42 | `reports_steps.py` submits reports with the fixed `reportName` "TELEMETRY_USAGE" (an OpenADR payload-type constant, not a name). `report_name` is globally unique on the VTN (`report_report_name_uindex`), so the fixed name collides across scenarios/clients and exercises the upsert path unintentionally. Switch to per-scenario unique names (needs sign-off: changes test fixtures). | `tests/features/steps/reports_steps.py` | Trivial | Low | 🟡 | Low — test-fixture correctness only |
| R-43 | `entities/history.rs::ReportSent` + `HistoryPort::append_report_sent` and the `GET /history/reports` route are fully wired end-to-end but no production call site ever invokes `append_report_sent` (only exercised in `history_store` unit tests) — found while implementing WP-T5 (`openspec/changes/wp-t5-report-submission-status/`). `GET /history/reports` therefore always returns empty. Wire it into the real report-submission call sites: `tasks/sim_tick/publish.rs::run_measurement_reports`, `services/obligation.rs`, and `routes/reports.rs`. | `VEN/src/tasks/sim_tick/publish.rs`, `VEN/src/services/obligation.rs`, `VEN/src/routes/reports.rs`, `VEN/src/history_store/mod.rs` | Small | Low (silent gap, no incorrect behaviour) | 🟡 | Medium — a whole user-visible endpoint (`/history/reports`) is currently silently empty |
| R-58 | Unconfirmed whether `PlanTrigger::CapacityChange`/`Alert` are actually wired to asset-level faults (thermal derate, BMS fault, breaker trip) rather than only tariff/VTN-sourced capacity changes — found during the deviation-scenarios analysis. Needs a verification pass: trace every call site that constructs these variants and confirm at least one covers an asset-originated fault, or extend one to. | `VEN/src/entities/asset.rs`, `VEN/src/services/planning.rs`, `VEN/src/assets/` | Small | Medium (unhandled asset fault may not trigger a replan) | 🟡 | Medium — an unwired asset fault could silently fail to trigger a needed replan |

## Low priority (🔵) — by topic

### Architecture & type placement

| ID | Description | Affected files | Effort | Risk | Gain |
|----|-------------|----------------|--------|------|------|
| R-08 | Replace `AssetConfig` manual dispatch enum (~9 methods × 5 variants) with `dyn Asset` or a macro forwarder — the one allowlisted file-size exception rides on this. Details: `docs/plans/refactoring_backlog.md`. | `VEN/src/assets/mod.rs` | Large | Serialisation risk | Medium — removes the last standing file-size-audit exception, real architecture cleanup |
| R-28 | `VEN/src/models.rs` is a 34-line grab-bag (`SensorSnapshot`/`SensorInput`) predating the ring layout. Fold into entities/ (or a simulator-owned module) and delete. | `VEN/src/models.rs` + 5 importers | Trivial | Mechanical | Low — repo hygiene only |
| R-39 | `state/mod.rs` mixes app wiring (`AppState`) with domain-ish value types (`EvSettings`, `HemsState`). Decide whether the two value types move to entities/ (as `AssetLedgerEntry` did) or stay — record the conclusion either way. | `VEN/src/state/mod.rs` | Trivial | Mechanical | Low — architecture clarity, no behavior change |
| R-47 | `AppState` keeps accumulating flat diagnostic fields (VTN connection status, storage-ok flag, per-task status map, etc.) added ad hoc per WP (T1/T3). No grouping/namespacing, so it will keep growing linearly with every future observability WP. Consider a `diagnostics: DiagnosticsState` sub-struct. Found during the WP-T1/T3/T5/T7 combined code review (2026-07-18). | `VEN/src/state/mod.rs` | Small | Low | Low-Medium — prevents compounding maintenance debt on every future observability WP |

### Code & repo hygiene

| ID | Description | Affected files | Effort | Risk | Gain |
|----|-------------|----------------|--------|------|------|
| R-27 | Hard-coded tuning constants: task intervals (`state_persist.rs:8` 15 s, `progress_ticker.rs:15` 1 s) and MILP solver tolerance `with_mip_gap(0.02)` (`solver_phase1.rs:151`). Name them and/or expose via config/PlannerParams. | tasks/, milp_planner/ | Trivial | Low | Low — config flexibility only |
| R-30 | 32 `console.log` calls in UI production code (`[VEN-UI]`-style debug logging). Strip or gate behind a debug flag/logger utility. | `VEN/ui/src/`, `VTN/ui/src/` | Trivial | Low | Low — hygiene/minor info-leak concern |
| R-36 | Lint/doc hygiene bundle: (a) module-wide `#![allow(dead_code)]` without justification in `entities/capacity.rs:5`, `entities/design_vocabulary.rs:7`; (b) 12 eslint warnings (exhaustive-deps, mixed exports); (c) eslint lints the generated `VTN/ui/coverage/` dir — add to ignore list; (d) `solve_ven3_heater_three_tier_zones_feasible` runs >60 s in debug `cargo test` — consider a smaller horizon variant; (e) "Stage 5 —" phase labels in `entities/user_request.rs` / `controller/user_request.rs` doc comments — drop the prefixes. | entities/, VEN/ui, VTN/ui, milp_planner/tests | Small | Low | Low — mostly cosmetic; (d) has a small developer-friction upside (faster test runs) |
| R-38 | (a) `VEN/Cargo.toml` carries blueprint-era comments (commented-out `openleadr-client` etc.); (b) verify `VTN/data/db` (runtime artifact) is gitignored. | `VEN/Cargo.toml`, `VTN/data/` | Trivial | Low | None — pure hygiene |
| R-44 | `/health` handler (`routes/system.rs::health`) deep-clones the full `VtnConnectionStatus` and active `Plan` on every poll just to read a couple of fields. Cheap today but grows with `Plan` size; consider a narrower state accessor. Found during the WP-T1/T3/T5/T7 combined code review (2026-07-18). | `VEN/src/routes/system.rs` | Trivial | Low | Low — cheap today, future-proofing only |
| R-45 | `routes/reports.rs::post_reports` and `put_report` duplicate the `submission_outcome()` call-and-record logic almost verbatim (WP-T5). Extract a shared helper. Found during the WP-T1/T3/T5/T7 combined code review (2026-07-18). | `VEN/src/routes/reports.rs` | Trivial | Low | Low — maintenance/consistency only |
| R-46 | Ring-buffer eviction (push-and-truncate-to-capacity) is duplicated near-identically in at least 3 places (`state/event_log.rs`, `state/report_submissions.rs`, and a third ring state module). Extract a shared `RingBuffer<T>` helper. Found during the WP-T1/T3/T5/T7 combined code review (2026-07-18). | `VEN/src/state/event_log.rs`, `VEN/src/state/report_submissions.rs` | Small | Low | Low — maintenance/consistency only |

### UI performance

| ID | Description | Affected files | Effort | Risk | Gain |
|----|-------------|----------------|--------|------|------|
| R-48 | `useAssetCapabilities`/`useAssetForecasts` (WP-T6) fire one HTTP request per asset in parallel rather than a single batched endpoint; fine at lab scale (few assets) but won't scale. Found during the WP-T1/T3/T5/T7 combined code review (2026-07-18). | `VEN/ui/src/api/hooks.ts` | Small | Low | Low — fine at current (lab) scale, future-proofing only |
| R-49 | `Reports.tsx::latestSubmissionFor` recomputes its scan over all submissions on every render (not memoized) — fine at current volumes, revisit if submission history grows large. Found during the WP-T1/T3/T5/T7 combined code review (2026-07-18). | `VEN/ui/src/pages/Reports.tsx` | Trivial | Low | Low — fine at current volumes, future-proofing only |

### Weather forecast plugin (docs/architecture/weather_forecast.md)

| ID | Description | Affected files | Effort | Risk | Gain |
|----|-------------|----------------|--------|------|------|
| R-52 | `MqttWeatherAdapter::is_alive()` (liveness/heartbeat check) and the cached `last_status` aren't surfaced anywhere yet (no `/health` integration, no metric). Currently `#[allow(dead_code)]`. | `VEN/src/weather.rs` | Trivial | Low | Medium — a live external feed's health isn't currently observable, contrary to this project's own `ui-transparency` principle |
| R-53 | Horizon/shading obstructions, the Perez/HDKR diffuse-sky model (vs. the current isotropic-on-zenith simplification), and module degradation over time are known, deliberately deferred accuracy gaps in `entities::solar`'s clear-sky transposition — see `docs/architecture/weather_forecast.md`. | `VEN/src/entities/solar.rs` | Medium | Low | Low-Medium — PV forecast accuracy improvement, deliberately deferred until it's the dominant error source |
| R-54 | The Mosquitto broker in this project's existing deployment (Pi4-Server) allows anonymous connections on its plaintext 1883 listener — anyone on the local network can publish to the weather topics today. Acceptable for a lab on a trusted LAN; revisit (password file already exists at `/srv/docker/mosquitto/config/pwfile`, unused) before any exposure beyond the local network. | Pi4-Server `mosquitto` deployment | Small | Low | Low today (LAN-only lab) — would become High if this deployment is ever network-exposed |
| R-55 | Snow-cover model's initial state (`PvSnowState` at the start of a forecast trajectory) only has the forecast-only fallback implemented — no cross-check against live PV telemetry deviation (`AssetState.power_deviation_kw`) to detect "actually covered right now" the way `docs/architecture/weather_forecast.md` describes as the preferred source. | `VEN/src/entities/pv_snow.rs` | Small | Low | Low — accuracy improvement for a specific, infrequent edge case |
| R-56 | No REST/BDD-executable end-to-end coverage for the weather MQTT path yet — `tests/features/weather_forecast.feature` is committed `@wip` (excluded from the default suite). No longer blocked (planner-input wiring landed); only needs Pi4/Docker access to un-`@wip` and run. | `tests/features/weather_forecast.feature` | Small | Low | Medium — a whole live external-data feature path currently has zero E2E coverage |

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
| R-22 | E2E scenario `ven_shiftable_lifecycle.feature:11` can time out under Pi4 load peaks (passes in isolation, 35–40 s). Tag it `@isolated` (move to `features/isolated/`) so it gets the load-settle wait like its siblings, or raise its poll timeout. | `tests/features/ven_shiftable_lifecycle.feature` | Trivial | Low (flake) | Medium — removes a confirmed, actively-observed E2E flake (trivial fix, same pattern as its siblings) |

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
like Medium-High/Low-Medium). No item is currently rated plain High, so this is the 10 items
rated Medium: R-08, R-21, R-22, R-24, R-29, R-31, R-43, R-52, R-56, R-58. Ordered by
dependency, not by ID — work top-to-bottom.

**Why this order:**

1. **R-22, R-52** first — both trivial, fully isolated (one `.feature` file; one weather
   module), no interaction with anything else on this list. Quick wins.
2. **R-56** next — builds on R-52: once liveness/`last_status` is surfaced, the weather E2E
   scenario being un-`@wip`'d can assert on it too, not just forecast values. Needs Pi4/Docker
   access either way.
3. **R-24** — threads an injectable clock through `assets/base_load.rs`, `assets/battery.rs`,
   `assets/ev.rs`, `assets/grid.rs` (additive parameter, no structural change). Land this
   *before* R-08 so the big dispatch-enum rewrite doesn't need a second pass to accommodate a
   clock parameter added afterward.
4. **R-08** — the large `AssetConfig` → `dyn Asset`/macro-forwarder refactor. Since this
   touches every asset variant's methods anyway, fold in R-29's heater/ev/battery_milp.rs
   `unwrap()`/`expect()` triage (~6 of its ~24 call sites) as part of the same pass instead of
   touching those files twice.
5. **R-58** — asset-level fault-trigger verification (`entities/asset.rs`,
   `services/planning.rs`, `assets/`). Soft-clustered after R-08/R-24 so it verifies/extends
   trigger call sites against the *settled* asset layer, not one mid-refactor.
6. **R-29 (remainder)** — the non-asset `unwrap()`/`expect()` call sites (`milp_interactions.rs`,
   `common/mod.rs`, `services/planning.rs`, `user_request.rs`, `routes/hems/sessions.rs`,
   `openadr_interface.rs`, `sim_tick/tick.rs`, `services/hems.rs`, `milp_planner/inputs.rs`) —
   the asset-file portion is already done in step 4.
7. **R-31, R-43** last — both fully independent of everything above (R-31 is VTN/bff-only;
   R-43 wires VEN report-submission call sites nothing else on this list touches). Order
   between them doesn't matter; listed in ID order.

Each item's tasks follow this repo's test-first convention (`test-first` rule, `CLAUDE.md`):
write the test, confirm it fails, implement until green. Full verification before considering
an item done: `wsl cargo test -j 2 -p ven-app` under `wsl_lock`, `cargo fmt --check`,
`cargo clippy --all-targets --all-features -- -D warnings`, `scripts/audit_file_sizes.py`;
update `docs/history/project_journal.md` and remove the item from this register once resolved.

### 1. R-22 — Tag the shiftable-lifecycle E2E flake `@isolated`

- [ ] 1.1 Move `Running shiftable load appears in GET /sim` from
      `tests/features/ven_shiftable_lifecycle.feature` into `tests/features/isolated/` (or
      raise its poll timeout, if moving is judged disruptive to the feature file's grouping).
- [ ] 1.2 Confirm it now gets the `entrypoint.sh` load-settle wait like its siblings.
- [ ] 1.3 Run the full E2E suite on Pi4 (`bash run_all_tests.sh --e2e`) at least twice under
      concurrent load to confirm the flake is gone; remove R-22 from this register.

### 2. R-52 — Surface `MqttWeatherAdapter::is_alive()`/`last_status`

- [ ] 2.1 Write a failing test asserting a `/health`-reachable (or dedicated) field reflects
      `is_alive()`'s current value.
- [ ] 2.2 Wire `is_alive()`/`last_status` into `routes/system.rs::health` (or a `/weather`
      liveness field, whichever fits the existing `ui-transparency` pattern better) and drop
      the `#[allow(dead_code)]`.
- [ ] 2.3 VEN UI: surface the liveness field somewhere on the Weather tab or diagnostics area,
      per this project's `ui-transparency` rule.
- [ ] 2.4 Full verification; remove R-52 from this register.

### 3. R-56 — Un-`@wip` the weather MQTT E2E coverage

- [ ] 3.1 Remove the `@wip` tag from `tests/features/weather_forecast.feature` and run it on
      Pi4 to see its current state (it was written before R-50's planner wiring landed).
- [ ] 3.2 Fix whatever the scenario reveals against the now-landed planner-input path.
- [ ] 3.3 If R-52 is already done, extend the scenario to assert on the new liveness field too.
- [ ] 3.4 Confirm it passes in the default (non-`@wip`) suite on Pi4; remove R-56 from this
      register.

### 4. R-24 — Thread an injectable clock through the remaining simulator/assets gaps

- [ ] 4.1 Classify each listed call site as a legitimate live-loop entry point (keep
      `Utc::now()`) vs. a genuine violation (needs threading) — `entities/site_meter.rs:49`,
      `controller/openadr_interface.rs:230`, `simulator/mod.rs:156,367`,
      `assets/base_load.rs:108`, `assets/battery.rs:142`, `assets/ev.rs:184`,
      `assets/grid.rs:86`.
- [ ] 4.2 Write a failing test for one violation (e.g. `assets/battery.rs:142`) that injects a
      fixed clock and asserts deterministic output across repeated calls.
- [ ] 4.3 Thread the tick clock through each confirmed violation; repeat 4.2 per site.
- [ ] 4.4 Replace `simulator/power_model.rs::random_voltage()`'s unseeded `rand::thread_rng()`
      with a seedable RNG threaded the same way.
- [ ] 4.5 Full verification; remove R-24 from this register.

### 5. R-08 — Replace the `AssetConfig` manual dispatch enum

- [ ] 5.1 Design pass: `dyn Asset` trait object vs. a macro forwarder — see
      `docs/plans/refactoring_backlog.md` for prior diagnostics; confirm the chosen approach
      resolves the allowlisted file-size exception in `scripts/audit_file_sizes.py`.
- [ ] 5.2 Write/port tests asserting each of the 5 variants' 9 methods behave identically
      before and after dispatch mechanism changes (characterization tests if none exist yet).
- [ ] 5.3 Implement the new dispatch mechanism in `VEN/src/assets/mod.rs`.
- [ ] 5.4 While touching each variant's methods, fold in R-29's heater/ev/battery_milp.rs
      `unwrap()`/`expect()` triage (~6 call sites) — convert to `Result` or add a
      safety-justifying comment, per R-29's own fix note.
- [ ] 5.5 Remove `VEN/src/assets/mod.rs` from `scripts/audit_file_sizes.py`'s allowlist.
- [ ] 5.6 Full verification; remove R-08 from this register.

### 6. R-58 — Verify `PlanTrigger::CapacityChange`/`Alert` cover asset-level faults

- [ ] 6.1 Trace every call site constructing `PlanTrigger::CapacityChange`/`Alert` in
      `services/planning.rs` and `assets/`.
- [ ] 6.2 Confirm at least one covers an asset-originated fault (thermal derate, BMS fault,
      breaker trip), not only tariff/VTN-sourced capacity changes.
- [ ] 6.3 If none do, write a failing test for the missing case (e.g. a simulated thermal
      derate should emit `CapacityChange`) and extend the relevant call site.
- [ ] 6.4 Full verification; remove R-58 from this register.

### 7. R-29 — Triage the remaining `unwrap()`/`expect()` call sites

- [ ] 7.1 List the non-asset call sites (asset-file ones already handled in step 5.4):
      `milp_interactions.rs` ×4, `common/mod.rs` ×4, `services/planning.rs` ×3,
      `user_request.rs` ×2, `routes/hems/sessions.rs` ×2, `openadr_interface.rs` ×2,
      `sim_tick/tick.rs` ×1, `services/hems.rs` ×1, `milp_planner/inputs.rs` ×1.
- [ ] 7.2 For each: convert to `Result` if the panic path is reachable with attacker/user-
      controlled or otherwise fallible input; otherwise add a one-line safety-justifying
      comment explaining why it can't panic in practice.
- [ ] 7.3 Full verification; remove R-29 from this register.

### 8. R-31 — Propagate VTN BFF upstream error status class

- [ ] 8.1 Write a failing unit test in `VTN/bff/src/error.rs` asserting a VTN 4xx
      validation/conflict error surfaces as its own status class, not a flattened 502.
- [ ] 8.2 Implement status-class propagation in `error.rs`/`vtn_client.rs` where the upstream
      status is known; keep 502 only for genuine gateway/connectivity failures.
- [ ] 8.3 Update the existing pinning unit test in `error.rs` to match the new behavior.
- [ ] 8.4 Full verification; remove R-31 from this register.

### 9. R-43 — Wire `append_report_sent` into real report-submission call sites

- [ ] 9.1 Write a failing integration test: submitting a report via the real call path results
      in a row visible through `GET /history/reports`.
- [ ] 9.2 Call `HistoryPort::append_report_sent` from `tasks/sim_tick/publish.rs::run_measurement_reports`.
- [ ] 9.3 Call it from `services/obligation.rs` and `routes/reports.rs`'s submission paths too.
- [ ] 9.4 Confirm `GET /history/reports` returns non-empty results after a real submission
      (BDD scenario, if practical, per this project's E2E conventions).
- [ ] 9.5 Full verification; remove R-43 from this register.
