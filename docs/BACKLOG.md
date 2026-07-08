## REQUIREMENTS.md and VEN_ARCHITECTURE.md: Requirements Gap Backlog (from 2026-03-21 code audit)

Items ordered by recommended implementation sequence: dependencies first, then by impact.

> **Note (2026-07-05):** BL-01 through BL-13 are numbered in original-audit order, not
> necessarily today's priority. BL-14 through BL-29 (added from the R5 dead-code review
> — see `docs/plans/review_items_resolution_strategy.md` R5) were appended at the end
> rather than interleaved by priority. All of R5's items are now quarantined-and-kept,
> not deleted — the underlying types moved to `entities/design_vocabulary.rs` (Group 1)
> or stayed in place with corrected comments (Group 2: BL-22 through BL-25). This file
> needs a proper re-sort/prioritization pass to decide actual implementation order — not
> done here, flagged for a future session.

---

### ~~BL-01: PlanTrigger wiring — RATE_CHANGE / CAPACITY_CHANGE~~ ✅ RESOLVED
`trigger_tx.send(PlanTrigger::RateChange)` is implemented in `tasks/poll_events.rs`. Verified 2026-07-03.

---

### BL-02: Event priority ordering before merge — RESOLVED (Phase 0, WP0.1)
**Req:** FR-OA-08
**Problem:** `openadr_interface.rs:186-195` merges events in array order (last-write-wins). A lower-priority event processed later silently overwrites a higher-priority one. The `priority` field is not read at all.
**Fix:** Extract `priority` (integer, lower = higher priority) and `createdDateTime` from each event. Sort events by ascending priority, then descending `createdDateTime` (newer breaks ties), before entering the merge loop.
**Complexity:** Small (1–2 hours). Sort + two field extractions.
**Verify:** Unit test: two PRICE events with same interval, different priorities — higher-priority value wins. Second test: same priority, newer event wins.
**Resolution:** Added `createdDateTime: Option<String>` to `OadrEvent` (vtn_port.rs). `parse_rate_snapshots` now sorts events before the merge loop so the highest-priority (lowest number, `None` = lowest) event is processed last, with `createdDateTime` breaking priority ties (newer wins). 3 new unit tests. See project_journal.md.

---

### BL-03: Exponential backoff on VTN communication failure
**Req:** FR-OA-07
**Problem:** All poll loops (`main.rs:101-298`) use fixed `tokio::time::interval`. On VTN failure, VEN retries every 30s indefinitely — no backoff, no jitter.
**Fix:** Replace fixed interval with adaptive delay: on success reset to 30s; on failure double delay (30s → 60s → 120s → 240s → 480s → max 900s). Add ±10% jitter. On success, reset immediately.
**Complexity:** Medium (2–4 hours). Affects 3 poll loops (programs, events, reports). Extract shared backoff helper.
**Verify:** Integration test: stop VTN, observe VEN log shows increasing intervals. Restart VTN, observe immediate reset to 30s.

---

### BL-04: ALERT_GRID_EMERGENCY handling
**Req:** UC-06, OA-01
**Problem:** `ALERT_GRID_EMERGENCY` and `ALERT_BLACK_START` event types are not parsed. Emergency signals from the VTN are silently ignored.
**Fix:** In `openadr_interface`, detect ALERT payload types and emit `PlanTrigger::Alert`. Planner enforces a zero/minimal import hard constraint for the alert duration as highest-priority FIRM slots.
**Complexity:** Medium (3–5 hours). New parsing path + synthetic packet creation + planner priority handling.
**Verify:** BDD test: send ALERT_GRID_EMERGENCY event, assert planner creates shed packet and reduces import within one poll cycle.

---

