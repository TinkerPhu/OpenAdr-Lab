# Strategy — Resolving the Open Wiki Review Items (2026-07-05)

> Source: the six unresolved items in `wiki/review.md`, all filed during the VEN code
> deep-ingest at `e138861` (full findings: `wiki/queries/ven-code-vs-docs-audit.md`).
> This document is the decision base for implementation planning: per item, the solution
> options with pros/cons and a 1st/2nd recommendation with reasoning.
> Status register for accepted debt: `docs/reference/TECHNICAL_DEBTS.md`.

---

## Overview and suggested sequencing

| # | Item | Kind | Effort | 1st recommendation |
|---|------|------|--------|--------------------|
| R1 | `SolverPort` rule vs missing trait | rule/code mismatch | M | Introduce the port for real — **✅ DONE** |
| R2 | `VEN_ARCHITECTURE.md` stale sections | doc drift | S–M | Two-stage rewrite, aligned with R3/R5/R6 outcomes — **✅ DONE** (Stage 1+2 combined, with GAP markers) |
| R3 | Dead plan-cycle TELEMETRY_STATUS report | dead code | S | Delete the path — **✅ DONE** |
| R4 | File-size caps violated (9 files + tasks/planning.rs) | rule vs reality | M–L | Targeted splits + production-line metric — **✅ DONE** |
| R5 | Dead-code inventory (`StaleRatePolicy`, vocab blocks, …) | dead code | S–M | Quarantine (not delete); every item backed by a BACKLOG entry — **✅ DONE** |
| R6 | One-shot report obligations | functional gap (cert) | M | Implement recurring re-arm — **✅ DONE** |

**Dependency-driven order:** R5 (delete dead code) → R3 (delete dead path) → R1 + R4
(port + splits touch the same files — do as one refactor arc) → R6 (feature) → R2 last
(rewrite the doc once, against the settled code). R2's *purely descriptive* fixes
(route table, trace endpoints, §5 audit) can be done any time.

---

## R1 — `SolverPort`: documented port that doesn't exist

**Finding.** `.claude/CLAUDE.md` §ven-architecture and `docs/architecture/VEN_ARCHITECTURE.md`
list `SolverPort: services → controller/milp_planner (solve)` as a port obligation.
No such trait exists: `tasks/planning.rs:266` calls `milp_planner::run_planner()` (a free
function with 16 parameters) directly inside `spawn_blocking`. Sub-finding: the same
CLAUDE.md's §testing says shared mocks in `services/test_support/` are "not cfg(test)",
but `services/mod.rs:2` gates them with `#[cfg(test)]`.

### Option A — Introduce the port for real

Define a `SolveRequest` struct bundling `run_planner`'s parameters, a
`trait SolverPort { fn solve(&self, req: SolveRequest) -> Plan }` implemented by the MILP
module, and inject `Arc<dyn SolverPort>` into the planning cycle. Move the cycle
orchestration (context building, anchor computation, terminal-reward resolution —
currently ~200 lines in `tasks/planning.rs`) into `services/planning.rs`; the task keeps
only the timer/trigger loop and `spawn_blocking`.

- **Pros**
  - Code finally matches the documented architecture instead of the docs being amended
    downward.
  - The planning *use case* becomes unit-testable without HiGHS: a mock `SolverPort`
    returning canned `Plan`s lets service-level tests cover trigger handling, anchor
    logic, and gate interaction in milliseconds (today that needs the full solver).
  - Collapses the widest function signature in the codebase into one typed request
    struct — independent readability win.
  - Directly shrinks `tasks/planning.rs` below its 200-line cap → solves part of R4.
- **Cons**
  - Medium effort; touches the hottest file in the repo (merge-conflict risk with
    feature branches).
  - Risk of a ceremonial single-impl trait if the mock is never actually used — the
    payoff only materialises if service tests are written alongside.
  - `SolveRequest` needs care with `Send + 'static` for `spawn_blocking`.

### Option B — Amend the rule instead

Remove `SolverPort` from `.claude/CLAUDE.md` and `VEN_ARCHITECTURE.md`; document
`AssetMilpContext` as the solver-side port and `run_planner` as a deliberate direct call
from the adapter ring (adapters may import infra — no ring violation exists today).

