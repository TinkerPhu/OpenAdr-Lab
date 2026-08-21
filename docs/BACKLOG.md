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

### VEN user (site operator) — forecast accuracy

| ID | What the user gets | Gain | Effort | Risk |
|---|---|---|---|---|

### VEN user (site operator) — comfort, control & trust

| ID | What the user gets | Gain | Effort | Risk |
|---|---|---|---|---|
| [BL-18](#bl-18-assetflexibility--real-time-per-asset-flexibility-snapshot) | A live "how much can this device flex right now" widget, per asset instead of whole-site | Low-Medium | M (scope TBD) | Low — but needs a design decision (superseded by `FlexibilityEnvelope`?) before scoping |
| [BL-35](#bl-35-notification-producers-for-tier-fallback--deadline-at-risk--packet-abandoned) | Gets warned *before* a tier fallback / missed deadline / abandoned session, not after | Low | S (once BL-09 lands) | Low — blocked on BL-09's tier machinery existing |

### VTN user (aggregator / program operator)

| ID | What the user gets | Gain | Effort | Risk |
|---|---|---|---|---|

### No direct user value — internal cleanup/consistency (do opportunistically, don't prioritize)

| ID | Note | Gain |
|---|---|---|
| [BL-23](#bl-23-hvacservice--route-wiring-or-removal-of-the-unused-impl) | Consistency-only decision, no behavior change either way | None |
| [BL-29](#bl-29-flexibilitydirection-ratetype-rateunit--narrow-supporting-enums) | No standalone value — fold into whichever future feature needs each enum | None |
| [GB-11](#general-backlog) | Process/docs alignment items, not user-facing | Low |

---

### BL-18: AssetFlexibility — real-time per-asset flexibility snapshot
**Req:** entities/design_vocabulary.rs §3.5 (`AssetFlexibility`)
**Problem:** `AssetFlexibility` sketches an on-demand "how much can this asset flex right now" snapshot (`can_increase/decrease_consumption/production_kw`), computed per-asset rather than for the whole site. This is distinct from `FlexibilityEnvelope`, which is planner-produced, horizon-wide, and already reported to the VTN — `AssetFlexibility` would be the instantaneous, single-asset building block.
**Fix:** Decide first whether this is still wanted as a separate real-time endpoint (e.g. for a live UI widget) or fully superseded by `FlexibilityEnvelope`; if wanted, compute it on demand from each asset's current state and `PowerRange`/`ThermalModelParams` limits, no persistence needed.
**Gain:** Low-Medium — a real-time per-asset flex widget is a nice-to-have, but its incremental value over the already-shipped `FlexibilityEnvelope` is unclear until the design question is resolved.
**Complexity:** Medium, but scope depends on the design decision above — resolve that first.
**Verify:** TBD pending scope decision.

---

### BL-23: `HvacService` — route wiring or removal of the unused impl
**Req:** `services/hems.rs` (`HvacService`)
**Problem:** `EvSessionService` is the live pattern for session lifecycle; `HvacService` sketches the same shape for heater targets, but `post_heater_target` sets the target directly instead of going through it — so `HvacService`'s methods are never called.
**Fix:** Either route `post_heater_target` through `HvacService` for consistency with the EV path, or fold whatever `HvacService` was meant to add into the existing direct path and delete the empty shell.
**Gain:** None (cleanup only) — consistency decision, no behavior change either way.
**Complexity:** Small — this is a consistency decision, not new functionality.
**Verify:** `cargo build` clean; existing heater-target route tests unaffected either way.

---

### BL-29: `FlexibilityDirection`, `RateType`, `RateUnit` — narrow supporting enums
**Req:** `entities/design_vocabulary.rs`
**Problem:** Three small enums with no current consumer. `RateUnit` overlaps with the live `RateUnit`-shaped fields already handled ad hoc as bare `f64`/currency-implicit values in `TariffSnapshot`; `RateType` (per-kWh vs. per-kW) and `FlexibilityDirection` (import/export) are classification vocabulary for capacity-rate handling and capacity-request direction respectively — relevant once envelope reporting is extended or a capacity-reservation-request workflow to the VTN is actually built (the earlier `OadrCapacityRequest` sketch tracking that, BL-24, was removed as dead code — no dependent feature ever appeared).
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
| GB-11 | Remaining AI-SW-Development alignment items (from the retired root alignment-plan.md, Pass 3): backlog-handling + tool-installation + archive-folder notes in CLAUDE.md; USER_STORIES.md; RISK_ANALYSIS.md; PROMPT_LIBRARY.md; changelog decision (journal-as-changelog note); security-review cadence; automated code-review hook; file-header descriptions on key VEN modules | Low | Low |
| GB-12 | BDD scenario for `Plan.solve_status == Infeasible` on `/plan`/`/plan/events`. Unit-level coverage exists (`run_planner_infeasible_constraints_fallback_no_panic` plus new solve_status assertions); no BDD scenario forces an infeasible solve today because doing so needs a fixture heavier than the existing `InfeasibleBatCtx` test double, which isn't exposed at the BDD/E2E layer | Low | Low |
| GB-15 | The VTN's `/vens` list has no `ven-1` entry — only `ven-1-name`, an old provisioning typo predating the Node2 fleet work (found 2026-08-02 while running `seed_vtn.py`). The stale name also breaks the "Summer Peak DR" demo program's target update (targets `["ven-2", "ven-1-name"]`, 400 on re-seed). Rename the VEN entity to `ven-1` and fix the program's targets | Low — cosmetic/demo-data correctness, `ven-1` itself works fine under its real `CLIENT_ID` | Low |
| GB-35 | GB-22's own "audit other `@ven-ui`/browser scenarios for the same gap" reminder, narrowed: a full `features/` tree audit (2026-08-16, closing GB-22) found the browser+real-backend-poll race pattern concretely limited to the scenarios GB-22 itself fixed, but also surfaced a broader, lower-priority class not addressed there — backend-only scenarios with long (90–300s) `poll_until` calls that share the same host-load sensitivity mechanism without the browser-page combo that caused GB-22's actual reported failures: `features/controller/05_ev_charging_scenarios.feature` (all 4 scenarios, `dispatcher_steps.py` timeouts up to 90s) plus other long-timeout call sites (`alerts_steps.py`, `ev_charging_steps.py`, `uc_steps.py`, `planner_steps.py` at 300s; `request_modes_steps.py`, `comfort_steps.py` at 180s). None have failed under contention yet (unlike the GB-22 instances), so isolating all of them pre-emptively would bloat the isolated pass without evidence; this item exists so the reminder isn't lost, not to isolate all of them by default | Low — speculative, no confirmed failures yet, same mechanism as GB-22 | Low — review each site if/when it's observed flaking, same "tag `@isolated` or raise the timeout" fix shape as GB-22 |
| GB-38 | Three VENs' MILP hit the solver time limit on essentially every solve for a whole 24h run and consequently never charged their EVs at all, despite valid active sessions and 25-30% SoC (`docs/history/fleet_run_journal.md`, S-9 re-run 2026-08-20/21): ven-12 TIME_LIMIT on 1419/1419 solves (avg 63 s), ven-3 1363 TIME_LIMIT + 56 INFEASIBLE (avg 112 s), ven-5 1237 TIME_LIMIT + 14 INFEASIBLE (avg 115 s), against a `solver_timeout_s: 60` default applied per phase of a two-phase solve (~120 s ceiling, matching observed maxima). These are not chronically slow VENs — ven-12 has 1163 OPTIMAL solves in its lifetime `plan_history` and zero during that run window. The leading explanation is GB-37's expired-hard-deadline condition (fixed forward there) making the problem infeasible rather than merely large, but that is unconfirmed: ven-1/ven-7 charged normally *after* the same expired deadline, which the mechanism does not explain. Needs a clean re-run with the GB-37 deadline/soft-deadline fixes to see whether TIME_LIMIT disappears on its own before any solver tuning is attempted | Medium — a VEN whose planner times out silently contributes nothing to a scenario and quietly corrupts fleet-wide KPIs, with no error surfaced to the operator | Medium — first re-run S-9 post-GB-37 and re-measure; only if TIME_LIMIT persists, investigate problem size for heavy asset mixes (EV+heater, EV+PV+battery at 288 slots), the `MIP_GAP_TARGET` 0.02 tolerance, and whether `solver_timeout_s` should scale with binary count rather than being a flat 60 s |

---

## Dependency Vulnerabilities — 2026-08-19 (npm rows); 2026-07-16 (cargo rows)

> Re-run `cargo audit` and `npm audit` before each release and update this section.

Cargo rows unchanged since the 2026-07-16 `cargo update` (VEN, VTN/bff) pass on
`fix/review-c3-code` — not re-verified in this update. npm rows re-run 2026-08-19
after the GB-34 `react-router`/`react-router-dom` v6→v7 migration (`^7.18.0` in
both `VEN/ui` and `VTN/ui`) closed out the last open finding from the 2026-08-16
GB-16 pass:

| Component | cargo/npm audit result |
|-----------|------------------------|
| VEN (Rust) | **0 vulnerabilities, 0 warnings** (267 crates) — not re-verified 2026-08-16 |
| VTN/bff (Rust) | 1 advisory — see below (315 crates) — not re-verified 2026-08-16 |
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