### BL-05: Obligation-triggered report submission
**Req:** FR-OA-04
**Problem:** `main.rs:506-512` checks `due_obligations(now)` and marks them `fulfilled`, but does **not** build or submit reports. Reports are only sent on timer (`report_interval_s`) and packet transitions — not when obligations actually become due.
**Fix:** In the obligation check loop, when `due_obligations` returns non-empty: call `build_measurement_reports_for_active_events()` for each due obligation, submit via `upsert_report()`, then mark fulfilled.
**Complexity:** Small–Medium (2–3 hours). Wire existing report builder to obligation trigger.
**Verify:** BDD test: create event with `reportDescriptor` that has short interval, assert report submitted at `due_at` time (not just at timer tick).

---

### BL-06: DISPATCH_SETPOINT + CHARGE_STATE_SETPOINT parsing
**Req:** UC-13, VEN_ARCHITECTURE §2.1
**Problem:** These event types are not parsed in `openadr_interface`. `DISPATCH_SETPOINT` should bypass the planner and go directly to the dispatcher. `CHARGE_STATE_SETPOINT` should create/modify an `EvSession` targeting the specified SoC.
**Fix:** Add parsing branches in `openadr_interface` for both types. `DISPATCH_SETPOINT` → store in `OadrEventCache.dispatch_setpoints` (field already exists in `capacity.rs:53`) and flag for dispatcher override. `CHARGE_STATE_SETPOINT` → create `EvSession` with target SoC via `user_request` machinery.
**Complexity:** Medium (4–6 hours). Two new parsing paths + dispatcher override mode + session creation.
**Verify:** BDD test: send DISPATCH_SETPOINT event, assert sim setpoint matches within one poll cycle. Send CHARGE_STATE_SETPOINT, assert `EvSession` created with correct target SoC.

---

### BL-07: StaleRatePolicy dispatch in planner
**Req:** UC-12, REQUIREMENTS §3.2.1
**Problem:** `StaleRatePolicy` enum is defined (`asset.rs:109-114`) with 4 variants (LAST_KNOWN, HEURISTIC_FORECAST, DEFER_TO_FLEXIBLE, SAFE_AVERAGE), but the planner has no dispatch logic. When VTN is unreachable, slots beyond the last known tariff get no special treatment.
**Fix:** In planner Phase 1 (`build_grid`), after populating tariff data, detect slots with no rate coverage. Apply the configured `StaleRatePolicy`: LAST_KNOWN → repeat last value; DEFER_TO_FLEXIBLE → mark those slots FLEXIBLE regardless of horizon; SAFE_AVERAGE → use configurable percentile tariff.
**Complexity:** Medium (3–4 hours). Policy dispatch + per-slot fallback logic.
**Verify:** Unit test: planner with rates covering only 2h of a 6h horizon, each policy variant produces different slot classifications and costs.

---

### BL-08: SITE_RESIDUAL computation
**Req:** REQUIREMENTS §3.3, VEN_ARCHITECTURE §2.1 (Monitor)
**Problem:** `AssetType::SiteResidual` is defined but never instantiated. The monitor does not compute `P_residual = P_utility − Σ P_modelled_assets`. Unmodeled site consumption is invisible to the planner.
**Fix:** In the monitor's 1s tick, compute residual power from grid meter minus sum of all modeled asset powers. Expose as a virtual asset entry (read-only, not controllable). Include in planner baseline so it accounts for background load.
**Complexity:** Medium (3–4 hours). New virtual asset + monitor computation + planner baseline integration.
**Verify:** Unit test: sim with known base_load + PV, grid meter shows extra 500W → SITE_RESIDUAL reads 500W. Planner baseline includes it.

---

### BL-09: Phase 6 — Penalty threshold check
**Req:** UC-10, VEN_ARCHITECTURE §2.3
**Problem:** Planner Phase 6 is marked "deferred to Stage 4" (`planner.rs:76`). No penalty avoidance logic exists. Peak demand penalties are not evaluated.
**Fix:** After Phase 5, evaluate each FIRM slot against configurable penalty thresholds (e.g., MeasurementWindow peak kW). If projected peak exceeds threshold, compute penalty cost vs. avoidance cost (rescheduling allocations to stay below). Reschedule if avoidance is cheaper.
**Complexity:** Large (5–8 hours). Needs penalty rule configuration, threshold evaluation, cost comparison, and slot reallocation.
**Verify:** BDD test: configure 10kW penalty threshold, schedule 12kW of load in one slot, assert planner splits across two slots to stay below threshold.

