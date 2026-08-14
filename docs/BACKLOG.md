## REQUIREMENTS.md and VEN_ARCHITECTURE.md: Requirements Gap Backlog

Open items only — resolved entries are removed (their resolution notes live in
`docs/history/project_journal.md` and git history). IDs are stable and never
reused; gaps in the numbering are removed items. BL-14 through BL-29 originate
from the dead-code vocabulary review (types quarantined in
`entities/design_vocabulary.rs`, not deleted). This file still needs a proper
re-sort/prioritization pass to decide actual implementation order.

---

## User-Value View

Same items as below, regrouped by *who benefits and how* rather than by where
the gap sits in the code. Effort mirrors each item's own Complexity field
(S/M/L). Risk is regression/architectural risk of the change itself, not of
leaving it undone. Gain is the value delivered if built — independent of
effort/risk — mirroring each item's own Gain field below (High/Medium/Low/None).

### VEN user (site operator) — money saved

| ID | What the user gets | Gain | Effort | Risk |
|---|---|---|---|---|
| [BL-11](#bl-11-time-weighted-tariff-averaging-for-planner-slot-costing) | Slightly cheaper/more accurate plans for slots that straddle a tariff-rate boundary | Low | S | Low — isolated calc on existing `TimeSeries` |
| [BL-13](#bl-13-early-firm-up-heuristic) | Fewer noisy replans under flat-rate tariffs (plan feels more stable) | Low | S | Low — statistical check + reclassification |

### VEN user (site operator) — forecast accuracy

| ID | What the user gets | Gain | Effort | Risk |
|---|---|---|---|---|
| [BL-17](#bl-17-externaldatasource--grid-co2-intensity-forecast-ingestion) | Grid-CO2-aware planning from a real CO2-intensity forecast (weather/irradiance ingestion for PV is already implemented) | Medium | L | Medium–High — third-party API dependency, staleness/failure handling, provider not yet chosen |

### VEN user (site operator) — comfort, control & trust

| ID | What the user gets | Gain | Effort | Risk |
|---|---|---|---|---|
| [BL-27](#bl-27-poweradjustability--powerrange--device-control-mode-classification) | UI controls (e.g. a stepped EV charger) snap to real device levels instead of rendering a misleading continuous slider | Medium | M | Low–Medium — every asset's `capability()` impl must report it |
| [BL-39](#bl-39-per-session-accumulated-cost-accounting-real-budget-bar) | Budget bar on the session board shows real money spent so far instead of a plan-time estimate | Medium | M | Medium — new accounting invariant in monitor/ledger or history-store, session attribution |
| [BL-18](#bl-18-assetflexibility--real-time-per-asset-flexibility-snapshot) | A live "how much can this device flex right now" widget, per asset instead of whole-site | Low-Medium | M (scope TBD) | Low — but needs a design decision (superseded by `FlexibilityEnvelope`?) before scoping |
| [BL-38](#bl-38-planner-tab-layout--userdiagnostic-split-and-matrix-slottrace-linking) | Planner tab reads cleanly for operators (user zone on top) and debugs faster (click a slot → see its trace) | Low-Medium | S (layout) / M (slot→trace) | Low — UI-only |
| [BL-35](#bl-35-notification-producers-for-tier-fallback--deadline-at-risk--packet-abandoned) | Gets warned *before* a tier fallback / missed deadline / abandoned session, not after | Low | S (once BL-09 lands) | Low — blocked on BL-09's tier machinery existing |
| [GB-09](#general-backlog) | Fleet operators get a per-profile poll-interval override | Low | S | Low — current jitter already covers the motivating case, so low urgency |

### VTN user (aggregator / program operator)

| ID | What the user gets | Gain | Effort | Risk |
|---|---|---|---|---|
| [BL-24](#bl-24-oadrprogramconfigoadreventcacheoadrcapacityrequest-wiring) | Would let the VTN request/receive capacity reservations from the VEN — no such workflow exists today | Low | S if removed / unknown if built | Low — no consumer yet; recommend leaving parked until a real feature needs it |

### No direct user value — internal cleanup/consistency (do opportunistically, don't prioritize)

| ID | Note | Gain |
|---|---|---|
| [BL-21](#bl-21-reconcile-duplicate-thermalmodelparams) | Duplicate dead struct, superseded by `assets/heater.rs`'s own | None |
| [BL-23](#bl-23-hvacservice--route-wiring-or-removal-of-the-unused-impl) | Consistency-only decision, no behavior change either way | None |
| [BL-26](#bl-26-assetstate-entities--resolve-the-name-collision-with-the-live-assetsassetstate) | Dead type shadowing a live one's name | None |
| [BL-29](#bl-29-flexibilitydirection-ratetype-rateunit--narrow-supporting-enums) | No standalone value — fold into whichever future feature needs each enum | None |
| [GB-11](#general-backlog) | Process/docs alignment items, not user-facing | Low |
| [GB-16](#general-backlog) | `npm audit` findings in `VEN/ui` (brace-expansion, react-router) — dependency hygiene, not user-facing | Low-Medium |

---

### BL-11: Time-weighted tariff averaging for planner slot costing
**Req:** VEN_ARCHITECTURE §5.3
**Problem:** Planner evaluates tariff at `slot.start` only. A 5-min slot straddling a tariff boundary (e.g., €0.20 → €0.15 at 10:57) uses only the first tariff, ignoring the 3 min at the cheaper rate.
**Fix:** Replace `tariff_at(slot.start)` with `Σ(tariff_i × overlap(slot, interval_i)) / slot.duration` using the existing `TimeSeries` abstraction. For capacity: `min(capacity_i for all overlapping intervals)`.
**Gain:** Low — the mispriced window is at most one slot per tariff boundary; small, rare savings.
**Complexity:** Small–Medium (2–3 hours). Use existing TimeSeries infrastructure.
**Verify:** Unit test: 10-min slot spanning tariff boundary at minute 7 → weighted average matches `(7*0.20 + 3*0.15)/10 = 0.185`.

---

### BL-13: Early firm-up heuristic
**Req:** VEN_ARCHITECTURE §2.3
**Problem:** Spec says if rate variance across FLEXIBLE window is < 10% (flat rate), FLEXIBLE slots may firm up early. Code comment at `planner.rs:271` acknowledges this but it's not implemented.
**Fix:** After Phase 7, compute variance of tariff across all FLEXIBLE slots. If coefficient of variation < 0.10, reclassify FLEXIBLE → FIRM and re-run allocation (Phases 2–5) for those slots.
**Gain:** Low — perceived plan stability under flat tariffs, no cost or capability change.
**Complexity:** Small (1–2 hours). Statistical check + slot reclassification.
**Verify:** Unit test: flat-rate tariff (all €0.15) → all slots classified FIRM. Variable tariff (€0.10–€0.30) → FLEXIBLE slots remain FLEXIBLE.

---

### BL-17: ExternalDataSource — grid CO2-intensity forecast ingestion
**Req:** entities/design_vocabulary.rs §2.11 (`ExternalDataSource`, `ExternalDataSourceType`, `ExternalDataFetchStatus`)
**Problem:** Weather/irradiation ingestion for PV forecasting is implemented (`docs/architecture/weather_forecast.md`, an MQTT-pushed feed rather than the originally-sketched poll loop). Grid CO2-intensity forecasting is not: no code path polls or receives a CO2-intensity feed, so the planner has no way to prefer low-carbon slots beyond whatever GHG values arrive on an event.
**Fix:** Implement an `ExternalDataSource`/`ExternalDataPort` poll loop for a CO2-intensity provider, caching the last successful response and tracking `ExternalDataFetchStatus`; feed results into planning as a new forecast/cost input alongside tariffs.
**Gain:** Medium — genuine carbon-aware planning capability, but realized value is speculative until a provider is actually chosen and integrated.
**Complexity:** Large — third-party API dependency (no evaluated free-tier provider yet, per `docs/plans/roadmap/phase-5-forecast-and-baseline.md`), staleness/failure handling.
**Verify:** TBD — depends on the chosen external API; at minimum, a fake-server integration test asserting `fetch_status` transitions correctly on success/failure/timeout.

---

### BL-18: AssetFlexibility — real-time per-asset flexibility snapshot
**Req:** entities/design_vocabulary.rs §3.5 (`AssetFlexibility`)
**Problem:** `AssetFlexibility` sketches an on-demand "how much can this asset flex right now" snapshot (`can_increase/decrease_consumption/production_kw`), computed per-asset rather than for the whole site. This is distinct from `FlexibilityEnvelope`, which is planner-produced, horizon-wide, and already reported to the VTN — `AssetFlexibility` would be the instantaneous, single-asset building block.
**Fix:** Decide first whether this is still wanted as a separate real-time endpoint (e.g. for a live UI widget) or fully superseded by `FlexibilityEnvelope`; if wanted, compute it on demand from each asset's current state and `PowerRange`/`ThermalModelParams` limits, no persistence needed.
**Gain:** Low-Medium — a real-time per-asset flex widget is a nice-to-have, but its incremental value over the already-shipped `FlexibilityEnvelope` is unclear until the design question is resolved.
**Complexity:** Medium, but scope depends on the design decision above — resolve that first.
**Verify:** TBD pending scope decision.

---

### BL-21: Reconcile duplicate ThermalModelParams
**Req:** entities/design_vocabulary.rs §3.1.1 (`ThermalModelParams`)
**Problem:** `entities/design_vocabulary.rs::ThermalModelParams` (thermal mass, insulation factor, min/max temperature) has zero references anywhere — `assets/heater.rs` already has its own, separately-defined thermal parameter struct that is the one actually wired into the heater's MILP model. This one is a leftover duplicate from the original spec-vocabulary pass, not a distinct future feature.
**Fix:** Confirm `assets/heater.rs`'s struct is a full superset; if so, delete `entities/design_vocabulary.rs::ThermalModelParams` and its now-unused field on the (already-quarantined) `AssetProfile`. If it's missing fields the entities version has, fold those into the heater-side struct instead of keeping two.
**Gain:** None (cleanup only) — dead code removal, no behavior or user-facing change.
**Complexity:** Small. Comparison + deletion or field merge.
**Verify:** `cargo build` clean after deletion; heater MILP tests unaffected.

---

### BL-23: `HvacService` — route wiring or removal of the unused impl
**Req:** `services/hems.rs` (`HvacService`)
**Problem:** `EvSessionService` is the live pattern for session lifecycle; `HvacService` sketches the same shape for heater targets, but `post_heater_target` sets the target directly instead of going through it — so `HvacService`'s methods are never called.
**Fix:** Either route `post_heater_target` through `HvacService` for consistency with the EV path, or fold whatever `HvacService` was meant to add into the existing direct path and delete the empty shell.
**Gain:** None (cleanup only) — consistency decision, no behavior change either way.
**Complexity:** Small — this is a consistency decision, not new functionality.
**Verify:** `cargo build` clean; existing heater-target route tests unaffected either way.

---

### BL-24: `OadrProgramConfig`/`OadrEventCache`/`OadrCapacityRequest` wiring
**Req:** `entities/capacity.rs`
**Problem:** Three unwired sketches in a file that otherwise holds live types (`OadrCapacityState`, `OadrReportObligation`). DISPATCH_SETPOINT handling landed as typed `DispatchWindow` state (matching the alert/SIMPLE pattern), so `OadrEventCache`'s anticipated consumer no longer exists; `OadrProgramConfig` and `OadrCapacityRequest` have no consumer at all — no code path builds or sends a capacity reservation request to the VTN in this shape.
**Fix:** Consider removal for all three next time this file is triaged; for `OadrProgramConfig`/`OadrCapacityRequest`, no dependent feature identified yet — lowest priority of this group until one exists.
**Gain:** Low — no consumer feature exists yet; value only materializes if/when a capacity-reservation workflow is actually needed.
**Complexity:** Small (removal) — TBD if a consuming feature appears instead.
**Verify:** Tied to whichever consuming feature lands first, or `cargo build` clean after removal.

---

### BL-26: `AssetState` (entities) — resolve the name collision with the live `assets::AssetState`
**Req:** `entities/design_vocabulary.rs` (`AssetState`)
**Problem:** A second unreferenced type sharing a name with a real, heavily-used live type (`assets::mod::AssetState`, the per-device-kind enum driving `step`/`capability`). The entities-level one (device status snapshot: commanded/actual power, responsiveness, SoC, temperature, connection) has no consumer and predates the real `Asset` trait design.
**Fix:** Most likely resolution: this was superseded by the live `assets::AssetState` + `AssetCapability` combination and should eventually be deleted rather than implemented — but that's a re-confirmation, not assumed here. If any of its fields (e.g. `last_confirmed_response`, `is_available`) represent monitoring data genuinely missing from the live type, fold those in instead.
**Gain:** None (cleanup only) — dead type shadowing a live one's name, no behavior change.
**Complexity:** Small — comparison against the live type, then either deletion or a small field migration.
**Verify:** `cargo build` clean; no behavior change (nothing references it today).

---

### BL-27: `PowerAdjustability` + `PowerRange` — device control-mode classification
**Req:** `entities/design_vocabulary.rs` (`PowerAdjustability`, `PowerRange`)
**Problem:** Not a duplicate of the live `AssetCapability` (`assets/mod.rs`) as might be assumed at a glance — `AssetCapability` only carries instantaneous `max_import_kw`/`max_export_kw`, no discrete-step list and no semantic classification of *how* an asset can be controlled (on/off vs. stepped vs. continuously variable vs. curtail-only vs. advisory-only). `PowerAdjustability`/`PowerRange.power_steps_kw` sketch a real, currently-missing capability: exposing control-mode metadata (e.g. to the UI, so a stepped charger's slider snaps to real levels instead of rendering continuous).
**Fix:** If wanted, add `adjustability: PowerAdjustability` and `power_steps_kw: Vec<f64>` to the live `ControlDescriptor`/`AssetCapability` path (`assets/mod.rs`) rather than reviving these as separate entities-level types.
**Gain:** Medium — fixes a real UI correctness issue (misleading continuous sliders on stepped devices), improving user trust in the controls.
**Complexity:** Medium — touches every asset's `capability()` implementation to report ranked/stepped power correctly.
**Verify:** UI test: a stepped-charger control descriptor exposes discrete levels; a stepless one doesn't.

---

### BL-29: `FlexibilityDirection`, `RateType`, `RateUnit` — narrow supporting enums
**Req:** `entities/design_vocabulary.rs`
**Problem:** Three small enums with no current consumer. `RateUnit` overlaps with the live `RateUnit`-shaped fields already handled ad hoc as bare `f64`/currency-implicit values in `TariffSnapshot`; `RateType` (per-kWh vs. per-kW) and `FlexibilityDirection` (import/export) are classification vocabulary for capacity-rate handling and capacity-request direction respectively — relevant once envelope reporting is extended or BL-24's `OadrCapacityRequest` is actually implemented.
**Fix:** Don't implement standalone — fold each into whichever feature actually needs it when that feature is built: `RateType`/`RateUnit` into a future multi-currency/multi-unit tariff handling pass (no BL item yet — add one if/when multi-currency support is requested); `FlexibilityDirection` into envelope-report-building work.
**Gain:** None standalone — no value until folded into a parent feature that needs one of these enums.
**Complexity:** N/A standalone — tracked here only so they're not forgotten, not as independent work items.
**Verify:** N/A until folded into a parent feature.

---

### BL-35: Notification producers for tier fallback / deadline-at-risk / packet abandoned
**Req:** `entities/design_vocabulary.rs` (`UserNotificationSeverity` doc comments)
**Problem:** The notification feed (ring + SSE + persistence, `services/notify.rs`) carries grid-emergency, VTN-reachability, and adopted-plan-warning producers. The remaining producers named by the severity enum's own doc comments — tier fallback, deadline approaching, packet abandoned — have nothing to hook onto because the Stage-5 tier/SIMPLE-level-fallback machinery doesn't exist yet. Not unblocked by BL-09 (peak-demand penalty threshold, resolved): that shipped a lightweight, per-solve constraint with no persisted tier/billing-period state, deliberately not the stateful tracker this item's tier-fallback producer would hook into — see `docs/history/project_journal.md` (search "BL-09") for that scope decision.
**Fix:** Wire these producers when the tier/SIMPLE-level-fallback machinery lands; each should emit through the existing `Notifier` with a stable dedup text.
**Gain:** Low — incremental notification coverage, and blocked on that machinery existing first.
**Complexity:** Small once the producing machinery exists.
**Verify:** Test per producer: the triggering condition emits exactly one notification of the expected severity.

---

### BL-38: Planner tab layout — user/diagnostic split and matrix-slot→trace linking
**Req:** `VEN/ui/src/pages/Planner.tsx`; wiki `queries/planner-tab-purpose.md`
**Problem:** User-facing elements (objective, power stack, session progress) are interleaved with diagnostic surfaces (trigger timeline, decision matrix, trace table), so the operator persona sees noise and the debugging persona scrolls past controls. Additionally, answering "what happened in slot 14:35?" requires manually cross-reading the decision matrix and the trace table.
**Fix:** (a) Reorder into a user zone on top (objective + legend, power stack, session progress) and a diagnostics zone below a divider, collapsed by default like the existing trace accordion. (b) Make decision-matrix slots clickable, filtering the TraceTable to entries relevant to that slot's window.
**Gain:** Low-Medium — pure UX clarity improvement for two personas; no behavior or data change.
**Complexity:** Small for (a) — pure reordering/collapse; Medium for (b) — needs a slot↔trace-entry time-window correlation and filter state.
**Verify:** (a) UI test: diagnostics sections render collapsed by default, user zone above the divider. (b) UI test: clicking a matrix slot filters the trace table to entries whose timestamp falls in that slot.

---

### BL-39: Per-session accumulated-cost accounting (real budget bar)
**Req:** `VEN/ui/src/components/sessions/SessionProgressBoard.tsx` (BudgetLine); `VEN/src/controller/monitor.rs` (AssetLedger); `docs/reference/TECHNICAL_DEBTS.md` R-24 (ledger clock/persistence)
**Problem:** The session board's budget bar compares the user's budget against `estimated_cost_eur` (a plan-time estimate, labeled "est.") because no per-session accumulated cost exists anywhere: the `AssetLedger` accumulates per asset since startup with no session attribution, resets on restart, and the plan envelope's `budget_remaining_eur` is a placeholder. Spun off from the BL-36 resolution — the SessionProgressBoard rebuild deliberately excluded this.
**Fix:** Either extend the monitor ledger with session-scoped accumulation (attribute each tick's asset cost to the active session id), or derive it on demand from the history store windowed on `session.created_at` × recorded tariffs. Decide only if enforcement-grade budget tracking is actually needed; the estimate may be good enough.
**Gain:** Medium — the budget bar's current "est." label undermines user trust in the number; real accounting would let the session board be trusted at a glance.
**Complexity:** Medium — a new accounting invariant plus persistence questions (interacts with R-24).
**Verify:** Unit test: a session accumulating N ticks at known power/tariff reports Σ(power × Δt × tariff); UI budget bar switches from "est." to actual once the field exists.

---

## General Backlog

| ID | Item | Gain | Priority |
|---|---|---|---|
| GB-09 | Per-profile VEN poll interval override. The original motivation ("N VENs don't poll in lockstep") is met via the one-time `POLL_STARTUP_JITTER_S` stagger; a per-profile interval override remains unbuilt and nothing currently needs it | Low | Low |
| GB-11 | Remaining AI-SW-Development alignment items (from the retired root alignment-plan.md, Pass 3): backlog-handling + tool-installation + archive-folder notes in CLAUDE.md; USER_STORIES.md; RISK_ANALYSIS.md; PROMPT_LIBRARY.md; changelog decision (journal-as-changelog note); security-review cadence; automated code-review hook; file-header descriptions on key VEN modules | Low | Low |
| GB-12 | BDD scenario for `Plan.solve_status == Infeasible` on `/plan`/`/plan/events`. Unit-level coverage exists (`run_planner_infeasible_constraints_fallback_no_panic` plus new solve_status assertions); no BDD scenario forces an infeasible solve today because doing so needs a fixture heavier than the existing `InfeasibleBatCtx` test double, which isn't exposed at the BDD/E2E layer | Low | Low |
| GB-13 | Wire the Event Log's SSE stream (`GET /events/log/events`) into the UI — `useEventLog()` (`VEN/ui/src/api/hooks.ts`) still polls every 10s; the backend route works but nothing consumes it | Low-Medium — removes needless polling overhead, minor UX win | Low |
| GB-14 | Create a dedicated SSH key pair for the `Node1` host instead of falling back to the default `id_rsa` — checked 2026-07-31: `~/.ssh/config`'s `Node1` entry has no `IdentityFile`, so it authenticates with whatever default identity (`id_rsa`) the server happens to accept, unlike `Node2` which already has its own pinned identity file | Low — security/hygiene hardening, no functional gap | Low |
| GB-15 | The VTN's `/vens` list has no `ven-1` entry — only `ven-1-name`, an old provisioning typo predating the Node2 fleet work (found 2026-08-02 while running `seed_vtn.py`). The stale name also breaks the "Summer Peak DR" demo program's target update (targets `["ven-2", "ven-1-name"]`, 400 on re-seed). Rename the VEN entity to `ven-1` and fix the program's targets | Low — cosmetic/demo-data correctness, `ven-1` itself works fine under its real `CLIENT_ID` | Low |
| GB-16 | `npm audit` in `VEN/ui` (checked 2026-08-03, pre-existing — not introduced by any recent change): `brace-expansion` (high, DoS via exponential/unbounded expansion, transitive via eslint — build-time only, not shipped) and `react-router`/`react-router-dom` 6.0.0–7.17.0 (moderate, open-redirect + arbitrary-constructor-injection CVEs — a real runtime dependency). Both have fixes via `npm audit fix`; not applied here since it's outside this session's scope — run it and re-test the UI suite before the next release | Low-Medium — react-router is runtime-shipped; brace-expansion is dev-only | Low — `npm audit fix` is typically a patch/minor bump, but re-run the full UI suite after |
| GB-20 | `fleet.sh down --purge` fails to delete each fleet VEN's data files (`history.sqlite`, `state.json`, `sim_state.json`) with `Permission denied` — Docker Compose creates the bind-mounted `VEN/data/fleet-ven-*` directories as `root:root`, but the files inside end up owned by the container's uid 2000 (once a chown fix or fresh boot is in place), and the purge script runs as the regular host user. Containers are still removed cleanly; only the data-file cleanup silently fails, requiring a manual `sudo rm -rf` (found 2026-08-10, see project_journal.md persona-re-run entry) | Low — cosmetic/hygiene, doesn't block anything functionally | Low — `fleet.sh`'s purge path needs either a `sudo rm` or to run as the same uid that owns the files |
| GB-22 | `features/phase_a_physics.feature`'s "Battery at full SoC reports zero import capability" scenario is flaky under host resource contention: its `poll_until(.../capability/battery max_import_kw==0.0)` has a 120s timeout with no `@isolated` tag, so when it runs during the E2E main pass (not the load-gated `@isolated` pass) under heavy host load (observed load 7.3–7.7 on Node2 during a `run_all_tests.sh --e2e` run, 2026-08-11) it can time out even though the underlying logic (`assets/battery.rs:90`, `if state.soc >= 1.0 { 0.0 }`) is correct — confirmed by re-running the exact same scenario in isolation under load ~1.8, where it passed in 0.06s. Not a real bug; a test-robustness gap shared with the `@isolated`-tagged scenarios' own stated rationale (`tests/entrypoint.sh`'s `wait_for_load_to_settle` comment). A second instance of the same class found 2026-08-12 (`report-obligation-lifecycle` E2E verification, `run_all_tests.sh --e2e` on Node2): `features/controller/04_navigation.feature:24` ("Right section starts collapsed and can be expanded then collapsed", `@ven-ui`, no `@isolated`) failed with `[HTTP 502] .../health` and `[HTTP 404] .../capability/site-residual` browser console errors during a main-pass run at observed load 5–6; the identical scenario passed cleanly in an earlier run the same day at low load, and passed again unmodified as part of that run's own `@isolated` pass (load ~1.8) minutes later. Same root cause as the battery scenario — an un-isolated scenario racing real backend calls during host contention — just manifesting as a UI-render assertion instead of a `poll_until` timeout. It recurred a third time 2026-08-12 (`baseline-override-devices-ui` E2E verification, same scenario, same signature) — moved to `features/isolated/controller_navigation.feature` and tagged `@isolated`, closing this specific instance. The battery scenario (`phase_a_physics.feature`) remains un-isolated — this item stays open for that one and as a reminder to audit other `@ven-ui`/browser scenarios for the same gap | Low — false-failure risk only under host contention, not a functional defect | Low — either tag affected scenarios `@isolated` like their siblings, or raise timeout/retry margins; one instance fixed, the pattern likely recurs elsewhere |
| GB-24 | Node2's `run_all_tests.sh --e2e` suite is unreliable when it coincides with load from the permanently-resident 10-VEN fleet experiment (`worktrees/fleet-13-ven-experiment`, `VEN/scale_out/node2/`) also running on the same host — found 2026-08-13 (`baseline-override-devices-ui` E2E verification). One run coincided with all 10 fleet VEN containers restarting simultaneously (`docker events` showed synchronized `exec_die` at 01:50), which broke DNS resolution inside the test stack's own docker network and cascaded into 195/269 scenario failures (`Failed to resolve 'test-vtn'`). A clean retry ~15 min later still ran degraded — 40/270 scenarios failed, every single one the same `poll_until(VEN sees a just-created VTN object)` timeout, spread across completely unrelated features (alerts, reports, rate system, resilience, UI use cases) — while the change's own 3 new BDD scenarios and all 534 local UI unit tests passed cleanly. Node2 has only 3.7 GiB RAM total; 10 idle-but-resident fleet VEN containers plus a full E2E stack (VTN/DB/BFF/5 test VENs/UI/headless browser) appears to be structurally tight capacity, not just a one-off contention spike | Medium — repeatedly wastes significant session time chasing failures that are environmental, not code defects, and risks a real regression being misread as "just Node2 being flaky" or vice versa | Medium — options: give the E2E suite a pre-flight capacity/health check that fails fast with a clear message instead of running 40+ min degraded; move the fleet experiment off Node2 (or make it stoppable/pausable before E2E runs, coordinated via the existing docker_host_lock.sh mechanism); or accept Node1 as the required host for E2E while the fleet experiment lives on Node2 |
| GB-31 | GB-25's persisted `Plan.mip_gap_target` (and the `plan_history` row's copy of it) is a proxy only — the solver's *configured* MIP-gap tolerance (`controller::milp_planner::types::MIP_GAP_TARGET`, `0.02`), not the *achieved* gap on any given solve. `good_lp`/`highs` expose no achieved-gap query today, so a plan that solved to a much tighter gap than the 2% target looks identical, in this field, to one that just barely made the cutoff. Explicit scope decision made during GB-25's implementation, not an oversight | Low — diagnostic-quality gap, not a functional defect; the configured target still tells an operator "this is at least as good as X%" | Low-Medium — needs either a `good_lp`/`highs` upstream API for the achieved gap, or computing it manually from the solver's best-bound and incumbent objective values if HiGHS exposes those separately |

---

## Dependency Vulnerabilities — 2026-07-16

> Re-run `cargo audit` and `npm audit` before each release and update this section.

Current state after `cargo update` (VEN, VTN/bff) and the vite 8 / vitest 4 toolchain
upgrade + `npm audit fix` (both UIs), all done 2026-07-16 on `fix/review-c3-code`:

| Component | cargo/npm audit result |
|-----------|------------------------|
| VEN (Rust) | **0 vulnerabilities, 0 warnings** (267 crates) |
| VTN/bff (Rust) | 1 advisory — see below (315 crates) |
| VEN/ui (npm) | **0 vulnerabilities** |
| VTN/ui (npm) | **0 vulnerabilities** |

### VTN/bff — RUSTSEC-2023-0071 (`rsa` 0.9.x, Marvin timing side-channel, medium 5.9)

Lockfile-only false positive: `rsa` enters `Cargo.lock` via `sqlx-mysql`, an *optional*
sqlx driver that is never enabled (the BFF pins
`sqlx = { default-features = false, features = ["postgres", ...] }`) and never compiled —
`cargo tree -i rsa` resolves to nothing. Cargo records optional dependencies for all
features in the lockfile, and `cargo audit` scans the lockfile, hence the hit. No fixed
`rsa` release exists upstream. Accept and re-check on sqlx upgrades.

**Risk context:** Lab/Node1 deployment — not internet-exposed. Re-run both audits before
any internet-exposed deployment.

---
