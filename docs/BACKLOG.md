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

### BL-03: Exponential backoff on VTN communication failure — RESOLVED (Phase 2, WP2.1)
**Req:** FR-OA-07
**Problem:** All poll loops (`main.rs:101-298`) use fixed `tokio::time::interval`. On VTN failure, VEN retries every 30s indefinitely — no backoff, no jitter.
**Fix:** Replace fixed interval with adaptive delay: on success reset to 30s; on failure double delay (30s → 60s → 120s → 240s → 480s → max 900s). Add ±10% jitter. On success, reset immediately.
**Resolution:** `VEN/src/tasks/backoff.rs` (`Backoff`, seeded RNG for deterministic tests) wired into `poll_programs.rs`/`poll_events.rs`/`poll_reports.rs`. Verified against a live Pi4 stack: `tests/features/ven_resilience.feature`'s new scenario stops the VTN for 130s, asserts growing gaps between consecutive poll-failure log timestamps, then restarts and confirms recovery. One real finding during that verification: recovery latency after a *sustained* outage is bounded by whatever backoff delay was already in flight when the VTN comes back (up to ~130s here), not instant — the reset to the base interval only takes effect on the *next* successful poll. That's the deliberate trade-off (never hammering a still-recovering VTN), documented in the feature file rather than tightened away.

---

### BL-04: ALERT_GRID_EMERGENCY handling — RESOLVED (Phase 3, WP3.1)
**Req:** UC-06, OA-01
**Problem:** `ALERT_GRID_EMERGENCY` and `ALERT_BLACK_START` event types are not parsed. Emergency signals from the VTN are silently ignored.
**Fix:** In `openadr_interface`, detect ALERT payload types and emit `PlanTrigger::Alert`. Planner enforces a zero/minimal import hard constraint for the alert duration as highest-priority FIRM slots.
**Complexity:** Medium (3–5 hours). New parsing path + synthetic packet creation + planner priority handling.
**Verify:** BDD test: send ALERT_GRID_EMERGENCY event, assert planner creates shed packet and reduces import within one poll cycle.
**Resolution:** `parse_alert_windows` (openadr_interface.rs) extracts both alert types (interval-level window, event-level fallback per User Guide Example 8.1-1); `PlanTrigger::Alert` fires on change; `build_milp_inputs` clamps the per-slot contractual import cap to 0 over the window (soft constraint — unavoidable base load becomes a penalized/warned violation, never infeasibility; user deadlines yield automatically). Not "highest-priority FIRM slots + synthetic packet" as sketched here — the per-slot cap on the existing constraint path achieves the shed without new packet machinery. `ven_alerts.feature` verifies clamp + recovery-on-delete live.

---