---

### BL-10: FlexibilityEnvelope → VTN report
**Req:** UC-05, UC-07
**Problem:** Planner builds `FlexibilityEnvelope` (Phase 7) and exposes via `GET /flexibility`, but never submits them to the VTN as `IMPORT_CAPACITY_RESERVATION` / `EXPORT_CAPACITY_RESERVATION` reports. Aggregators cannot see available DR capacity.
**Fix:** In the report submission loop, when a new plan is produced with non-empty envelopes, build report payloads of type `IMPORT_CAPACITY_RESERVATION` / `EXPORT_CAPACITY_RESERVATION` from the envelope data and submit to VTN.
**Complexity:** Medium (3–5 hours). Report payload construction from envelope fields + submission wiring.
**Verify:** BDD test: planner produces envelopes for FLEXIBLE packets, assert VTN receives capacity reservation report with matching power/energy values.

---

### BL-11: Time-weighted tariff averaging for planner slot costing
**Req:** VEN_ARCHITECTURE §5.3
**Problem:** Planner evaluates tariff at `slot.start` only. A 5-min slot straddling a tariff boundary (e.g., €0.20 → €0.15 at 10:57) uses only the first tariff, ignoring the 3 min at the cheaper rate.
**Fix:** Replace `tariff_at(slot.start)` with `Σ(tariff_i × overlap(slot, interval_i)) / slot.duration` using the existing `TimeSeries` abstraction. For capacity: `min(capacity_i for all overlapping intervals)`.
**Complexity:** Small–Medium (2–3 hours). Use existing TimeSeries infrastructure.
**Verify:** Unit test: 10-min slot spanning tariff boundary at minute 7 → weighted average matches `(7*0.20 + 3*0.15)/10 = 0.185`.

---

### BL-12: EV minimum charge rate + response delay model
**Req:** FR-SIM-05
**Problem:** EV asset has no 1.5kW minimum active charge rate floor. Setpoints between 0 and 1.5kW are accepted (should snap to 0 or 1.5kW). 10s response delay not modeled — setpoints apply instantly.
**Fix:** In `assets/ev.rs` update logic: if `0 < setpoint < min_charge_kw`, snap to 0. Add single-step lag buffer: store commanded setpoint, apply previous tick's command (simulating 10s delay at 10s tick or interpolated at 1s tick).
**Complexity:** Small (1–2 hours).
**Verify:** Unit test: setpoint 0.5kW → actual power 0. Setpoint 7kW at t=0 → actual power still 0 at t=0, becomes 7kW at t=10s.

---

### BL-13: Early firm-up heuristic
**Req:** VEN_ARCHITECTURE §2.3
**Problem:** Spec says if rate variance across FLEXIBLE window is < 10% (flat rate), FLEXIBLE slots may firm up early. Code comment at `planner.rs:271` acknowledges this but it's not implemented.
**Fix:** After Phase 7, compute variance of tariff across all FLEXIBLE slots. If coefficient of variation < 0.10, reclassify FLEXIBLE → FIRM and re-run allocation (Phases 2–5) for those slots.
**Complexity:** Small (1–2 hours). Statistical check + slot reclassification.
**Verify:** Unit test: flat-rate tariff (all €0.15) → all slots classified FIRM. Variable tariff (€0.10–€0.30) → FLEXIBLE slots remain FLEXIBLE.

---

Add log for past. to be shown in VEN UI

---

