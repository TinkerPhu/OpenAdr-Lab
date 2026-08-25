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
| GB-35 | GB-22's own "audit other `@ven-ui`/browser scenarios for the same gap" reminder, narrowed: a full `features/` tree audit (2026-08-16, closing GB-22) found the browser+real-backend-poll race pattern concretely limited to the scenarios GB-22 itself fixed, but also surfaced a broader, lower-priority class not addressed there — backend-only scenarios with long (90–300s) `poll_until` calls that share the same host-load sensitivity mechanism without the browser-page combo that caused GB-22's actual reported failures: `features/controller/05_ev_charging_scenarios.feature` (all 4 scenarios, `dispatcher_steps.py` timeouts up to 90s) plus other long-timeout call sites (`alerts_steps.py`, `ev_charging_steps.py`, `uc_steps.py`, `planner_steps.py` at 300s; `request_modes_steps.py`, `comfort_steps.py` at 180s). None have failed under contention yet (unlike the GB-22 instances), so isolating all of them pre-emptively would bloat the isolated pass without evidence; this item exists so the reminder isn't lost, not to isolate all of them by default | Low — speculative, no confirmed failures yet, same mechanism as GB-22 | Low — review each site if/when it's observed flaking, same "tag `@isolated` or raise the timeout" fix shape as GB-22 |
| GB-38 | Three VENs' MILP hit the solver time limit on essentially every solve for a whole 24h run and consequently never charged their EVs at all, despite valid active sessions and 25-30% SoC (`docs/history/fleet_run_journal.md`, S-9 re-run 2026-08-20/21): ven-12 TIME_LIMIT on 1419/1419 solves (avg 63 s), ven-3 1363 TIME_LIMIT + 56 INFEASIBLE (avg 112 s), ven-5 1237 TIME_LIMIT + 14 INFEASIBLE (avg 115 s), against a `solver_timeout_s: 60` default applied per phase of a two-phase solve (~120 s ceiling, matching observed maxima). These are not chronically slow VENs — ven-12 has 1163 OPTIMAL solves in its lifetime `plan_history` and zero during that run window. ~~The leading explanation is GB-37's expired-hard-deadline condition~~ **Superseded 2026-08-25: the cause is almost certainly GB-40 (heater MILP), not the EV deadline.** The expired-deadline theory never explained why ven-1/ven-7 charged normally *after* the same expired deadline. Heater presence explains all six EV-roster VENs exactly: the three that timed out and never charged — ven-3 (ev+heater+pv), ven-5 (ev+heater+pv+battery), ven-12 (ev+heater) — **all carry a heater**; the two that charged — ven-1 (ev+pv+battery), ven-7 (ev+pv) — **carry none**; ven-11 (ev only, no heater) was blocked by the separate, known SoC-reset mistake. GB-40's isolated benchmark measures an active heater turning a 0.19 s solve into a 108.55 s timeout, which is precisely this symptom. The EV never charged because the *planner* never produced a plan, not because the EV's deadline had expired. Confirmation comes free from the S-9 re-run already in flight (which carries the GB-37 deadline fix): if the same heater VENs still time out, the deadline was never the cause. Resolve via GB-40; keep this row only until that re-run confirms | Medium — a VEN whose planner times out silently contributes nothing to a scenario and quietly corrupts fleet-wide KPIs, with no error surfaced to the operator | Medium — first re-run S-9 post-GB-37 and re-measure; only if TIME_LIMIT persists, investigate problem size for heavy asset mixes (EV+heater, EV+PV+battery at 288 slots), the `MIP_GAP_TARGET` 0.02 tolerance, and whether `solver_timeout_s` should scale with binary count rather than being a flat 60 s |
| GB-39 | Dark mode for VEN UI and VTN UI — no theme switching exists today, both UIs are light-only | Low — cosmetic/comfort, no functional gap | Low |
| GB-40 | A heater in a VEN's asset mix costs ~4.7× the MILP solve time of any other mix, and is the concrete driver behind GB-38's fleet-wide `TIME_LIMIT` symptom. Measured across all 20 VENs in one S-7 window (`experiments/results/20260824-0312-s7_stress/*-plan-history.json`): heater VENs mean **84.2 s** per solve (n=10) vs **18.0 s** without (n=10), and the eight slowest VENs in the fleet *all* carry a heater. Six of them sit at 105–121 s, i.e. pinned to the `solver_timeout_s: 60` two-phase ceiling, so they time out on essentially every cycle. This compounds: a `TIME_LIMIT` solve burns its **full** budget before giving up, so the slowest VENs are also the most CPU-expensive, which starves the rest and pushes more of them into timeout. Node2 (17 VENs, 4 cores) consequently runs 85–89% busy with a run queue of 4–5 — expected concurrent solves ≈ 17 × 51.7 s / 300 s ≈ 2.9 on 4 cores (measured 2026-08-25, `docs/history/fleet_run_journal.md`). Not every heater VEN is slow (ven-2 18.2 s, ven-20 29.0 s are both heater-bearing), so it is the heater's integer relay/staging variables *interacting* with other assets' continuous variables that should be suspected, not the heater alone. **Measured in isolation (2026-08-25, `VEN/src/controller/milp_planner/tests/solve_cost.rs`)**: the same ven-3-shaped site on the same 288-slot grid, solved with and without the heater and nothing else changed, gives **0.19 s without / 108.55 s with — 561×**. The with-heater figure is essentially the two-phase `solver_timeout_s` ceiling, i.e. an *active* heater does not merely slow the solve, it **times it out**. The fleet's gentler 4.7× is an average that dilutes active heaters with idle ones (`MustNotRun` fixes every `z` to 0, leaving nothing to branch on), so 561× is the real cost of a heater that is actually running. Debug build, but the caveat is immaterial here: the no-heater case at 0.19 s shows Rust-side constraint building is negligible, so the 108.55 s is essentially all HiGHS branch-and-bound. **Diagnosis (code-grounded)**: the cause is not binary *count* — battery VENs declare comparable numbers (`u_bat` + `z_active` + `delta_active`) and solve in 18–50 s. It is that the heater's binaries carry the **power level itself**, not a mode. Battery/EV power (`p_ch`/`p_dis`) are continuous variables whose binary only picks a direction, so the LP relaxation is tight; the heater has *no* continuous power variable at all — `P_heat = p_mid·z_mid + p_full·z_full` (`heater_milp.rs` C2), so the only way the relaxation can express the intermediate power that tank-trajectory tracking almost always wants is a fractional `z`. Nearly every slot therefore relaxes fractional, and branch-and-bound must branch across all 2n heater binaries (n=288 on the standard `plan_zones` grid, identical for every heater VEN, so the grid is not the differentiator). Compounding it, **no min-up/min-down (dwell-time) constraints exist anywhere** — anti-chatter is only the soft `sw` switching penalty, which prices chatter but does nothing to tighten the relaxation or prune the tree. Likely also why ven-2/ven-20 are fast: a heater in `MustNotRun` has all `z` fixed to 0 (`heater_milp.rs` build), leaving nothing to branch on — worth confirming those two were simply idle in that window rather than structurally cheaper | High — silently corrupts fleet KPIs (a timed-out planner contributes nothing and raises no operator-visible error), caps how large the fleet can grow on the available hardware, and is the mechanism behind GB-38 | Medium — first build a reproducible local benchmark that solves one heater VEN's MILP and reports binaries, LP-relaxation gap, nodes explored and phase split, so any change is measured rather than argued. Then, in order of expected value: (1) **add dwell-time constraints** (min slots on / min slots off per tier) — standard unit-commitment practice, prunes exactly the fractional-chatter region the relaxation currently wanders in, and is *more* physically faithful than today's soft penalty since real relays have minimum cycle times (`c_wear_eur` already shows relay wear is a modelled concern); (2) give the heater a continuous power variable bounded by the tier binaries (`P ≤ p_full·z_full + p_mid·z_mid`, tightening the relaxation without changing attainable power); (3) per-VEN `solver_timeout_s` scaled to binary count rather than a flat 60 s. Cheapest unrelated mitigation, orthogonal to the formulation: raise `replan_interval_s` to 600 s (halves fleet solver CPU, 2.9 → 1.5 expected concurrent solves, costs no VENs) |

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
