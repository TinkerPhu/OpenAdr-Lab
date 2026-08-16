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
| [BL-18](#bl-18-assetflexibility--real-time-per-asset-flexibility-snapshot) | A live "how much can this device flex right now" widget, per asset instead of whole-site | Low-Medium | M (scope TBD) | Low — but needs a design decision (superseded by `FlexibilityEnvelope`?) before scoping |
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
| [GB-34](#general-backlog) | react-router v6→v7 migration (security-driven, no new user-facing capability) | Low |

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

## General Backlog

| ID | Item | Gain | Priority |
|---|---|---|---|
| GB-09 | Per-profile VEN poll interval override. The original motivation ("N VENs don't poll in lockstep") is met via the one-time `POLL_STARTUP_JITTER_S` stagger; a per-profile interval override remains unbuilt and nothing currently needs it | Low | Low |
| GB-11 | Remaining AI-SW-Development alignment items (from the retired root alignment-plan.md, Pass 3): backlog-handling + tool-installation + archive-folder notes in CLAUDE.md; USER_STORIES.md; RISK_ANALYSIS.md; PROMPT_LIBRARY.md; changelog decision (journal-as-changelog note); security-review cadence; automated code-review hook; file-header descriptions on key VEN modules | Low | Low |
| GB-12 | BDD scenario for `Plan.solve_status == Infeasible` on `/plan`/`/plan/events`. Unit-level coverage exists (`run_planner_infeasible_constraints_fallback_no_panic` plus new solve_status assertions); no BDD scenario forces an infeasible solve today because doing so needs a fixture heavier than the existing `InfeasibleBatCtx` test double, which isn't exposed at the BDD/E2E layer | Low | Low |
| GB-14 | Create a dedicated SSH key pair for the `Node1` host instead of falling back to the default `id_rsa` — checked 2026-07-31: `~/.ssh/config`'s `Node1` entry has no `IdentityFile`, so it authenticates with whatever default identity (`id_rsa`) the server happens to accept, unlike `Node2` which already has its own pinned identity file | Low — security/hygiene hardening, no functional gap | Low |
| GB-15 | The VTN's `/vens` list has no `ven-1` entry — only `ven-1-name`, an old provisioning typo predating the Node2 fleet work (found 2026-08-02 while running `seed_vtn.py`). The stale name also breaks the "Summer Peak DR" demo program's target update (targets `["ven-2", "ven-1-name"]`, 400 on re-seed). Rename the VEN entity to `ven-1` and fix the program's targets | Low — cosmetic/demo-data correctness, `ven-1` itself works fine under its real `CLIENT_ID` | Low |
| GB-31 | GB-25's persisted `Plan.mip_gap_target` (and the `plan_history` row's copy of it) is a proxy only — the solver's *configured* MIP-gap tolerance (`controller::milp_planner::types::MIP_GAP_TARGET`, `0.02`), not the *achieved* gap on any given solve. `good_lp`/`highs` expose no achieved-gap query today, so a plan that solved to a much tighter gap than the 2% target looks identical, in this field, to one that just barely made the cutoff. Explicit scope decision made during GB-25's implementation, not an oversight | Low — diagnostic-quality gap, not a functional defect; the configured target still tells an operator "this is at least as good as X%" | Low-Medium — needs either a `good_lp`/`highs` upstream API for the achieved gap, or computing it manually from the solver's best-bound and incumbent objective values if HiGHS exposes those separately |
| GB-34 | `react-router`/`react-router-dom` (both `VEN/ui` and `VTN/ui`, pinned `^6.26.0`) has no patched 6.x release for its current advisories (open redirect via backslash in `<Link>`/`useNavigate`, arbitrary constructor injection via `deserializeErrors()`) — the latest 6.x, `6.30.4`, is itself still inside the vulnerable range; the only fix is the v7 major line (`7.18.x`). Split out of GB-16 2026-08-16 once `npm audit fix --dry-run` confirmed react-router doesn't move past `6.30.4` without `--force`, and that a real API migration (React Router v7's data-router APIs, some hook/export changes) is needed, not a patch/minor bump like the rest of GB-16's findings | Low-Medium — closes the last open moderate finding from GB-16; react-router is runtime-shipped in both UIs | Medium — real breaking-change migration: bump `react-router-dom` to `^7.18.0` in both `package.json`s, migrate any v6-specific API usage, re-verify both UI suites plus the E2E BDD suite (route navigation is exactly what `@ven-ui`/`controller` scenarios exercise) |
| GB-35 | GB-22's own "audit other `@ven-ui`/browser scenarios for the same gap" reminder, narrowed: a full `features/` tree audit (2026-08-16, closing GB-22) found the browser+real-backend-poll race pattern concretely limited to the scenarios GB-22 itself fixed, but also surfaced a broader, lower-priority class not addressed there — backend-only scenarios with long (90–300s) `poll_until` calls that share the same host-load sensitivity mechanism without the browser-page combo that caused GB-22's actual reported failures: `features/controller/05_ev_charging_scenarios.feature` (all 4 scenarios, `dispatcher_steps.py` timeouts up to 90s) plus other long-timeout call sites (`alerts_steps.py`, `ev_charging_steps.py`, `uc_steps.py`, `planner_steps.py` at 300s; `request_modes_steps.py`, `comfort_steps.py` at 180s). None have failed under contention yet (unlike the GB-22 instances), so isolating all of them pre-emptively would bloat the isolated pass without evidence; this item exists so the reminder isn't lost, not to isolate all of them by default | Low — speculative, no confirmed failures yet, same mechanism as GB-22 | Low — review each site if/when it's observed flaking, same "tag `@isolated` or raise the timeout" fix shape as GB-22 |

---

## Dependency Vulnerabilities — 2026-08-16 (npm rows); 2026-07-16 (cargo rows)

> Re-run `cargo audit` and `npm audit` before each release and update this section.

Cargo rows unchanged since the 2026-07-16 `cargo update` (VEN, VTN/bff) pass on
`fix/review-c3-code` — not re-verified in this update. npm rows re-run 2026-08-16
(GB-16): the 2026-07-16 "0 vulnerabilities" claim for both UIs had gone stale —
`npm audit` had accumulated 7 findings (3 moderate, 4 high) by 2026-08-03 (GB-16
filed) and grew to include `js-yaml`/`undici` by 2026-08-16. `npm audit fix`
(no `--force`) in both `VEN/ui` and `VTN/ui` resolved everything except
`react-router`/`react-router-dom`, split out as GB-34 (no patched 6.x exists;
fix requires the v7 major line, a real migration, not a hygiene bump):

| Component | cargo/npm audit result |
|-----------|------------------------|
| VEN (Rust) | **0 vulnerabilities, 0 warnings** (267 crates) — not re-verified 2026-08-16 |
| VTN/bff (Rust) | 1 advisory — see below (315 crates) — not re-verified 2026-08-16 |
| VEN/ui (npm) | 2 moderate (`react-router`/`react-router-dom`, tracked as GB-34) — all other findings fixed 2026-08-16 |
| VTN/ui (npm) | 2 moderate (`react-router`/`react-router-dom`, tracked as GB-34) — all other findings fixed 2026-08-16 |

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