### BL-14: AssetHeuristics — learned behavioral profile for uncontrollable assets
**Req:** entities/design_vocabulary.rs §3.3 (`AssetHeuristics`)
**Problem:** `AssetHeuristics` (24-entry `daytime_profile_kw`, 7-entry `weekday_weights`, `seasonal_factor`) is defined but never populated or read. Uncontrollable/implicit loads (base load, PV with no weather feed) have no learned-pattern fallback — the planner has nothing better than flat/last-known extrapolation for them.
**Fix:** Add a background job that aggregates persisted per-asset history into `daytime_profile_kw`/`weekday_weights`/`seasonal_factor` on a rolling basis; feed the result into `AssetForecast` (BL-15) as the `ForecastSource::Heuristic` source for assets without a physical or weather model.
**Complexity:** Large. New aggregation job + persistence + planner consumption path.
**Verify:** TBD once designed — needs a fixture with multi-week synthetic history to assert the learned profile converges to the injected pattern.

---

### BL-15: AssetForecast — per-asset predicted power profile
**Req:** entities/design_vocabulary.rs §3.6 (`AssetForecast`, `ForecastSource`, `TimeRange`)
**Problem:** `AssetForecast` (per-step predicted power/SoC, confidence, availability windows, tagged by `ForecastSource`) is defined but nothing constructs it. The MILP planner computes an equivalent per-slot forecast internally (`planned_state_by_asset`) but never exposes it in this shape, and it's also the missing piece behind the never-built outbound `USAGE_FORECAST` report (see `docs/reference/TECHNICAL_DEBTS.md` R-15).
**Fix:** Build `AssetForecast` from the planner's internal per-slot state after each plan cycle; expose via a route and use it as the source for the `USAGE_FORECAST` report (R-15).
**Complexity:** Medium — mostly plumbing existing planner output into the documented shape, plus BL-14's heuristic source once that exists.
**Verify:** Unit test: after a plan cycle, `AssetForecast.power_kw` matches the planner's `planned_state_by_asset` for the same asset/horizon.

---

### BL-16: AssetLedger — per-asset billing-period cost/CO2 ledger
**Req:** entities/design_vocabulary.rs §3.7 (`AssetLedger`)
**Problem:** `monitor::record_tick` accumulates tick-level cost/CO2 into the asset ledger already (correcting an earlier mis-attribution to the dispatcher — see R5 finding), but there is no `AssetLedger`-shaped per-asset, per-billing-period rollup with defined `period_start`/`period_end` and reset semantics. Nothing constructs or periodically resets one today.
**Fix:** Wire `AssetLedger` as the billing-period aggregation layer on top of `monitor::record_tick`'s tick-level accumulation; add a period-rollover trigger (e.g. monthly) that closes the current ledger and opens the next.
**Complexity:** Medium–Large. Needs period-boundary logic and persistence across restarts.
**Verify:** Unit test: ledger accumulates across ticks within a period, resets exactly at `period_end`, and totals reconcile against `monitor::record_tick`'s raw sums.

---

### BL-17: ExternalDataSource — external weather/irradiation/CO2-forecast ingestion
**Req:** entities/design_vocabulary.rs §2.11 (`ExternalDataSource`, `ExternalDataSourceType`, `ExternalDataFetchStatus`)
**Problem:** No code path polls an external weather/irradiation/CO2-intensity feed. PV forecasting and heuristic-based assets (BL-14/BL-15) have no external data input to draw from — `ExternalDataSource` sketches the polling/caching contract but nothing implements it.
**Fix:** Implement a poll loop per configured `ExternalDataSource` (weather, irradiation, grid CO2 forecast), caching the last successful response and tracking `ExternalDataFetchStatus`; feed results into `ForecastSource::WeatherModel`-tagged `AssetForecast`s (BL-15).
**Complexity:** Large. External API integration, caching, and failure/staleness handling depend on which provider is chosen.
**Verify:** TBD — depends on the chosen external API; at minimum, a fake-server integration test asserting `fetch_status` transitions correctly on success/failure/timeout.

---

