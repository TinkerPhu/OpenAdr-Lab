## REQUIREMENTS.md and VEN_ARCHITECTURE.md: Requirements Gap Backlog

Open items only — resolved entries are removed (their resolution notes live in
`docs/history/project_journal.md` and git history). IDs are stable and never
reused; gaps in the numbering are removed items. BL-14 through BL-29 originate
from the dead-code vocabulary review (types quarantined in
`entities/design_vocabulary.rs`, not deleted). This file still needs a proper
re-sort/prioritization pass to decide actual implementation order.

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

### BL-25: Reserved `DomainError::ProfileInvalid` — wire if profile hot-reload is built
**Req:** `entities/error.rs` (`DomainError::ProfileInvalid`)
**Problem:** `ProfileInvalid` is constructed only inside its own `Display`-format unit test. It is only applicable if profile hot-reload (not just startup validation) is ever built; until then this variant stays reserved with no natural call site. (`PlanInfeasible` and `VtnUnreachable` are constructed at real boundaries as structured log lines.)
**Fix:** Wire when hot-reload exists; otherwise leave reserved.
**Complexity:** N/A until the driving feature exists.
**Verify:** Tied to the hot-reload feature.

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