- **Pros**
  - Zero code risk, one-commit fix; documentation becomes honest immediately.
  - Defensible architecturally: the swappability that matters (assets into the solver)
    already exists via `AssetMilpContext`; nobody is about to swap HiGHS.
- **Cons**
  - The planning loop stays untestable without HiGHS; slow tests remain the only
    coverage for cycle orchestration.
  - Use-case logic stays in the adapter ring (`tasks/`), which is the actual clean-
    architecture smell behind the missing port.
  - Locks in the R4 problem for `tasks/planning.rs` (its oversize *is* the misplaced
    orchestration).

### Option C — Function-injection seam, no named trait

Pass a `solve: impl Fn(SolveRequest) -> Plan` closure into the service.

- **Pros**: cheapest testability gain.
- **Cons**: matches neither the docs nor Option B's amended docs; ad-hoc seams age
  poorly; still requires the `SolveRequest` refactor that is most of Option A's cost.

### Recommendation

1. **Option A** — because it resolves three findings with one refactor (missing port,
   `tasks/planning.rs` oversize from R4, untestable orchestration), and the main cost
   (`SolveRequest` struct) is work every option larger than "edit the docs" needs
   anyway. Do it together with the R4 split of the same file.
2. **Option B** — if the refactor budget goes to R6 (certification) instead. An honest
   rule beats an aspirational one; B can be upgraded to A later without wasted work.

**Sub-decision (cfg(test) mismatch):** amend CLAUDE.md's wording rather than un-gating
the mocks. Nothing outside `#[cfg(test)]` consumes them; compiling mocks into the
production binary to satisfy a stale sentence is backwards. One-line doc fix.

---

## R2 — `VEN_ARCHITECTURE.md` has accumulated contradictions

**Finding.** Eight sections now contradict the code: §2.1 dispatcher description
(auto-follow/deviation distribution not implemented; ledger lives in the monitor),
§2.1 event table (`ALERT_*`, `DISPATCH_SETPOINT`, `CHARGE_STATE_SETPOINT`, export-side
subscription/reservation unhandled), §2.2 "20 s periodic" (default 300 s), §2.3
`StaleRatePolicy` (dead enum), §3.0 `AssetInterface`/`SimulatedAsset`/`MeasuredAsset`
(never built), §3.3/§4.5/§4.7 `/trace` + `/sim/override` (endpoints replaced), §4 route
list (~14 endpoints missing, routes live in `routes/mod.rs` not `main.rs`), §5.2/§5.3
(audit of pre-`TimeSeries` code; the "target architecture" is now implemented).

### Option A — Full rewrite pass now

One doc-only PR rewriting all stale sections against `e138861`, sourced from the wiki
audit page.

- **Pros**: restores the doc's "authoritative reference" status in one stroke; the wiki
  DRIFT callouts can all be resolved; zero code risk.
- **Cons**: several sections describe behaviour whose *fate is decided by other items*
  (§2.1 event table → R5/R6, §2.3 StaleRatePolicy → R5, dispatcher deviation text → R5's
  overlay decision, `SolverPort` mentions → R1). Rewriting first means rewriting twice.

### Option B — Two-stage rewrite, decision-aligned

Stage 1 (now): fix the purely descriptive drift that no pending decision touches —
§4 route table, §4.7 trace/storage table, §3.0 asset abstraction, §2.2 loop numbers,
§3.3/§4.5 endpoint names, and **replace §5.2/§5.3 wholesale** with a short section
describing the implemented `TimeSeries` (`common/mod.rs`) and the one remaining gap
(slot-start tariff sampling). Stage 2 (after R1/R3/R5/R6 land): rewrite §2.1/§2.3 to
match the settled behaviour.

- **Pros**: no double work; the doc is accurate about everything that is stable; stale
  remainder is explicitly tracked (wiki callouts + this plan).
- **Cons**: the doc is mixed-accuracy for the interim; requires discipline to actually
  do stage 2.

### Option C — Demote the doc