### BL-18: AssetFlexibility — real-time per-asset flexibility snapshot
**Req:** entities/design_vocabulary.rs §3.5 (`AssetFlexibility`)
**Problem:** `AssetFlexibility` sketches an on-demand "how much can this asset flex right now" snapshot (`can_increase/decrease_consumption/production_kw`), computed per-asset rather than for the whole site. This is distinct from `FlexibilityEnvelope` (BL-10), which is planner-produced, horizon-wide, and already reported to the VTN — `AssetFlexibility` would be the instantaneous, single-asset building block.
**Fix:** Decide first whether this is still wanted as a separate real-time endpoint (e.g. for a live UI widget) or fully superseded by `FlexibilityEnvelope`; if wanted, compute it on demand from each asset's current state and `PowerRange`/`ThermalModelParams` limits, no persistence needed.
**Complexity:** Medium, but scope depends on the design decision above — resolve that first.
**Verify:** TBD pending scope decision.

---

### BL-19: DefaultValueCurve / ComfortRate user-override wiring
**Req:** entities/design_vocabulary.rs §3.1 (`DefaultValueCurve`, wraps `ComfortRate`)
**Problem:** `ComfortRate` itself is implemented and live (`default_comfort_rates()` on every asset, exposed via `GET` in `routes/hems.rs:268`) — not part of this gap. What's missing is `DefaultValueCurve`, the named wrapper that would let a user override an asset's default comfort/value curve with their own bid curve instead of always using the hardcoded per-asset default.
**Fix:** Add a route to accept a user-provided `DefaultValueCurve` (or equivalent `Vec<ComfortRate>`) per asset, persist it, and have the planner prefer it over `default_comfort_rates()` when present.
**Complexity:** Small–Medium. The comfort-rate consumption path already exists; this only adds the override source and persistence.
**Verify:** Unit test: asset with a user-provided curve uses it in planning instead of the built-in default; asset with none falls back to `default_comfort_rates()` unchanged.

---

