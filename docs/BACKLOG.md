## REQUIREMENTS.md and VEN_ARCHITECTURE.md: Requirements Gap Backlog (from 2026-03-21 code audit)

Items ordered by recommended implementation sequence: dependencies first, then by impact.

---

### BL-01: PlanTrigger wiring — RATE_CHANGE / CAPACITY_CHANGE
**Req:** UC-04, UC-07, VEN_ARCHITECTURE §2.1
**Problem:** `openadr_interface` parses new rates and capacity from VTN events, but never emits `PlanTrigger::RateChange` or `PlanTrigger::CapacityChange` on the watch channel. The planner only replans on the 20s periodic tick or on packet transitions — rate/capacity changes are invisible until the next periodic cycle.
**Fix:** After `parse_rate_snapshots()` / `parse_capacity_state()` in the poll loop (`main.rs:133-216`), compare new values against previous. If changed, send the appropriate `PlanTrigger` variant on the watch channel.
**Complexity:** Small (1–2 hours). Diff detection + channel send.
**Verify:** BDD test: seed a PRICE event mid-run, assert replan fires within one poll cycle (not 20s).

---

### BL-02: Event priority ordering before merge
**Req:** FR-OA-08
**Problem:** `openadr_interface.rs:186-195` merges events in array order (last-write-wins). A lower-priority event processed later silently overwrites a higher-priority one. The `priority` field is not read at all.
**Fix:** Extract `priority` (integer, lower = higher priority) and `createdDateTime` from each event. Sort events by ascending priority, then descending `createdDateTime` (newer breaks ties), before entering the merge loop.
**Complexity:** Small (1–2 hours). Sort + two field extractions.
**Verify:** Unit test: two PRICE events with same interval, different priorities — higher-priority value wins. Second test: same priority, newer event wins.

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
**Fix:** In `openadr_interface`, detect ALERT payload types. Create a high-priority synthetic `EnergyPacket` (shed/limit import) and emit `PlanTrigger::Alert`. Planner treats alert packets as highest-priority FIRM allocations.
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
**Problem:** These event types are not parsed in `openadr_interface`. `DISPATCH_SETPOINT` should bypass the planner and go directly to the dispatcher. `CHARGE_STATE_SETPOINT` should create/modify an EnergyPacket targeting the specified SOC.
**Fix:** Add parsing branches in `openadr_interface` for both types. `DISPATCH_SETPOINT` → store in `OadrEventCache.dispatch_setpoints` (field already exists in `capacity.rs:53`) and flag for dispatcher override. `CHARGE_STATE_SETPOINT` → create EnergyPacket with target SOC via user_request machinery.
**Complexity:** Medium (4–6 hours). Two new parsing paths + dispatcher override mode + packet creation.
**Verify:** BDD test: send DISPATCH_SETPOINT event, assert sim setpoint matches within one poll cycle. Send CHARGE_STATE_SETPOINT, assert EnergyPacket created with correct target.

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



---

## General Backlog

clean up docker orphans

ven-1 differs in naming scheme from othe VENs. this causes confusion and sometimes errors. can we unify them?

make the ven-1 id a uuid and change it in all test and seed references.

DB-level optimization for active event filter: add `ends_at timestamptz` computed column + index so the `?active=true` filter can run in SQL instead of post-filtering in Rust. Not needed until event tables grow large.


Add a filter in VTN UI event table to omit the past events.

Add a DB-Reset script so it can be re-seeded easily.


add a setup script that docker composes all required containers.


add code coverage tools to tests and formater and linter tools to be applied for each code change.


check and remove warnings in all builds.

check for code quality and refactoring possibilities.

write down all your findings to the test errors around VEN UI simulation tests into ven_ui_simulation_test_issues.md. 

The fix is there. Docker's layer cache is stale — it doesn't see the change to Simulation.tsx. Need to force a rebuild without cache


add time provider for simulation: 
pub trait TimeContext: Clone + Send + Sync + 'static {
    type Instant: Copy + Ord + Send + 'static;

    fn now(&self) -> Self::Instant;
    fn sleep_until(&self, deadline: Self::Instant) -> Pin<Box<dyn Future<Output = ()> + Send>>;
    fn sleep(&self, duration: Duration) -> Pin<Box<dyn Future<Output = ()> + Send>>;

    fn pause(&self);
    fn resume(&self);
    fn set_rate(&self, rate: f64);
    fn advance(&self, delta: Duration);
}


how can I test the ven controller in ui?


also add ui tests for UserRequests and Controller in VEN\ui\src\__tests__   


the ven poll interval should be configurable in the config file so during test we can easily shorten it. or is there a better option? 

reactor still there?

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