Strip `VEN_ARCHITECTURE.md` to a stable overview (rings, ports, component map) and point
to the wiki for volatile detail; the wiki has staleness detection, the doc does not.

- **Pros**: ends the recurring drift problem structurally; least total maintenance.
- **Cons**: `docs/` is the repo-canonical, human-reviewed reference — CLAUDE.md and
  SESSION_START point to it; the wiki is LLM-maintained context infrastructure, not a
  reviewed spec. Moving authority there inverts the intended relationship
  (`wiki/purpose.md` non-goal: "the wiki never becomes a second spec").

### Recommendation

1. **Option B** — decision-aligned staging is the only variant that neither wastes work
   nor leaves known falsehoods standing where they mislead (the §5 audit and §4 API
   table are the most-cited sections and are fixable today).
2. **Option C-lite folded into B** — while doing stage 1, *do* cut content that has no
   business in an architecture reference regardless of decisions: the §5.2 line-number
   audit (historical) and the §4 exhaustive route table could become a generated
   appendix or a pointer to `routes/mod.rs`, which cannot drift.

---

## R3 — Plan-cycle TELEMETRY_STATUS report is dead on arrival

**✅ RESOLVED (Option B — deleted).** `build_status_report` and its call site removed
(`controller/reporter.rs`, `tasks/planning.rs`), along with the now-orphaned
`latest_net_import_kw` helper and the `plan_cycle_event` field on `PlanCycleResult`
(nothing outside `services/planning.rs` read it once the report-building call site was
gone). `docs/BACKLOG_OpenADR_Cert.md`'s "Status reports (event-driven)" row corrected
from "Full" to "Missing", pointing at the surviving observability paths
(`/trace/events`, `/plan/events` SSE). Full VEN test suite green (452 tests, down from
458 by exactly the 6 tests removed with the dead code); `cargo fmt`/`clippy -D warnings`
clean; ring-invariant greps unaffected.

**Finding.** `tasks/planning.rs:338` calls `build_status_report(..., program_id: None, ...)`;
the function's first line returns `None` without a program id (`reporter.rs:512`), so no
status report has ever been submitted. `docs/BACKLOG_OpenADR_Cert.md` rates this row
"Full — Triggered on `PlanCycle` controller event", which is therefore also wrong.

### Option A — Wire a real programID

Resolve the programID at plan-cycle time (from the triggering event, or the first active
event/program in state) and submit as designed; optionally only on *adopted* plans to
limit volume.

- **Pros**: small change; makes the cert-backlog row true; gives the VTN visibility into
  VEN planning activity — plausibly useful for the swarm-behaviour roadmap (many VENs,
  one operator view).
- **Cons**: TELEMETRY_STATUS with a free-text "PlanCycle trigger=… slots=…" description
  is a home-grown payload, not something the spec requests; no current consumer — the
  VTN UI doesn't render it; replans every 300 s across 3 VENs is meaningful report
  traffic for zero readers; the OpenADR-correctness goal argues against inventing
  payload semantics.

### Option B — Delete the path

Remove the call in `tasks/planning.rs`, `build_status_report`, its tests, and correct
the cert-backlog row to "Missing".

- **Pros**: honest and free; observability of plan cycles already exists twice — the
  `ControllerEvent::PlanCycle` trace (`/trace/events`) and the SSE `PlannerEvent` stream
  (`/plan/events`) that the UI actually consumes; git history preserves the code if a
  VTN-side consumer ever materialises.
- **Cons**: loses the *intended* (never realised) VTN-side signal; a future
  multi-VEN operator view would have to rebuild it.

### Option C — Keep behind a default-off config flag

- **Pros**: preserves optionality.
- **Cons**: dead code with extra steps; flags nobody flips are the same drift generator
  this audit just cleaned up.

### Recommendation

1. **Option B (delete)** — no consumer, no spec mandate, duplicate observability paths
   already exist, and the repo has just paid the price of keeping aspirational code
   around (R5). The cert backlog row change is part of the same commit.
2. **Option A** — only if the swarm/operator-view milestone is being pulled forward;
   then do it properly: programID from the triggering event, submit only on adopted
   plans, and add a VTN-UI consumer in the same feature so it isn't write-only.