### BL-05: Obligation-triggered report submission — RESOLVED (found already done during Phase 3, WP3.5)
**Req:** FR-OA-04
**Problem:** `main.rs:506-512` checks `due_obligations(now)` and marks them `fulfilled`, but does **not** build or submit reports. Reports are only sent on timer (`report_interval_s`) and packet transitions — not when obligations actually become due.
**Fix:** In the obligation check loop, when `due_obligations` returns non-empty: call `build_measurement_reports_for_active_events()` for each due obligation, submit via `upsert_report()`, then mark fulfilled.
**Resolution:** Discovered already implemented when Phase 3's WP3.5 came up — this landed as part of the 2026-07-06 R6 review-item resolution (obligation recurrence), after the roadmap doc was written. `ObligationService::check_and_report` (`services/obligation.rs`, driven by the 5s `tasks/obligation.rs` tick) builds a per-obligation measurement report via `build_measurement_report_for_obligation` and submits via `upsert_report`, re-arming `due_at` on success and leaving it unchanged on VTN error (natural retry next tick). Unit-tested (rearm-not-remove, error-leaves-due_at, no-history skip) and BDD-covered by `reporter_resampling.feature`'s obligation-driven scenario (5-second `reportDescriptor` frequency — precisely this entry's verify clause). No new work was needed in Phase 3.

---

### BL-06: DISPATCH_SETPOINT + CHARGE_STATE_SETPOINT parsing — RESOLVED (Phase 3, WP3.4)
**Req:** UC-13, VEN_ARCHITECTURE §2.1
**Problem:** These event types are not parsed in `openadr_interface`. `DISPATCH_SETPOINT` should bypass the planner and go directly to the dispatcher. `CHARGE_STATE_SETPOINT` should create/modify an `EvSession` targeting the specified SoC.
**Fix:** Add parsing branches in `openadr_interface` for both types. `DISPATCH_SETPOINT` → store in `OadrEventCache.dispatch_setpoints` (field already exists in `capacity.rs:53`) and flag for dispatcher override. `CHARGE_STATE_SETPOINT` → create `EvSession` with target SoC via `user_request` machinery.
**Complexity:** Medium (4–6 hours). Two new parsing paths + dispatcher override mode + session creation.
**Verify:** BDD test: send DISPATCH_SETPOINT event, assert sim setpoint matches within one poll cycle. Send CHARGE_STATE_SETPOINT, assert `EvSession` created with correct target SoC.
**Resolution:** Both parsed in openadr_interface.rs. DISPATCH_SETPOINT feeds typed `DispatchWindow` state (not `OadrEventCache.dispatch_setpoints` — that stays an unwired sketch, see BL-24); `apply_dispatch_override` (sim_tick/helpers.rs) steers the battery to hit the commanded net site power while the window is active, clamped to live capability, with the plan running underneath. Alert wins over dispatch (recorded precedence decision). `ControllerEvent::DispatchOverride` traces transitions. CHARGE_STATE_SETPOINT creates/updates an EvSession (fraction or percent value; window end = departure); deleting the event cancels the event-created session — and only that one. `ven_dispatch_setpoints.feature` covers both live.

---

### BL-07: StaleRatePolicy dispatch in planner
**Req:** UC-12, REQUIREMENTS §3.2.1
**Problem:** `StaleRatePolicy` enum is defined (`asset.rs:109-114`) with 4 variants (LAST_KNOWN, HEURISTIC_FORECAST, DEFER_TO_FLEXIBLE, SAFE_AVERAGE), but the planner has no dispatch logic. When VTN is unreachable, slots beyond the last known tariff get no special treatment.
**Fix:** In planner Phase 1 (`build_grid`), after populating tariff data, detect slots with no rate coverage. Apply the configured `StaleRatePolicy`: LAST_KNOWN → repeat last value; DEFER_TO_FLEXIBLE → mark those slots FLEXIBLE regardless of horizon; SAFE_AVERAGE → use configurable percentile tariff.
**Complexity:** Medium (3–4 hours). Policy dispatch + per-slot fallback logic.
**Verify:** Unit test: planner with rates covering only 2h of a 6h horizon, each policy variant produces different slot classifications and costs.
**Resolution (Phase 4, WP4.4):** `TariffTimeSeries` tracks `import_coverage_end`; `build_milp_inputs` routes the per-slot import price through `milp_planner/stale_rates.rs`. LAST_KNOWN repeats, SAFE_AVERAGE takes the `stale_rate_safe_pctl` nearest-rank percentile (default 0.8), DEFER_TO_FLEXIBLE prices stale slots at the max known rate (the LP analogue of forcing FLEXIBLE), HEURISTIC_FORECAST (default) is a documented stub → LAST_KNOWN until Phase 5 BL-14 and says so in the warning. Stale slots set `PlanTimeSlot.rate_estimated`; a stable-text plan warning flows to the WP4.3 feed as exactly one notification. 7 unit tests incl. the verify clause.

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

### BL-10: FlexibilityEnvelope → VTN report — RESOLVED (verified Phase 3, WP3.6)
**Req:** UC-05, UC-07
**Problem:** Planner builds `FlexibilityEnvelope` (Phase 7) and exposes via `GET /flexibility`, but never submits them to the VTN as `IMPORT_CAPACITY_RESERVATION` / `EXPORT_CAPACITY_RESERVATION` reports. Aggregators cannot see available DR capacity.
**Fix:** In the report submission loop, when a new plan is produced with non-empty envelopes, build report payloads of type `IMPORT_CAPACITY_RESERVATION` / `EXPORT_CAPACITY_RESERVATION` from the envelope data and submit to VTN.
**Complexity:** Medium (3–5 hours). Report payload construction from envelope fields + submission wiring.
**Verify:** BDD test: planner produces envelopes for FLEXIBLE packets, assert VTN receives capacity reservation report with matching power/energy values.
**Resolution:** The builder arms already existed (descriptor-driven: `build_measurement_report_for_obligation` serves IMPORT/EXPORT_CAPACITY_RESERVATION payload types from the site envelope) — what was missing was verification, not implementation. `ven_reporting_out.feature` now proves a VTN with a reservation reportDescriptor receives envelope-valued reports. Note: descriptor-driven, not submitted unsolicited on every plan as this entry sketched.

---

### BL-11: Time-weighted tariff averaging for planner slot costing
**Req:** VEN_ARCHITECTURE §5.3
**Problem:** Planner evaluates tariff at `slot.start` only. A 5-min slot straddling a tariff boundary (e.g., €0.20 → €0.15 at 10:57) uses only the first tariff, ignoring the 3 min at the cheaper rate.
**Fix:** Replace `tariff_at(slot.start)` with `Σ(tariff_i × overlap(slot, interval_i)) / slot.duration` using the existing `TimeSeries` abstraction. For capacity: `min(capacity_i for all overlapping intervals)`.
**Complexity:** Small–Medium (2–3 hours). Use existing TimeSeries infrastructure.
**Verify:** Unit test: 10-min slot spanning tariff boundary at minute 7 → weighted average matches `(7*0.20 + 3*0.15)/10 = 0.185`.

---

### BL-12: EV minimum charge rate + response delay model — RESOLVED (Phase 0, WP0.3)
**Req:** FR-SIM-05
**Problem:** EV asset has no 1.5kW minimum active charge rate floor. Setpoints between 0 and 1.5kW are accepted (should snap to 0 or 1.5kW). 10s response delay not modeled — setpoints apply instantly.
**Fix:** In `assets/ev.rs` update logic: if `0 < setpoint < min_charge_kw`, snap to 0. Add single-step lag buffer: store commanded setpoint, apply previous tick's command (simulating 10s delay at 10s tick or interpolated at 1s tick).
**Complexity:** Small (1–2 hours).
**Verify:** Unit test: setpoint 0.5kW → actual power 0. Setpoint 7kW at t=0 → actual power still 0 at t=0, becomes 7kW at t=10s.
**Resolution:** `snap_to_min_charge` (pure function) enforces the floor (kept 1.4kW default —
the existing MILP-side default — rather than 1.5kW, so no profile edits needed). Response
delay implemented as a literal single-*tick* lag buffer (`EvState.pending_command_kw`), not
a 10-second timer: all profiles run `tick_s: 1`, so a true 10s buffer would need a multi-tick
queue. Deferred a duration-based version — track if a profile ever sets `tick_s` close to
`response_delay_s`'s 10s default, where a 1-tick lag would under-model the delay. 3 new unit
tests; see project_journal.md.

---

### BL-13: Early firm-up heuristic
**Req:** VEN_ARCHITECTURE §2.3
**Problem:** Spec says if rate variance across FLEXIBLE window is < 10% (flat rate), FLEXIBLE slots may firm up early. Code comment at `planner.rs:271` acknowledges this but it's not implemented.
**Fix:** After Phase 7, compute variance of tariff across all FLEXIBLE slots. If coefficient of variation < 0.10, reclassify FLEXIBLE → FIRM and re-run allocation (Phases 2–5) for those slots.
**Complexity:** Small (1–2 hours). Statistical check + slot reclassification.
**Verify:** Unit test: flat-rate tariff (all €0.15) → all slots classified FIRM. Variable tariff (€0.10–€0.30) → FLEXIBLE slots remain FLEXIBLE.

---

### BL-30: Show past behaviour ("log for past") in VEN UI — RESOLVED (Phase 1, WP1.5)
**Req:** `docs/plans/strategic_roadmap.md` §9 (numbered here per its own suggestion)
**Problem:** the VEN UI had no way to browse historical power/cost/CO2, received events, or sent reports beyond the live in-memory view.
**Fix:** a thin read-only view over A-1 (BL-31, the persistent history store) — see WP1.5.
**Resolution:** `VEN/ui/src/pages/History.tsx` — date picker (defaults to yesterday), reused `AssetTimelineChart`/`TariffChart` fed from `/history/ticks`/`/history/grid`, plain tables for events received/reports sent. Verified in a real browser via a new `@ven-ui` Playwright scenario.

---

### BL-14: AssetHeuristics — learned behavioral profile for uncontrollable assets
**Req:** entities/design_vocabulary.rs §3.3 (`AssetHeuristics`)
**Problem:** `AssetHeuristics` (24-entry `daytime_profile_kw`, 7-entry `weekday_weights`, `seasonal_factor`) is defined but never populated or read. Uncontrollable/implicit loads (base load, PV with no weather feed) have no learned-pattern fallback — the planner has nothing better than flat/last-known extrapolation for them.
**Fix:** Add a background job that aggregates persisted per-asset history into `daytime_profile_kw`/`weekday_weights`/`seasonal_factor` on a rolling basis; feed the result into `AssetForecast` (BL-15) as the `ForecastSource::Heuristic` source for assets without a physical or weather model.
**Complexity:** Large. New aggregation job + persistence + planner consumption path.
**Verify:** TBD once designed — needs a fixture with multi-week synthetic history to assert the learned profile converges to the injected pattern.

---

### BL-15: AssetForecast — per-asset predicted power profile — RESOLVED (Phase 3, WP3.6)
**Req:** entities/design_vocabulary.rs §3.6 (`AssetForecast`, `ForecastSource`, `TimeRange`)
**Problem:** `AssetForecast` (per-step predicted power/SoC, confidence, availability windows, tagged by `ForecastSource`) is defined but nothing constructs it. The MILP planner computes an equivalent per-slot forecast internally (`planned_state_by_asset`) but never exposes it in this shape, and it's also the missing piece behind the never-built outbound `USAGE_FORECAST` report (see `docs/reference/TECHNICAL_DEBTS.md` R-15).
**Fix:** Build `AssetForecast` from the planner's internal per-slot state after each plan cycle; expose via a route and use it as the source for the `USAGE_FORECAST` report (R-15).
**Complexity:** Medium — mostly plumbing existing planner output into the documented shape, plus BL-14's heuristic source once that exists.
**Verify:** Unit test: after a plan cycle, `AssetForecast.power_kw` matches the planner's `planned_state_by_asset` for the same asset/horizon.
**Resolution:** `services/forecast.rs` builds one `AssetForecast` per asset from every adopted plan (`ForecastSource::Optimization`, new enum variant), served at `GET /forecast`; the `USAGE_FORECAST` report (R-15's other half) is built directly from plan slots at their native boundaries, descriptor-driven via the obligation machinery. BL-14's Heuristic source remains future work.

---

### BL-16: AssetLedger — per-asset billing-period cost/CO2 ledger — RESOLVED (Phase 1, WP1.6)
**Req:** entities/design_vocabulary.rs §3.7 (`AssetLedger`)
**Problem:** `monitor::record_tick` accumulates tick-level cost/CO2 into the asset ledger already (correcting an earlier mis-attribution to the dispatcher — see R5 finding), but there is no `AssetLedger`-shaped per-asset, per-billing-period rollup with defined `period_start`/`period_end` and reset semantics. Nothing constructs or periodically resets one today.
**Fix:** Wire `AssetLedger` as the billing-period aggregation layer on top of `monitor::record_tick`'s tick-level accumulation; add a period-rollover trigger (e.g. monthly) that closes the current ledger and opens the next.
**Complexity:** Medium–Large. Needs period-boundary logic and persistence across restarts.
**Verify:** Unit test: ledger accumulates across ticks within a period, resets exactly at `period_end`, and totals reconcile against `monitor::record_tick`'s raw sums.
**Resolution:** `tasks/history_sampler/mod.rs`'s existing 1s loop gained a
`month_boundary_crossed()` check (pure, clock-injected, distinct from the day-pruning
check — does NOT fire on the very first call, since the live ledger survives restarts
via `state.json` and must not be closed just because the process happened to restart
mid-month). On a real crossing, `close_ledger_period()` snapshots `AppState`'s existing
`AssetLedgerEntry` map into `HistoryPort::append_ledger_period` rows (the `ledger_periods`
table from WP1.1's schema v1), then resets the live ledger via the existing
`set_asset_ledger()`. `GET /ledger?asset_id=` now returns `{ current, closed_periods }`
for one asset (unchanged shape when `asset_id` is omitted — the existing Dashboard
`LedgerCard` consumer is unaffected). No new UI needed: that same `LedgerCard` already
shows per-asset current-period cost/energy/CO2 with a "running since" label, which now
correctly reflects the current billing period after each rollover.

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
**Resolution (Phase 4, WP4.2):** `POST/GET/DELETE /assets/:id/comfort_curve` (wire shape = domain `ComfortRate`, DTO passthrough; validation: non-empty, finite, strictly increasing fill in [0,1], bids ≤ 10 €/kWh). Persisted via the new sibling `SettingsPort` (`user_settings` table, schema v3) on the Phase-1 SQLite store; hot map on `AppState` re-seeded at startup. The override beats `default_comfort_rates()` at the `AssetRequestSlice` build (verify clause: `services/comfort.rs` tests). **Caveat found during implementation:** the "existing consumption path" this entry assumed is thinner than believed — `create_from_body` resolves the curve but then drops it (`_comfort_rates`), so no curve (default or override) reaches MILP constraints today; translating curves into solver tier constraints remains open (noted here rather than silently absorbed). UI: curve editor card on the Devices page. BDD: `ven_comfort_curve.feature`.

---

### BL-20: UserNotificationSeverity — user-facing notification feed
**Req:** entities/design_vocabulary.rs (`UserNotificationSeverity`, doc comment: "used in Stage 5")
**Problem:** No notification concept exists anywhere in the VEN today — no queue, no route, no UI surface. `UserNotificationSeverity` (Info/Warn/Alert) is the only trace of the intended feature, sketched but with nothing to attach severities to yet.
**Fix:** Design a minimal notification event (message + severity + timestamp + optional asset/event reference), a bounded in-memory feed (similar shape to the existing `/trace/events` ring buffer), and a route to poll or stream it; wire initial producers at natural trigger points (tier fallback, budget warning, deadline approaching, packet abandoned, grid emergency — per the enum's own doc comments).
**Complexity:** Medium. New cross-cutting concept — needs a decision on where in the architecture it's produced from before implementation.
**Verify:** TBD pending design — at minimum, a test asserting a triggered condition (e.g. tier fallback) produces a notification of the expected severity.
**Resolution (Phase 4, WP4.3):** `UserNotification` entity + bounded ring on `AppState` (cap 200) + `notifications` table (schema v2) so the feed survives restarts (ring re-seeded at startup). Application-layer `Notifier` (`services/notify.rs`) fans out to ring + SSE broadcast + store. Producers wired: grid-emergency alert windows (Alert, once per window), VTN reachability edges (Warn/Info, never per-poll), and newly-appearing warnings on an *adopted* plan (Warning→Warn, Critical→Alert, Info suppressed) — the plan-warning channel automatically carries WP4.4's stale-rate and WP4.1-c's MAX_COST budget warnings, with dedup keyed on stable warning text. Routes `GET /notifications?since=` + `/notifications/events` (SSE); UI bell + feed panel. Producers this entry names that are NOT yet wired: tier fallback / deadline-at-risk / packet abandoned (the Stage-5 tier machinery itself is still BL-09-adjacent future work).

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
**Problem:** Three unwired sketches in a file that otherwise holds live types (`OadrCapacityState`, `OadrReportObligation`). `OadrEventCache.dispatch_setpoints` is specifically the storage `TECHNICAL_DEBTS.md` R-13 (`DISPATCH_SETPOINT` parsing) would need once that's built; `OadrProgramConfig` and `OadrCapacityRequest` have no consumer at all yet — no code path builds or sends a capacity reservation request to the VTN in this shape.
**Fix:** For `OadrEventCache`: build alongside R-13's `DISPATCH_SETPOINT` parsing work (BL-06 already covers the parsing side). For `OadrProgramConfig`/`OadrCapacityRequest`: no dependent feature identified yet — lowest priority of this group until one exists.
**Update (Phase 3, WP3.4):** DISPATCH_SETPOINT parsing landed WITHOUT `OadrEventCache` — typed `DispatchWindow` state (matching the alert/SIMPLE pattern) proved the better shape, so the cache''s anticipated consumer no longer exists. All three sketches remain unwired; consider removal next time this file is triaged.
**Complexity:** Tied to BL-06/R-13 for the event cache; TBD for the other two pending a concrete driving feature.
**Verify:** Tied to whichever consuming feature lands first.

---

### BL-25: Reserved `DomainError` variants — wire at real boundaries — 2 of 3 RESOLVED (Phase 2, WP2.3)
**Req:** `entities/error.rs` (`DomainError::{PlanInfeasible, VtnUnreachable, ProfileInvalid}`)
**Problem:** All three are constructed only inside their own `Display`-format unit test — never at an actual error boundary in the running application.
**Fix:** `PlanInfeasible` — return from the planner's solve path when `SolverPort::solve` reports infeasibility, surfaced through the relevant route instead of a generic error. `VtnUnreachable` — classify repeated VTN-client timeouts distinctly from other request failures. `ProfileInvalid` — only applicable if profile hot-reload (not just startup validation) is ever built; until then this variant stays reserved with no natural call site.
**Resolution:** Both variants are now constructed at real boundaries, but as **structured log lines, not propagated errors** — the original "surfaced through the relevant route instead of a generic error" framing didn't match the code once investigated: `SolverPort::solve` is deliberately infallible (always returns a usable `Plan`, falling back with a `PlanWarning` on solver failure — see `solver_port.rs`'s own doc comment), so there was never a route-level 500 to replace for `PlanInfeasible`; it's now logged in `milp_planner::run_planner`'s existing fallback branch. `VtnUnreachable` is classified in `vtn.rs` from a connect/timeout-class `reqwest::Error` at every `send()` call site and logged for fleet debugging, without changing `VtnPort`'s `Result<T, anyhow::Error>` contract. `ProfileInvalid` stays reserved (no hot-reload feature exists).

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
**Resolution (Phase 4, WP4.1 a–c):** `mode: UserRequestMode` on `UserRequest`/`EvSession`/`HeaterTarget`/`ShiftableLoad` (serde default BY_DEADLINE = legacy behaviour; routes + UI accept it). MILP semantics land on the EV path (the phase plan's canonical carrier): ASAP = lateness penalty (`asap_lateness_eur_kwh_h`, 10 €/kWh·h) → cost-blind front-loading; OPPORTUNISTIC/ASAP_FREE/BY_DEADLINE_FREE = per-slot free-energy cap (PV surplus / non-positive tariff, via the new `AssetMilpContext::inject_grid_slots` hook) + per-charged-kWh reward (`v_ev_free_charge_eur_kwh`), ASAP_FREE with an early-slot bias, BY_DEADLINE_FREE under the deadline mask; MAX_COST = hard budget constraint on charging cost (session `budget_eur`) with per-kWh completion reward — unaffordable targets degrade to partial charging + a warning/notification, never an infeasible solve. 10 planner unit tests incl. `test_mode_asap_vs_opportunistic_allocations_differ`; BDD `ven_request_modes.feature`. Modes on heater/shiftable sessions are stored but planner-inert for now (documented). Side discovery: the legacy `e_ev_extra` reward is structurally inert → R-18 in TECHNICAL_DEBTS.md.

---

### BL-29: `FlexibilityDirection`, `RateType`, `RateUnit` — narrow supporting enums
**Req:** `entities/design_vocabulary.rs`
**Problem:** Three small enums with no current consumer. `RateUnit` overlaps with the live `RateUnit`-shaped fields already handled ad hoc as bare `f64`/currency-implicit values in `TariffSnapshot`; `RateType` (per-kWh vs. per-kW) and `FlexibilityDirection` (import/export) are classification vocabulary for capacity-rate handling and capacity-request direction respectively — relevant once BL-10 (`FlexibilityEnvelope` → VTN report) or BL-24 (`OadrCapacityRequest`) are actually implemented.
**Fix:** Don't implement standalone — fold each into whichever feature actually needs it when that feature is built: `RateType`/`RateUnit` into a future multi-currency/multi-unit tariff handling pass (no BL item yet — add one if/when multi-currency support is requested); `FlexibilityDirection` into BL-10's report-building work.
**Complexity:** N/A standalone — tracked here only so they're not forgotten, not as independent work items.
**Verify:** N/A until folded into a parent feature.

---

### BL-31: A-1 — Persistent VEN history store — RESOLVED (Phase 1, WP1.1–1.6)
**Req:** `docs/plans/roadmap/phase-1-data-foundation.md`
**Problem:** the VEN had no persistent history beyond process lifetime — only in-memory ring buffers.
**Fix:** `HistoryPort` trait + SQLite adapter (rusqlite, bundled), a 1-minute-mean downsampling sampler task, daily retention pruning, `GET /history/*` routes, a VEN UI History page, and monthly `AssetLedger` billing-period rollover (BL-16).
**Verify:** see the WP1.1–WP1.6 project journal entries; full E2E green on Pi4 after each WP.

### BL-32: A-2 — VTN recorder in the BFF — RESOLVED (Phase 1, WP1.7)
**Req:** `docs/plans/roadmap/phase-1-data-foundation.md`
**Problem:** nothing archived VTN-side reports/events/VEN health beyond openleadr-rs's own live tables — no historical record survives program/event deletion or VTN restarts.
**Fix:** a background poll task in `VTN/bff` (new `recorder.rs`), gated on `DATABASE_URL`, pages through `/reports`/`/events`/`/vens` via the existing `skip`/`limit` support, dedupes on `(id, modificationDateTime)` via a composite PK + `ON CONFLICT DO NOTHING`, and writes into a new `lab_recorder` Postgres schema in the *same* instance openleadr-rs already uses — never touching its own tables.
**Verify:** see the WP1.7 project journal entry; confirmed via a real Pi4 run (publish a program/event/report, poll interval elapses, rows appear in `lab_recorder.*` via `psql`).

### BL-33: A-3 — experiment harness + KPI jobs — RESOLVED (Phase 3, WP3.8; exit demo pending a scheduled window)
**Req:** `docs/plans/roadmap/phase-3-control-method-lab.md`
**Problem:** no scripted way to compare the control methods (price, capacity, alert, SIMPLE, dispatch) on KPIs — every comparison was manual.
**Fix:** `experiments/` — declarative scenario YAMLs (S-1 flat baseline … S-6 combined), `run_experiment.py` (drives the VTN API per scenario, snapshots VEN SQLite stores WAL-aware + `lab_recorder` CSVs), `kpi.py` (energy/cost/peak/load-factor/energy-shifted-vs-S1/report-timeliness per VEN), `report.py` (markdown + optional matplotlib charts).
**Constraint found (sim-time spike):** the sim clock is wall time (`tick_once` stamps `Utc::now()`, event windows are absolute), so time acceleration isn't externally drivable without an injectable clock through the whole tick/poll path — scenarios run in real time; S-1…S-6 are 30-minute same-day windows (~3 h for the full set).
**Verify:** 3-minute `smoke.yaml` run on Pi4 exercised the full pipeline (event replay, cleanup, WAL-aware snapshot, KPI extraction with real per-VEN values, report rendering). The full S-1…S-6 exit demonstration runs as a scheduled window (same deferral rationale as Phase 2's N=10).

---

## General Backlog

| ID | Item | Priority |
|---|---|---|
| GB-01 | Clean up Docker orphan containers | Low |
| GB-02 | Unify VEN-1 naming scheme to match VEN-2/VEN-3 (causes test confusion) | Medium |
| GB-03 | Make VEN-1 ID a UUID and update all test/seed references | Medium |
| GB-04 | DB-level optimization: add `ends_at timestamptz` index so `?active=true` runs in SQL, not post-filter Rust | Low (not needed until event table is large) |
| GB-05 | VTN UI: filter past events from event table | Low |
| GB-06 | Add DB-reset script for easy re-seeding — RESOLVED (Phase 2, WP2.5: `scripts/db_reset.sh`) | Low |
| GB-07 | Add setup script to bring up all required containers | Low |
| GB-08 | Add VEN UI tests for UserRequests and Controller pages | Medium |
| GB-09 | Make VEN poll interval configurable per profile (useful for testing) — PARTIALLY RESOLVED (Phase 2, WP2.5): the actual goal ("N VENs don't poll in lockstep") is met via a one-time `POLL_STARTUP_JITTER_S` stagger, not a per-profile interval override — simpler and lower-risk than moving poll intervals from env vars into the profile schema, which nothing currently needs. | Medium |
| GB-10 | Remove remaining compiler warnings across all builds | Medium |
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