### BL-20: UserNotificationSeverity — user-facing notification feed
**Req:** entities/design_vocabulary.rs (`UserNotificationSeverity`, doc comment: "used in Stage 5")
**Problem:** No notification concept exists anywhere in the VEN today — no queue, no route, no UI surface. `UserNotificationSeverity` (Info/Warn/Alert) is the only trace of the intended feature, sketched but with nothing to attach severities to yet.
**Fix:** Design a minimal notification event (message + severity + timestamp + optional asset/event reference), a bounded in-memory feed (similar shape to the existing `/trace/events` ring buffer), and a route to poll or stream it; wire initial producers at natural trigger points (tier fallback, budget warning, deadline approaching, packet abandoned, grid emergency — per the enum's own doc comments).
**Complexity:** Medium. New cross-cutting concept — needs a decision on where in the architecture it's produced from before implementation.
**Verify:** TBD pending design — at minimum, a test asserting a triggered condition (e.g. tier fallback) produces a notification of the expected severity.

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
**Problem:** A finished, unit-tested dead-beat P-controller that reacts to grid deviation by nudging the battery setpoint — but never called from `build_setpoints()`. Kept per explicit user decision (not deleted); its old design-doc reference (`openspec/changes/warnings-cleanup/design.md`) no longer exists.
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
**Problem:** Three unwired sketches in a file that otherwise holds live types (`OadrCapacityState`, `OadrReportObligation`). `OadrEventCache.dispatch_setpoints` is specifically the storage `TECHNICAL_DEBTS.md` R-13 (`DISPATCH_SETPOINT` parsing) would need once that's built; `OadrProgramConfig` and `OadrCapacityRequest` have no consumer at all yet — no code path builds or sends a capacity reservation request to the VTN in this shape.
**Fix:** For `OadrEventCache`: build alongside R-13's `DISPATCH_SETPOINT` parsing work (BL-06 already covers the parsing side). For `OadrProgramConfig`/`OadrCapacityRequest`: no dependent feature identified yet — lowest priority of this group until one exists.
**Complexity:** Tied to BL-06/R-13 for the event cache; TBD for the other two pending a concrete driving feature.
**Verify:** Tied to whichever consuming feature lands first.

---

### BL-25: Reserved `DomainError` variants — wire at real boundaries
**Req:** `entities/error.rs` (`DomainError::{PlanInfeasible, VtnUnreachable, ProfileInvalid}`)
**Problem:** All three are constructed only inside their own `Display`-format unit test — never at an actual error boundary in the running application.
**Fix:** `PlanInfeasible` — return from the planner's solve path when `SolverPort::solve` reports infeasibility, surfaced through the relevant route instead of a generic error. `VtnUnreachable` — classify repeated VTN-client timeouts distinctly from other request failures. `ProfileInvalid` — only applicable if profile hot-reload (not just startup validation) is ever built; until then this variant stays reserved with no natural call site.
**Complexity:** Small–Medium for the first two (mostly replacing an existing generic error return with the specific variant at an already-identified call site); `ProfileInvalid` is blocked on a feature that doesn't exist yet.
**Verify:** Unit test per variant: trigger the real condition (e.g. force `SolverPort::solve` to return infeasible), assert the route/caller receives `DomainError::PlanInfeasible`, not a generic error.

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

### BL-28: `UserRequestMode` — user-facing request mode, above `CompletionPolicy`
**Req:** `entities/design_vocabulary.rs` (`UserRequestMode`); already documented conceptually in `docs/REQUIREMENTS.md` §3.2.1
**Problem:** REQUIREMENTS already documents the 6 modes (ASAP, ASAP_FREE, BY_DEADLINE, BY_DEADLINE_FREE, MAX_COST, OPPORTUNISTIC) as the intended vocabulary for *how* a user expressed an energy task — meant to sit above the live `CompletionPolicy` (which only governs what happens at deadline, not the request's cost/urgency posture). Nothing in code reads or sets a mode today; every user request implicitly behaves like a plain deadline/cost-aware request.
**Fix:** Add `mode: UserRequestMode` to `UserRequest`/`EvSession`/`HeaterTarget`/`ShiftableLoad` construction and have the MILP planner's session-intent translation (`controller/milp_planner`) branch on it (e.g. `OPPORTUNISTIC` → soft/no deadline constraint, only allocate when marginal cost is ~0).
**Complexity:** Medium–Large — six real behavioral variants to implement in the solver's constraint construction, not just a stored field.
**Verify:** Unit test per mode: given the same session parameters, each mode produces a distinguishably different solver constraint/allocation (at minimum, `OPPORTUNISTIC` vs. `ASAP` differ).

---

### BL-29: `FlexibilityDirection`, `RateType`, `RateUnit` — narrow supporting enums
**Req:** `entities/design_vocabulary.rs`
**Problem:** Three small enums with no current consumer. `RateUnit` overlaps with the live `RateUnit`-shaped fields already handled ad hoc as bare `f64`/currency-implicit values in `TariffSnapshot`; `RateType` (per-kWh vs. per-kW) and `FlexibilityDirection` (import/export) are classification vocabulary for capacity-rate handling and capacity-request direction respectively — relevant once BL-10 (`FlexibilityEnvelope` → VTN report) or BL-24 (`OadrCapacityRequest`) are actually implemented.
**Fix:** Don't implement standalone — fold each into whichever feature actually needs it when that feature is built: `RateType`/`RateUnit` into a future multi-currency/multi-unit tariff handling pass (no BL item yet — add one if/when multi-currency support is requested); `FlexibilityDirection` into BL-10's report-building work.
**Complexity:** N/A standalone — tracked here only so they're not forgotten, not as independent work items.
**Verify:** N/A until folded into a parent feature.

---

## General Backlog

| ID | Item | Priority |
|---|---|---|
| GB-01 | Clean up Docker orphan containers | Low |
| GB-02 | Unify VEN-1 naming scheme to match VEN-2/VEN-3 (causes test confusion) | Medium |
| GB-03 | Make VEN-1 ID a UUID and update all test/seed references | Medium |
| GB-04 | DB-level optimization: add `ends_at timestamptz` index so `?active=true` runs in SQL, not post-filter Rust | Low (not needed until event table is large) |
| GB-05 | VTN UI: filter past events from event table | Low |
| GB-06 | Add DB-reset script for easy re-seeding | Low |
| GB-07 | Add setup script to bring up all required containers | Low |
| GB-08 | Add VEN UI tests for UserRequests and Controller pages | Medium |
| GB-09 | Make VEN poll interval configurable per profile (useful for testing) | Medium |
| GB-10 | Remove remaining compiler warnings across all builds | Medium |

---

## Dependency Vulnerabilities — 2026-05-25

> Re-run `cargo audit` and `npm audit` before each release and add new findings here.

### Rust (cargo audit) — 10 vulnerabilities

| ID | Crate | Version | Severity | Title | Fix |
|----|-------|---------|----------|-------|-----|
| RUSTSEC-2026-0048 | aws-lc-sys | 0.37.0 | High (7.4) | CRL Distribution Point Scope Check Logic Error | Upgrade to ≥0.39.0 |
| RUSTSEC-2026-0047 | aws-lc-sys | 0.37.0 | High (7.5) | PKCS7_verify Signature Validation Bypass | Upgrade to ≥0.38.0 |
| RUSTSEC-2026-0046 | aws-lc-sys | 0.37.0 | High (7.5) | PKCS7_verify Certificate Chain Validation Bypass | Upgrade to ≥0.38.0 |
| RUSTSEC-2026-0045 | aws-lc-sys | 0.37.0 | Medium (5.9) | Timing Side-Channel in AES-CCM Tag Verification | Upgrade to ≥0.38.0 |
| RUSTSEC-2026-0044 | aws-lc-sys | 0.37.0 | — | X.509 Name Constraints Bypass via Wildcard/Unicode CN | Upgrade to ≥0.39.0 |
| RUSTSEC-2026-0037 | quinn-proto | 0.11.13 | High (8.7) | Denial of service in Quinn endpoints | Upgrade to ≥0.11.14 |
| RUSTSEC-2026-0099 | rustls-webpki | 0.103.9 | — | Name constraints accepted for wildcard certificate names | Upgrade to ≥0.103.12 |
| RUSTSEC-2026-0104 | rustls-webpki | 0.103.9 | — | Reachable panic in CRL parsing | Upgrade to ≥0.103.13 |
| RUSTSEC-2026-0049 | rustls-webpki | 0.103.9 | — | CRLs not authoritative due to faulty matching logic | Upgrade to ≥0.103.10 |
| RUSTSEC-2026-0098 | rustls-webpki | 0.103.9 | — | Name constraints for URI names incorrectly accepted | Upgrade to ≥0.103.12 |

**Dependency chain:** All via `reqwest` TLS stack — `aws-lc-rs` → `rustls` → `tokio-rustls` / `hyper-rustls` / `quinn`. Upgrading `reqwest` to a version that pins `aws-lc-sys ≥0.39.0` and `rustls-webpki ≥0.103.13` should resolve all 10.

**Risk context:** Lab/Pi4 deployment — not internet-exposed. VEN communicates only with local VTN. Real-world exploitability is low; fix before any internet-exposed deployment.

### npm — VEN/ui: 12 vulnerabilities (2 high)

| Package | Severity | Issue |
|---------|----------|-------|
| esbuild | High | Dev-server allows cross-origin requests |
| vite | Moderate | Transitive dep on vulnerable esbuild |
| (10 others) | Low–Moderate | Various transitive deps |

**Fix:** `cd VEN/ui && npm audit fix`. The high-severity issue is in the dev server only — not in production builds.

### npm — VTN/ui: 11 vulnerabilities (1 high)

| Package | Severity | Issue |
|---------|----------|-------|
| esbuild | High | Same dev-server issue as VEN/ui |
| (10 others) | Low–Moderate | Various transitive deps |

**Fix:** `cd VTN/ui && npm audit fix`.

### RUSTSEC warnings (unsound, not vulnerabilities)

| ID | Crate | Title |
|----|-------|-------|
| RUSTSEC-2026-0097 | rand 0.8.5, 0.9.2 | Unsound with custom logger calling `rand::rng()` |

**Risk:** Only triggered when a custom global logger calls `rand::rng()` — not applicable here. No action required.