---

## R4 — File-size rule vs reality

**✅ RESOLVED (2026-07-06) — Option C.** Split the genuinely mixed-concern files,
extended the metric+script to cover all of `VEN/src/`, accepted the rest via a
documented allowlist:

- **`assets/heater.rs`/`ev.rs`/`battery.rs`** → split into physics (`heater.rs`/`ev.rs`/
  `battery.rs`, kept) + MILP-context impls (new `heater_milp.rs`/`ev_milp.rs`/
  `battery_milp.rs`). Zero import-path changes needed outside `assets/` — every external
  consumer already reached the `*MilpContext` types via `controller::milp_planner::asset_port`.
  Deleted a handful of confirmed-dead delegate wrapper methods uncovered in the process
  (re-verified independently, not just moved forward).
- **`profile.rs`** → `profile/{schema,defaults,validate}.rs`; `Profile::default()`
  converted from a bare fn to `#[derive(Default)]` for consistency with the other config
  structs' `impl Default`.
- **`routes/hems.rs`** → `routes/hems/{mod,misc,sessions,ev,heater,shiftable_loads,baseline_override}.rs`,
  re-exported at the `hems::` root so `routes/mod.rs`'s existing fully-qualified route
  wiring needed zero changes.
- **`tasks/planning.rs`** → hoisted pure/business-logic blocks into `services/planning.rs`
  (`align_to_step`, `apply_pending_pv_inject`, `build_asset_contexts`, `build_solve_request`)
  and extracted the progress-ticker task machinery into `tasks/progress_ticker.rs` — got
  it from 286 to 198 production lines (200 cap).
- **`state.rs`** → moved `SimInjectState` to `entities/sim_inject.rs` (Domain ring, so
  both `routes/sim.rs` and `tasks/sim_tick/helpers.rs`, both Adapters ring, import in the
  correct direction) — 531 → 412 production lines.
- **`controller/reporter.rs`** needed no split — R3's dead-code deletion already brought
  it under the cap (559 → ~483).
- **Accepted via allowlist**: `assets/mod.rs` only (617 production lines) — cohesive
  `AssetConfig` dispatch boilerplate; real fix is the enum→trait refactor already tracked
  in `docs/plans/refactoring_backlog.md`. `simulator/mod.rs` turned out **not** to need
  an allowlist entry — the corrected non-blank-line metric puts it at 470, under cap.
- **`scripts/audit_file_sizes.py`** (renamed from `scripts/check_task_file_sizes.py`)
  now checks both caps across all of `VEN/src/`, with test-only paths (any directory
  literally named `tests`) excluded and the allowlist above encoded directly in the
  script. `.github/workflows/file_size_audit-splittasks.yml` updated to call it.
