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
leaving it undone.

### VEN user (site operator) — money saved

| ID | What the user gets | Effort | Risk |
|---|---|---|---|
| [BL-09](#bl-09-phase-6--penalty-threshold-check) | Avoids demand-charge/peak penalties by rescheduling load ahead of a threshold — real €/kW savings on penalty tariffs | L | Medium — new constraint category + cost comparison, touches Phase 6 slot reallocation |
| [BL-11](#bl-11-time-weighted-tariff-averaging-for-planner-slot-costing) | Slightly cheaper/more accurate plans for slots that straddle a tariff-rate boundary | S | Low — isolated calc on existing `TimeSeries` |
| [BL-13](#bl-13-early-firm-up-heuristic) | Fewer noisy replans under flat-rate tariffs (plan feels more stable) | S | Low — statistical check + reclassification |
| [BL-22](#bl-22-apply_battery_correction_overlay--wire-behind-a-flag-or-re-confirm-abandoned) | Tighter grid-deviation tracking → better self-consumption; logic already built and tested | S | Low — flag-gated, but needs a decision on adoption-gate interaction |

### VEN user (site operator) — comfort, control & trust

| ID | What the user gets | Effort | Risk |
|---|---|---|---|
| [BL-34](#bl-34-comfort-curves-never-reach-the-milp-constraints) | Comfort-preference sliders the UI already exposes actually change what the planner does — today they're silently dropped | M | Medium — touches solver constraints on every asset path |
| [BL-27](#bl-27-poweradjustability--powerrange--device-control-mode-classification) | UI controls (e.g. a stepped EV charger) snap to real device levels instead of rendering a misleading continuous slider | M | Low–Medium — every asset's `capability()` impl must report it |
| [BL-18](#bl-18-assetflexibility--real-time-per-asset-flexibility-snapshot) | A live "how much can this device flex right now" widget, per asset instead of whole-site | M (scope TBD) | Low — but needs a design decision (superseded by `FlexibilityEnvelope`?) before scoping |
| [BL-35](#bl-35-notification-producers-for-tier-fallback--deadline-at-risk--packet-abandoned) | Gets warned *before* a tier fallback / missed deadline / abandoned session, not after | S (once BL-09 lands) | Low — blocked on BL-09's tier machinery existing |
| [BL-39](#bl-39-per-session-accumulated-cost-accounting-real-budget-bar) | Budget bar on the session board shows real money spent so far instead of a plan-time estimate | M | Medium — new accounting invariant in monitor/ledger or history-store, session attribution |
| [BL-37](#bl-37-reactive-correction-events-into-the-notification-feed-sse-blind-spot) | Learns about reactive battery corrections even when not watching the Planner tab (today they're invisible elsewhere) | S | Low — one producer on the existing notification path |
| [BL-38](#bl-38-planner-tab-layout--userdiagnostic-split-and-matrix-slottrace-linking) | Planner tab reads cleanly for operators (user zone on top) and debugs faster (click a slot → see its trace) | S (layout) / M (slot→trace) | Low — UI-only |
| [GB-09](#general-backlog) | Fleet operators get a per-profile poll-interval override | S | Low — current jitter already covers the motivating case, so low urgency |

### VEN user (site operator) — forecast accuracy

| ID | What the user gets | Effort | Risk |
|---|---|---|---|
| [BL-17](#bl-17-externaldatasource--external-weatherirradiationco2-forecast-ingestion) | Better PV-yield and grid-CO2-aware planning from real weather/irradiance/CO2 forecasts instead of none | L | Medium–High — third-party API dependency, staleness/failure handling, provider not yet chosen |

### VTN user (aggregator / program operator)

| ID | What the user gets | Effort | Risk |
|---|---|---|---|
| [GB-04](#general-backlog) | VTN UI stays responsive as event history grows (SQL-side filtering instead of post-filter Rust) | S | Low |
| [GB-05](#general-backlog) | Faster triage — Events page can filter to active events, not just text-search | S | Low |
| [BL-24](#bl-24-oadrprogramconfigoadreventcacheoadrcapacityrequest-wiring) | Would let the VTN request/receive capacity reservations from the VEN — no such workflow exists today | S if removed / unknown if built | Low — no consumer yet; recommend leaving parked until a real feature needs it |

### No direct user value — internal cleanup/consistency (do opportunistically, don't prioritize)

| ID | Note |
|---|---|
| [BL-21](#bl-21-reconcile-duplicate-thermalmodelparams) | Duplicate dead struct, superseded by `assets/heater.rs`'s own |
| [BL-23](#bl-23-hvacservice--route-wiring-or-removal-of-the-unused-impl) | Consistency-only decision, no behavior change either way |
| [BL-26](#bl-26-assetstate-entities--resolve-the-name-collision-with-the-live-assetsassetstate) | Dead type shadowing a live one's name |
| [BL-29](#bl-29-flexibilitydirection-ratetype-rateunit--narrow-supporting-enums) | No standalone value — fold into whichever future feature needs each enum |
| [GB-07](#general-backlog) | Dev/ops convenience (container setup script), not user-facing |
| [GB-11](#general-backlog) | Process/docs alignment items, not user-facing |

---

### BL-09: Phase 6 — Penalty threshold check
**Req:** UC-10, VEN_ARCHITECTURE §2.3
**Problem:** Planner Phase 6 is marked "deferred to Stage 4" (`planner.rs:76`). No penalty avoidance logic exists. Peak demand penalties are not evaluated.
**Fix:** After Phase 5, evaluate each FIRM slot against configurable penalty thresholds (e.g., MeasurementWindow peak kW). If projected peak exceeds threshold, compute penalty cost vs. avoidance cost (rescheduling allocations to stay below). Reschedule if avoidance is cheaper.
**Complexity:** Large (5–8 hours). Needs penalty rule configuration, threshold evaluation, cost comparison, and slot reallocation.
**Verify:** BDD test: configure 10kW penalty threshold, schedule 12kW of load in one slot, assert planner splits across two slots to stay below threshold.

---

### BL-11: Time-weighted tariff averaging for planner slot costing
**Req:** VEN_ARCHITECTURE §5.3
**Problem:** Planner evaluates tariff at `slot.start` only. A 5-min slot straddling a tariff boundary (e.g., €0.20 → €0.15 at 10:57) uses only the first tariff, ignoring the 3 min at the cheaper rate.
**Fix:** Replace `tariff_at(slot.start)` with `Σ(tariff_i × overlap(slot, interval_i)) / slot.duration` using the existing `TimeSeries` abstraction. For capacity: `min(capacity_i for all overlapping intervals)`.
**Complexity:** Small–Medium (2–3 hours). Use existing TimeSeries infrastructure.
**Verify:** Unit test: 10-min slot spanning tariff boundary at minute 7 → weighted average matches `(7*0.20 + 3*0.15)/10 = 0.185`.

---

### BL-13: Early firm-up heuristic
**Req:** VEN_ARCHITECTURE §2.3
**Problem:** Spec says if rate variance across FLEXIBLE window is < 10% (flat rate), FLEXIBLE slots may firm up early. Code comment at `planner.rs:271` acknowledges this but it's not implemented.
**Fix:** After Phase 7, compute variance of tariff across all FLEXIBLE slots. If coefficient of variation < 0.10, reclassify FLEXIBLE → FIRM and re-run allocation (Phases 2–5) for those slots.
**Complexity:** Small (1–2 hours). Statistical check + slot reclassification.
**Verify:** Unit test: flat-rate tariff (all €0.15) → all slots classified FIRM. Variable tariff (€0.10–€0.30) → FLEXIBLE slots remain FLEXIBLE.

---

### BL-17: ExternalDataSource — external weather/irradiation/CO2-forecast ingestion
**Req:** entities/design_vocabulary.rs §2.11 (`ExternalDataSource`, `ExternalDataSourceType`, `ExternalDataFetchStatus`)
**Problem:** No code path polls an external weather/irradiation/CO2-intensity feed. PV forecasting has no external data input to draw from — `ExternalDataSource` sketches the polling/caching contract but nothing implements it.
**Fix:** Implement a poll loop per configured `ExternalDataSource` (weather, irradiation, grid CO2 forecast), caching the last successful response and tracking `ExternalDataFetchStatus`; feed results into `ForecastSource::WeatherModel`-tagged `AssetForecast`s.
**Complexity:** Large. External API integration, caching, and failure/staleness handling depend on which provider is chosen.
**Verify:** TBD — depends on the chosen external API; at minimum, a fake-server integration test asserting `fetch_status` transitions correctly on success/failure/timeout.

---

### BL-18: AssetFlexibility — real-time per-asset flexibility snapshot
**Req:** entities/design_vocabulary.rs §3.5 (`AssetFlexibility`)
**Problem:** `AssetFlexibility` sketches an on-demand "how much can this asset flex right now" snapshot (`can_increase/decrease_consumption/production_kw`), computed per-asset rather than for the whole site. This is distinct from `FlexibilityEnvelope`, which is planner-produced, horizon-wide, and already reported to the VTN — `AssetFlexibility` would be the instantaneous, single-asset building block.
**Fix:** Decide first whether this is still wanted as a separate real-time endpoint (e.g. for a live UI widget) or fully superseded by `FlexibilityEnvelope`; if wanted, compute it on demand from each asset's current state and `PowerRange`/`ThermalModelParams` limits, no persistence needed.
**Complexity:** Medium, but scope depends on the design decision above — resolve that first.
**Verify:** TBD pending scope decision.

---

### BL-21: Reconcile duplicate ThermalModelParams
**Req:** entities/design_vocabulary.rs §3.1.1 (`ThermalModelParams`)
**Problem:** `entities/design_vocabulary.rs::ThermalModelParams` (thermal mass, insulation factor, min/max temperature) has zero references anywhere — `assets/heater.rs` already has its own, separately-defined thermal parameter struct that is the one actually wired into the heater's MILP model. This one is a leftover duplicate from the original spec-vocabulary pass, not a distinct future feature.
**Fix:** Confirm `assets/heater.rs`'s struct is a full superset; if so, delete `entities/design_vocabulary.rs::ThermalModelParams` and its now-unused field on the (already-quarantined) `AssetProfile`. If it's missing fields the entities version has, fold those into the heater-side struct instead of keeping two.
**Complexity:** Small. Comparison + deletion or field merge.
**Verify:** `cargo build` clean after deletion; heater MILP tests unaffected.

---

### BL-22: `apply_battery_correction_overlay` — wire behind a flag, or re-confirm abandoned
**Req:** `controller/dispatcher.rs` (`apply_battery_correction_overlay`)
**Problem:** A finished, unit-tested dead-beat P-controller that reacts to grid deviation by nudging the battery setpoint — but never called from `build_setpoints()`. Deliberately kept unwired pending the wire-or-delete decision below.
**Fix:** Either wire it behind a profile flag (e.g. `battery.deviation_correction_enabled`) so it's an opt-in feature, or, at a later date, re-confirm with the user that it's genuinely abandoned and delete it then — this entry exists so that decision doesn't get lost.
**Complexity:** Small to wire behind a flag (the function and its tests already work); the design decision (default on/off, interaction with the adoption gate) is the real work.
**Verify:** Integration test: with the flag on, sustained grid deviation produces a nonzero correction visible in the dispatcher's setpoint output; with it off, behaviour is unchanged from today.

---

### BL-23: `HvacService` — route wiring or removal of the unused impl
**Req:** `services/hems.rs` (`HvacService`)
**Problem:** `EvSessionService` is the live pattern for session lifecycle; `HvacService` sketches the same shape for heater targets, but `post_heater_target` sets the target directly instead of going through it — so `HvacService`'s methods are never called.
**Fix:** Either route `post_heater_target` through `HvacService` for consistency with the EV path, or fold whatever `HvacService` was meant to add into the existing direct path and delete the empty shell.
**Complexity:** Small — this is a consistency decision, not new functionality.
**Verify:** `cargo build` clean; existing heater-target route tests unaffected either way.

---

### BL-24: `OadrProgramConfig`/`OadrEventCache`/`OadrCapacityRequest` wiring
**Req:** `entities/capacity.rs`
**Problem:** Three unwired sketches in a file that otherwise holds live types (`OadrCapacityState`, `OadrReportObligation`). DISPATCH_SETPOINT handling landed as typed `DispatchWindow` state (matching the alert/SIMPLE pattern), so `OadrEventCache`'s anticipated consumer no longer exists; `OadrProgramConfig` and `OadrCapacityRequest` have no consumer at all — no code path builds or sends a capacity reservation request to the VTN in this shape.
**Fix:** Consider removal for all three next time this file is triaged; for `OadrProgramConfig`/`OadrCapacityRequest`, no dependent feature identified yet — lowest priority of this group until one exists.
**Complexity:** Small (removal) — TBD if a consuming feature appears instead.
**Verify:** Tied to whichever consuming feature lands first, or `cargo build` clean after removal.

---

### BL-26: `AssetState` (entities) — resolve the name collision with the live `assets::AssetState`
**Req:** `entities/design_vocabulary.rs` (`AssetState`)
**Problem:** A second unreferenced type sharing a name with a real, heavily-used live type (`assets::mod::AssetState`, the per-device-kind enum driving `step`/`capability`). The entities-level one (device status snapshot: commanded/actual power, responsiveness, SoC, temperature, connection) has no consumer and predates the real `Asset` trait design.
**Fix:** Most likely resolution: this was superseded by the live `assets::AssetState` + `AssetCapability` combination and should eventually be deleted rather than implemented — but that's a re-confirmation, not assumed here. If any of its fields (e.g. `last_confirmed_response`, `is_available`) represent monitoring data genuinely missing from the live type, fold those in instead.
**Complexity:** Small — comparison against the live type, then either deletion or a small field migration.
**Verify:** `cargo build` clean; no behavior change (nothing references it today).

---

### BL-27: `PowerAdjustability` + `PowerRange` — device control-mode classification
**Req:** `entities/design_vocabulary.rs` (`PowerAdjustability`, `PowerRange`)
**Problem:** Not a duplicate of the live `AssetCapability` (`assets/mod.rs`) as might be assumed at a glance — `AssetCapability` only carries instantaneous `max_import_kw`/`max_export_kw`, no discrete-step list and no semantic classification of *how* an asset can be controlled (on/off vs. stepped vs. continuously variable vs. curtail-only vs. advisory-only). `PowerAdjustability`/`PowerRange.power_steps_kw` sketch a real, currently-missing capability: exposing control-mode metadata (e.g. to the UI, so a stepped charger's slider snaps to real levels instead of rendering continuous).
**Fix:** If wanted, add `adjustability: PowerAdjustability` and `power_steps_kw: Vec<f64>` to the live `ControlDescriptor`/`AssetCapability` path (`assets/mod.rs`) rather than reviving these as separate entities-level types.
**Complexity:** Medium — touches every asset's `capability()` implementation to report ranked/stepped power correctly.
**Verify:** UI test: a stepped-charger control descriptor exposes discrete levels; a stepless one doesn't.

---

### BL-29: `FlexibilityDirection`, `RateType`, `RateUnit` — narrow supporting enums
**Req:** `entities/design_vocabulary.rs`
**Problem:** Three small enums with no current consumer. `RateUnit` overlaps with the live `RateUnit`-shaped fields already handled ad hoc as bare `f64`/currency-implicit values in `TariffSnapshot`; `RateType` (per-kWh vs. per-kW) and `FlexibilityDirection` (import/export) are classification vocabulary for capacity-rate handling and capacity-request direction respectively — relevant once envelope reporting is extended or BL-24's `OadrCapacityRequest` is actually implemented.
**Fix:** Don't implement standalone — fold each into whichever feature actually needs it when that feature is built: `RateType`/`RateUnit` into a future multi-currency/multi-unit tariff handling pass (no BL item yet — add one if/when multi-currency support is requested); `FlexibilityDirection` into envelope-report-building work.
**Complexity:** N/A standalone — tracked here only so they're not forgotten, not as independent work items.
**Verify:** N/A until folded into a parent feature.

---

### BL-34: Comfort curves never reach the MILP constraints
**Req:** REQUIREMENTS §comfort curves; `services/comfort.rs`, `controller/user_request.rs`
**Problem:** The comfort-curve override path (routes + `SettingsPort` persistence + `AssetRequestSlice` resolution) is live, but `create_from_body` resolves the curve and then drops it (`_comfort_rates`) — no curve, default or user-provided, is translated into solver tier constraints. The curve currently influences nothing the planner decides.
**Fix:** Translate the resolved `ComfortRate` curve into MILP tier constraints/rewards in the session-intent path so fill-level bids actually shape the allocation.
**Complexity:** Medium — solver-side constraint construction plus tests per asset path.
**Verify:** Planner unit test: two identical sessions with different curves produce different allocations; no-curve session falls back to `default_comfort_rates()` behaviour.

---

### BL-35: Notification producers for tier fallback / deadline-at-risk / packet abandoned
**Req:** `entities/design_vocabulary.rs` (`UserNotificationSeverity` doc comments)
**Problem:** The notification feed (ring + SSE + persistence, `services/notify.rs`) carries grid-emergency, VTN-reachability, and adopted-plan-warning producers. The remaining producers named by the severity enum's own doc comments — tier fallback, deadline approaching, packet abandoned — have nothing to hook onto because the Stage-5 tier machinery (BL-09-adjacent) doesn't exist yet.
**Fix:** Wire these producers when the tier/penalty machinery lands (BL-09); each should emit through the existing `Notifier` with a stable dedup text.
**Complexity:** Small once the producing machinery exists.
**Verify:** Test per producer: the triggering condition emits exactly one notification of the expected severity.

---

### BL-37: Reactive-correction events into the notification feed (SSE blind spot)
**Req:** `VEN/ui/src/pages/Planner.tsx` (`usePlannerEvents`, CorrectionBanner); `services/notify.rs`; wiki `queries/planner-tab-purpose.md`
**Problem:** `usePlannerEvents` subscribes to the planner SSE stream only while the Planner page is mounted, so a Layer-1 reactive battery correction firing while the user is on any other tab is invisible — the CorrectionBanner never renders and no durable record reaches the user.
**Fix:** Emit `correction_active`/`correction_cleared` through the existing backend `Notifier` (ring + SSE + persistence, stable dedup text), so the global NotificationsBell carries it on every tab; the Planner-tab banner remains as the richer live view.
**Complexity:** Small — one edge-triggered producer on an existing signal, following the established producer pattern.
**Verify:** Test: a sustained deviation triggering a correction emits exactly one notification of severity info/warning; clearing emits at most one follow-up; no duplicates while the correction stays active.

---

### BL-38: Planner tab layout — user/diagnostic split and matrix-slot→trace linking
**Req:** `VEN/ui/src/pages/Planner.tsx`; wiki `queries/planner-tab-purpose.md`
**Problem:** User-facing elements (objective, power stack, session progress) are interleaved with diagnostic surfaces (trigger timeline, decision matrix, trace table), so the operator persona sees noise and the debugging persona scrolls past controls. Additionally, answering "what happened in slot 14:35?" requires manually cross-reading the decision matrix and the trace table.
**Fix:** (a) Reorder into a user zone on top (objective + legend, power stack, session progress) and a diagnostics zone below a divider, collapsed by default like the existing trace accordion. (b) Make decision-matrix slots clickable, filtering the TraceTable to entries relevant to that slot's window.
**Complexity:** Small for (a) — pure reordering/collapse; Medium for (b) — needs a slot↔trace-entry time-window correlation and filter state.
**Verify:** (a) UI test: diagnostics sections render collapsed by default, user zone above the divider. (b) UI test: clicking a matrix slot filters the trace table to entries whose timestamp falls in that slot.

---

### BL-39: Per-session accumulated-cost accounting (real budget bar)
**Req:** `VEN/ui/src/components/sessions/SessionProgressBoard.tsx` (BudgetLine); `VEN/src/controller/monitor.rs` (AssetLedger); `docs/reference/TECHNICAL_DEBTS.md` R-24 (ledger clock/persistence)
**Problem:** The session board's budget bar compares the user's budget against `estimated_cost_eur` (a plan-time estimate, labeled "est.") because no per-session accumulated cost exists anywhere: the `AssetLedger` accumulates per asset since startup with no session attribution, resets on restart, and the plan envelope's `budget_remaining_eur` is a placeholder. Spun off from the BL-36 resolution — the SessionProgressBoard rebuild deliberately excluded this.
**Fix:** Either extend the monitor ledger with session-scoped accumulation (attribute each tick's asset cost to the active session id), or derive it on demand from the history store windowed on `session.created_at` × recorded tariffs. Decide only if enforcement-grade budget tracking is actually needed; the estimate may be good enough.
**Complexity:** Medium — a new accounting invariant plus persistence questions (interacts with R-24).
**Verify:** Unit test: a session accumulating N ticks at known power/tariff reports Σ(power × Δt × tariff); UI budget bar switches from "est." to actual once the field exists.

---

## General Backlog

| ID | Item | Priority |
|---|---|---|
| GB-04 | DB-level optimization: add `ends_at timestamptz` index so `?active=true` runs in SQL, not post-filter Rust | Low (not needed until event table is large) |
| GB-05 | VTN UI: filter past events from event table (the Events page search box matches text only — no active/past filtering) | Low |
| GB-07 | Add setup script to bring up all required containers (fleet.sh covers only the fleet VENs; VTN stack + base VENs are separate compose invocations) | Low |
| GB-09 | Per-profile VEN poll interval override. The original motivation ("N VENs don't poll in lockstep") is met via the one-time `POLL_STARTUP_JITTER_S` stagger; a per-profile interval override remains unbuilt and nothing currently needs it | Low |
| GB-11 | Remaining AI-SW-Development alignment items (from the retired root alignment-plan.md, Pass 3): backlog-handling + tool-installation + archive-folder notes in CLAUDE.md; USER_STORIES.md; RISK_ANALYSIS.md; PROMPT_LIBRARY.md; changelog decision (journal-as-changelog note); security-review cadence; automated code-review hook; file-header descriptions on key VEN modules | Low |

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

**Risk context:** Lab/Pi4 deployment — not internet-exposed. Re-run both audits before
any internet-exposed deployment.