- `.claude/CLAUDE.md` corrected: the file-size rule now states the "production lines"
  metric explicitly (it previously didn't say how to count); the stale claim that
  `.github/workflows/` is empty was fixed (three workflows already exist: fmt/clippy/
  audit/DCO on PR, this file-size audit, and a manual-dispatch E2E suite).

**Finding.** `.claude/CLAUDE.md` caps `VEN/src/` files at 500 lines and `tasks/` at 200.
Counting **production lines only** (before `#[cfg(test)]`): `assets/heater.rs` 799,
`profile.rs` 777, `assets/mod.rs` 687, `routes/hems.rs` 678, `assets/ev.rs` 634,
`controller/reporter.rs` 559, `assets/battery.rs` 523, `state.rs` 519,
`simulator/mod.rs` 503; `tasks/planning.rs` 363. Counting raw lines (as any naive CI
check would), 16 files break the cap. No CI enforces it yet (`.github/workflows/` empty).

### Option A — Enforce as written: split everything

- **Pros**: rule stays absolute and simple; the biggest files genuinely mix concerns.
- **Cons**: large mechanical churn including files that are big-but-cohesive
  (`state.rs` is a flat accessor list; `assets/mod.rs` is dispatch boilerplate whose
  real fix is the enum→trait refactor already tracked in
  `docs/plans/refactoring_backlog.md`); splitting cohesive files creates navigation
  cost without design gain; high conflict risk with in-flight branches.

### Option B — Amend the rule only

Count production lines (exclude `#[cfg(test)]`), raise the cap to ~800.

- **Pros**: near-zero effort; acknowledges the Rust tests-in-file convention (half of
  several "violations" is test code, which coexists with the test-first rule).
- **Cons**: a cap adjusted to whatever currently violates it stops being a constraint;
  `heater.rs` at 799 production lines really is two files (physics + MILP context impl)
  living together.

### Option C — Targeted splits + production-line metric (hybrid)

Change the metric to production lines with the 500/200 caps kept, then split only where
cohesion is genuinely poor:

| File | Split |
|---|---|
| `assets/heater.rs`, `ev.rs`, `battery.rs` | physics vs `*MilpContext` impls (e.g. `assets/<x>.rs` + `assets/<x>_milp.rs`) — the MILP impls are already logically part of the `asset_port` boundary |
| `profile.rs` | schema structs vs `validate()` vs defaults/`effective_*` helpers |
| `routes/hems.rs` | per-resource route modules (sessions, requests, shiftable, baseline) |
| `tasks/planning.rs` | orchestration → `services/planning.rs` (this **is** R1 Option A) |
| `controller/reporter.rs` | measurement vs obligation vs status builders (status may vanish via R3) |

Accept `state.rs`, `simulator/mod.rs`, `assets/mod.rs` (503–687, cohesive) either via
the new metric (state.rs 519 → still over — trim by moving `SimInjectState` next to the
sim routes) or a documented allowlist, and add `scripts/audit_file_sizes.sh` so the rule
is mechanically checkable now and CI-ready later.

- **Pros**: effort lands where it pays; synergises with R1 and R3; rule becomes
  enforceable instead of aspirational; splits follow existing seams rather than a line
  counter.
- **Cons**: an allowlist (if used) is a rule-with-exceptions; partial compliance needs
  the audit script to stay honest.

### Recommendation

1. **Option C** — the cap's *purpose* (navigable files, reviewable diffs, forced
   separation of concerns) is served by splitting the mixed-concern files and measuring
   production lines; it is not served by slicing accessor lists. Bundle the
   `tasks/planning.rs` and `reporter.rs` pieces with R1 and R3 respectively.
2. **Option B** — acceptable interim if no refactor window exists, but pair it with a
   hard commitment on `heater.rs`/`ev.rs`/`profile.rs`, which are past any defensible
   cap.

---

## R5 — Dead-code inventory

**✅ RESOLVED (2026-07-05) — Option B (quarantine), uniformly, nothing deleted.** Both
Group 1 and Group 2 below are kept, per explicit user decision:

- **Group 1** (`AssetProfile`, `AssetHeuristics`, `AssetForecast`, `AssetLedger`,
  `PenaltyRule`, and the rest of the vocabulary block, plus 7 additional zero-reference
  types found during implementation: `AssetState`, `PowerAdjustability`,
  `UserRequestMode`, `FlexibilityDirection`, `RateType`, `RateUnit`, `PowerRange`) moved
  verbatim into new module `VEN/src/entities/design_vocabulary.rs`, with a file banner
  stating none of it is current behaviour. `entities/asset.rs` now holds only the five
  confirmed-live types (`AssetType`, `DeviceResponsiveness`, `CompletionPolicy`,
  `PlanTrigger`, `ComfortRate`) and lost its file-level `#![allow(dead_code)]`.
- **Group 2** (`apply_battery_correction_overlay`, `HvacService`, the `OadrEventCache`
  family, unused `DomainError` variants) stayed in place (no move needed — these are
  functions/services/enum variants embedded in production files, not standalone data
  types); their `#[allow(dead_code)]` comments were corrected to stop citing a deleted
  design doc and instead cite the new BACKLOG entries.
- Every item (both groups, all 16 BACKLOG entries: BL-14 through BL-29) is now backed by
  a `docs/BACKLOG.md` entry with a problem/fix/complexity writeup — "not implemented yet"
  is a tracked, prioritizable plan rather than a bare header comment.
- The two pre-existing GAP markers in `docs/architecture/VEN_ARCHITECTURE.md` (§2.1
  `apply_battery_correction_overlay`, §2.3 `StaleRatePolicy`) were updated to reflect the
  resolution (kept, BACKLOG-tracked — not "wire or delete, undecided"). A third,
  previously-undiscovered drift spot was also fixed: the §2.2 flow diagram's "Writes
  PlanWarnings → UserNotifications" line overstated a notification feed that doesn't
  exist — now has a GAP marker citing BL-20.
- Verification: `cargo build`/`cargo test` clean (pure move + comment-only edits, no
  behavior touched), `cargo fmt --check`/`cargo clippy -D warnings` clean, ring-invariant
  greps unaffected.

**Finding (corrected 2026-07-05).** The original finding lumped two different situations
together under one "dead code" label. Re-verified against current code:

- **Group 1 — roadmap vocabulary, confirmed wanted (keep).** The user confirmed these
  are future features, not abandoned experiments: `AssetProfile` (the
  `entities/asset.rs` one — distinct from the unrelated, actively-used `profile::AssetProfile`
  enum), `AssetHeuristics`, `AssetForecast` (+ `TimeRange`, `ForecastSource`),
  `AssetLedger`, `PenaltyRule` (+ `PenaltyThreshold`, `PenaltyCondition`). Same
  treatment extends to the rest of the `#![allow(dead_code)]` block for the same reason
  (type-level sketch of a real, not-yet-built feature): `AssetFlexibility`,
  `ExternalDataSource` (+ `ExternalDataSourceType`, `ExternalDataFetchStatus`),
  `StaleRatePolicy`, `ThermalModelParams`, `UserNotificationSeverity`,
  `DefaultValueCurve`. **Correction to the original finding: `ComfortRate` is not dead.**
  It's constructed by every asset's `default_comfort_rates()` and served via
  `GET` through `routes/hems.rs:268` — verified 2026-07-05. It was miscategorized in the
  original audit and needs no action here.
- **Group 2 — confirmed abandoned or genuinely unused, not yet re-confirmed with the
  user.** `apply_battery_correction_overlay` (implemented, unit-tested, called only from
  its own test module — never from `build_setpoints`), `HvacService` (referenced only
  within its own file, `services/hems.rs`), `entities/capacity.rs`'s `OadrEventCache` /
  `OadrProgramConfig` / `OadrCapacityRequest` (zero references anywhere outside their
  own definitions), and `DomainError::{PlanInfeasible, VtnUnreachable, ProfileInvalid}`
  (constructed only inside `entities/error.rs`'s own `Display` test, never at a real
  error boundary). These were not part of the "keep" list the user gave — still
  provisionally recommended for deletion (Option A), but that recommendation has not
  been separately re-confirmed since the Group 1 correction, so treat as open until
  asked directly.

### Option A — Delete, record intent in BACKLOG

Remove everything unreferenced; transfer genuinely-wanted design intent as one-line
backlog entries. Git preserves the implementations. **No longer the recommendation for
Group 1** — see below.

- **Pros**: kills the root cause of drift permanently; shrinks the domain ring;
  `entities/` stops needing `#![allow(dead_code)]` once Group 1 is moved out, so *future*
  dead code becomes a compiler warning again; recovery cost is `git log -S`.
- **Cons**: for Group 1, deleting loses the type-level sketch of features the user
  wants — not the right trade-off there. Still applicable to Group 2, pending
  confirmation.

### Option B — Quarantine

Move vocabulary types to a clearly-marked `entities/design_vocabulary.rs` (or
equivalent module) with a "not implemented — do not cite as current behaviour" header,
and make sure no doc (`VEN_ARCHITECTURE.md`, wiki pages) describes them as active. This
is the actual fix for the doc-drift failure mode: the drift was never caused by the
types existing, it was caused by docs describing unwired sketches as shipped behaviour.

- **Pros**: intent stays visible and compiles (useful for Group 1's real roadmap
  items); moving them out of `entities/asset.rs` into a module named for what it is
  ("design vocabulary," not "asset entities") stops them from reading as part of the
  live domain model at a glance; each BACKLOG entry (see below) becomes the actual
  tracked plan to implement instead of a vague "quarantine" label.
- **Cons**: still compiles dead weight and keeps `#![allow(dead_code)]` on that one
  module (acceptable — it's now explicitly labelled non-live, not silently tolerated
  across all of `entities/`); a header alone doesn't stop retrieval tools from quoting
  the types, which is why the BACKLOG entries (not just the header) are the binding
  documentation of "not implemented yet."

### Option C — Wire selectively

Implement the pieces whose absence the docs already lie about now, rather than deferring
to BACKLOG items. Already the substance of `BL-07` (`StaleRatePolicy` dispatch) and
`BL-09` (penalty threshold check) for two of the Group 1 items.

- **Pros**: converts docs from wrong to right immediately.
- **Cons**: feature work disguised as cleanup; each Group 1 item needs design, tests,
  and BDD coverage on its own schedule — doing all of it now isn't warranted just to
  resolve a doc-drift finding.

### Recommendation

1. **Group 1 (roadmap vocabulary) → Option B.** Quarantine into a dedicated module
   (e.g. `entities/design_vocabulary.rs`) with a clear "not implemented, tracked in
   `docs/BACKLOG.md`" header; keep `#![allow(dead_code)]` scoped to that module only.
   Every type gets a corresponding `docs/BACKLOG.md` entry (added — see BL-14 through
   BL-19 below, plus the existing BL-07/BL-09/BL-10 cross-references) so "not
   implemented yet" is a tracked, prioritizable item rather than a header comment.
   `VEN_ARCHITECTURE.md` and the wiki must never describe any Group 1 type as active
   behaviour — verify with a grep pass at implementation time.
2. **Group 2 (apply_battery_correction_overlay, HvacService, OadrEventCache family,
   unused DomainError variants) → Option A, still open.** Provisionally recommend
   delete-and-record-intent as before, but this needs the same explicit user
   confirmation Group 1 just got before acting — the same "is this really abandoned or
   is it roadmap" question applies to `apply_battery_correction_overlay` in particular
   (it's finished, tested work, just unwired).

---

## R6 — Report obligations are one-shot

**✅ RESOLVED (2026-07-05) — Option A (recurring re-arm).** Root cause was two
cooperating bugs, both fixed:

1. `state.rs::mark_obligation_fulfilled` replaced with `rearm_obligation(id, next_due_at)`
   — instead of permanently setting `fulfilled = true`, it advances `due_at` by
   `interval_duration_s` (computed in `services/obligation.rs::ObligationService::check_and_report`
   using the injected `now` clock). `fulfilled` stays `false`; it's no longer the
   mechanism that stops an obligation.
2. New `state.rs::retire_obligations_not_in(active_event_ids)` prunes obligations whose
   parent event has dropped out of the active poll set — called from
   `tasks/poll_events.rs` right after `add_obligations`, using `prev_event_ids` (which by
   that point in the loop already holds the current tick's IDs).

Three things the original finding worried about turned out to already be correct, so
they were **not** changed: (a) report windowing — `AssetHistoryBuffer`'s hard 3600-point
cap already gives `build_measurement_report_for_obligation` a natural bounded trailing
window once obligations actually recur; (b) VTN report naming — already stable per
`(ven, event, payload_type)`, so recurring cycles upsert-overwrite the same report
resource, exactly Option A's intended mechanism; (c) `extract_report_obligations`'s
dedup-by-`(event_id, payload_type)` — still correct under the new model, since the same
obligation is what recurs now (re-armed in place), not a freshly regenerated one each
cycle — its test comment was updated to explain why, not to weaken the assertion.

Verification: new unit tests for `rearm_obligation`/`retire_obligations_not_in`
(`state.rs`), a recurring-cycle + VTN-error end-to-end test (`services/obligation.rs`,
also closing a previously-acknowledged test gap), and an obligation-retirement test
(`tasks/poll_events.rs`). `docs/BACKLOG_OpenADR_Cert.md`'s report-obligation row updated
to describe the recurring behaviour.

**Finding.** `extract_report_obligations` creates one obligation per
(event, payloadType); `mark_obligation_fulfilled` never re-arms it. A
`reportDescriptor.frequency: 900` therefore produces exactly one report instead of one
per 15 minutes for the event's lifetime. Certification-relevant
(`docs/BACKLOG_OpenADR_Cert.md`; wiki: `openadr-spec-use-cases` §reports row).

### Option A — Recurring re-arm

On fulfilment, set `due_at += interval_duration_s` instead of `fulfilled = true` (or
keep `fulfilled` per period and store `next_due`); retire the obligation when its event
disappears from the poll set. Each period submits a report covering the elapsed
interval(s) from the per-asset history buffers (machinery exists —
`build_measurement_report_for_obligation` already resamples multi-interval windows).

- **Pros**: matches the descriptor's plain meaning; the report-building side needs no
  change; bounded, well-testable state change; directly improves the cert row.
- **Cons**: naming/upsert semantics need a decision — the current 409-upsert by
  `reportName` would make each period *overwrite* the previous report at the VTN, so
  either per-period names (report proliferation) or one growing report via PUT (see C);
  downtime/missed-period semantics need defining; report volume rises (3 VENs × events).
- **Design note**: obligation state is in-memory (`HemsState`) — a VEN restart forgets
  fulfilment history either way; recurring re-arm actually degrades more gracefully
  here than one-shot (it resumes; one-shot re-reports once).

### Option B — Defer, keep as documented cert gap

- **Pros**: zero effort; the lab's day-to-day loop (price → plan → usage reports) does
  not depend on it; cert is explicitly a "distant goal" (`wiki/purpose.md`).
- **Cons**: it is not only a cert checkbox — a VTN operator testing frequency-driven
  reporting today silently gets one report and may burn time debugging the VTN side;
  the behaviour contradicts the descriptor the VEN itself acknowledges.

### Option C — Rolling-report rework

One report object per obligation, updated each period via PUT with the growing interval
list (closer to the spec's §7.5 rolling-report concept; reuses the existing
upsert-by-name path as the *intended* mechanism rather than a collision handler).

- **Pros**: cleanest spec alignment; no report proliferation; the 409/PUT machinery in
  `vtn.rs` already exists.
- **Cons**: needs verification that the VTN (openleadr-rs) accepts growing interval
  lists idempotently; unbounded interval growth needs a cap/window policy; larger
  design surface than A.

### Recommendation

1. **Option A** — smallest change that makes the behaviour true to the descriptor,
   with the naming decision resolved *toward C's shape*: keep one `reportName` per
   obligation and let each period's submission carry the trailing window of intervals
   (bounded, e.g. last N periods), so upsert-overwrite becomes a feature (VTN always
   holds the freshest window) rather than a bug. This is A's effort with most of C's
   cleanliness.
2. **Option B** — legitimate if the next milestones are swarm behaviour rather than
   cert readiness; in that case update the cert backlog row from "Full" (it is not) and
   leave the wiki gap row as the tracked record.

---

## Cross-cutting notes for the implementation plan

- **One refactor arc, not six PRs in isolation**: R5 → R3 → (R1+R4) share files
  (`tasks/planning.rs`, `reporter.rs`, `entities/`); sequencing them avoids re-touching.
  R6 is independent; R2 stage 1 is independent, stage 2 waits for the rest.
- **Every code item lands with the usual gates**: test-first for new behaviour (R6),
  full VEN pyramid green, `cargo fmt`/`clippy -D warnings`, and E2E on Pi4 before merge
  (`.claude/CLAUDE.md` §testing) — the R4/R1 splits are pure refactors and must be
  behaviour-neutral (tests unchanged and green before/after).
- **Rule-file edits** (`.claude/CLAUDE.md` in R1/R4) are owner decisions — this document
  proposes wording; the human applies or amends it.
- **Wiki follow-up**: each resolved item closes its `wiki/review.md` entry and clears
  the corresponding DRIFT callouts (`bash scripts/wiki_callouts.sh` after edits);
  `TECHNICAL_DEBTS.md` gets the R5 inventory transfer regardless of which options are
  chosen.
