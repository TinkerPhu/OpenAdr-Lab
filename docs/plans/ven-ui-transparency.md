# VEN UI Transparency — Plan

> **Date:** 2026-07-18
> **Status:** proposed, not started
> **Context:** the VEN UI (`VEN/ui/`) does not transparently surface much of what the VEN
> backend actually does. This plan addresses the observability gaps found in a code
> survey (see §1) and reorganises the UI's 11 tabs around actual usage frequency and
> clearer visual communication (§3). No code has been written yet — this is the design
> document; implementation follows the phase/WP conventions in
> `docs/plans/roadmap/README.md`.

---

## 1. Gaps found (recap of the survey)

| Gap | Backend already has it? | Currently surfaced how |
|-----|--------------------------|-------------------------|
| G-1 VTN connection health | Partially (`poll_success_total`/`poll_error_total` counters, `tasks/backoff.rs` delay state, token expiry in `vtn.rs`) | Only as unlabeled numbers in the raw `/metrics` Prometheus dump. `/health` (`VEN/src/routes/system.rs`) is a hardcoded `"ok"` string — the Dashboard health chip is actively misleading. |
| G-2 MILP solve outcome | Yes (`PlanInfeasible` in `VEN/src/entities/error.rs:15`, solve status internally) | Collapses into the same generic `warnings[]` string as everything else in `PlanHeaderBar.tsx`; no distinct infeasible/fallback badge, no objective value shown. |
| G-3 Background task status | Yes (`supervised_spawn` restart-on-panic in `VEN/src/tasks/mod.rs`; each task in `VEN/src/tasks/` runs on its own schedule) | Not surfaced at all — no route, no UI. |
| G-4 Persistent error/event log | Partially (`DomainError` variants map to HTTP error responses on direct mutations) | Only shown ad hoc as a thrown JS error in `client.ts` for the request that triggered it. Background-task errors (e.g. `VtnUnreachable` during a poll) have no route and are silently server-log-only. |
| G-5 VTN report submission status | Yes (`reports_sent_total` counter in `VEN/src/routes/reports.rs:23,44`) | Reports page shows local report objects; "was this actually accepted by the VTN" is only in raw `/metrics`. |
| G-6 Unwired existing routes | Yes — routes exist and work | No UI caller at all for `/forecast`, `/forecast/:asset_id`, `/capability/:asset_id`, `/history/plans`, `/obligations`, `/notifications/events` (SSE), `/sim/inject/reset`, `/sim/config/battery`, `/plan/trigger`, `/debug/heuristics/preload`, `/history/:asset_id` |
| G-7 Metrics page is unlabeled | Yes | `MetricsPage.tsx` renders the raw Prometheus text as a generic table — no grouping by meaning, no thresholds/colour |

Not a gap: simulator/asset live state (SoC, power, temperature) is already reasonably
covered via `/sim` and `/timeline` on Controller/Devices/Dashboard.

---

## 2. Design principles for this plan

1. **Don't invent new backend concepts where wiring is enough.** G-5 and part of G-6
   need only a UI fetch + display; G-1/G-2/G-3/G-4 need a small new route or route
   extension because the data either doesn't exist in the right shape yet or has no
   route at all.
2. **Status, not just data.** Every new indicator distinguishes *connected/degraded/
   failed* (or *optimal/fallback/infeasible*), not just raw numbers — the whole point
   of this plan is legibility, not more data density.
3. **Keep the error/event log separate from Notifications, on purpose.** Notifications
   concerns the resident's personal plans and requests with the VEN (deadline-at-risk,
   tier fallback, request accepted/rejected). The new Event Log (G-4) concerns the
   VEN's own operational health (VTN unreachable, storage error, task restarted). These
   answer different questions for different mental models — "did my thing work" vs.
   "is the system working" — and merging them would make the resident-facing feed
   noisy with things they can't act on. Two feeds, two routes, two pages.
4. **No new persistent stores where in-memory + `/metrics` counters already exist** —
   G-1/G-3 can be answered from process state at request time; only G-4 needs a ring
   buffer (bounded, in-memory) since past errors from background tasks would otherwise
   vanish before anyone looks.

---

## 3. UI re-architecture — usage-frequency-based tab layout

Current tabs (11, flat list, alphabetical-ish order): Dashboard, History, Controller,
Planner, Devices, Programs, Events, Reports, Metrics, RawDiagnostics ("Raw Data"),
Notifications.

### 3.1 Usage-frequency reasoning

- **Highest frequency / glance-and-go** (checked many times a day, at-a-glance status
  only): Dashboard, Notifications.
- **Frequent / interactive** (adjusted, read, or reviewed during active use): Devices,
  Controller, History, Planner. Devices sits right after Dashboard, ahead of the rest
  of this group — it's the tab a resident actually touches most (EV session, heater
  target, shiftable loads, comfort overrides), so it gets the shortest path from
  landing. History sits with Controller/Devices rather than in the investigative
  group — it's the continuation of the Controller's live view into the past (same
  timelines, same assets, just scrolled back), not a separate occasional concern.
- **Occasional / investigative** (opened when something looks wrong or for a periodic
  review of what the VTN has sent): Reports, Programs, Events.
- **Always-on diagnostics** (not rare, not hidden — see §3.2 note on visibility):
  Metrics, RawDiagnostics, Tasks, Event Log.

The current flat list treats all 11 the same weight, and specifically buries the
things a resident would check constantly (Dashboard, Notifications, History) among
tabs they'd open once a week (Programs, Events, RawDiagnostics).

### 3.2 Proposed structure

```
Primary nav (top-level):
  ⦿ Dashboard        — status-first landing page (see 3.3)
  ⦿ Devices          — device config/session; most frequently interacted-with tab
  ⦿ Controller       — live control
  ⦿ History          — continuation of Controller into the past
  ⦿ Planner          — plan + solve status
  🔔 Notifications    — badge with unread count, same position always

Secondary nav ("VTN Feed" group, collapsed by default):
  Reports
  Programs
  Events

Diagnostics nav (own top-level group, always visible — see §5 revised, no gating):
  Metrics
  RawDiagnostics
  Tasks              — NEW (G-3)
  Event Log          — NEW (G-4), separate from Notifications (see §2 principle 3)
```

Rationale: reduces the top-level decision from 11 flat tabs to 6 primary + 2 grouped
menus, with History promoted next to Controller/Devices rather than grouped with the
VTN-facing review tabs. Reports/Programs/Events stay grouped together because they're
all "what did the VTN tell us" — currently split across 3 tabs a user has to check
separately. Diagnostics is its own always-visible group, not hidden behind a mode
flag (transparency is the point of this whole plan — see §5).

### 3.3 Dashboard redesign

The Dashboard becomes the answer to "is everything okay right now," combining
existing widgets with the new status ones:

```
┌─────────────────────────────────────────────────────────┐
│ VTN Connection: ● Connected   Last poll: 4s ago          │  ← NEW (G-1)
│ Plan status: ✓ Optimal (solved 45s ago)                  │  ← NEW (G-2, replaces generic warning text)
│ Active tasks: 8/8 running                                │  ← NEW (G-3, collapses to a single line unless degraded)
├─────────────────────────────────────────────────────────┤
│ [existing signal/health strip, asset snapshot cards]     │
└─────────────────────────────────────────────────────────┘
```

Each status line is a traffic-light row: green text/no expansion when healthy;
expands inline to show detail (retry countdown, infeasibility reason, which task is
down) only when degraded. This keeps the glance-and-go property — a healthy system
shows three short green lines, not a wall of gauges.

---

## 4. Work packages

Effort tags per roadmap convention: S ≤ ½ day · M ≈ 1–2 days · L ≈ 3–5 days.

| WP | Item | Backend change | UI change | Effort |
|----|------|-----------------|-----------|--------|
| WP-T1 ✅ | VTN connection status + multi-component health (G-1) — **done**, branch `032-vtn-health-status`, `openspec/changes/wp-t1-vtn-health-status/`. `/health` now `{status, components: {ven_process, vtn_connection, storage, planner}}`; new `GET /vtn/status`. Fixed the existing health chip's actual misleading-truthiness bug in the process | Existing Dashboard health chip now reads real `status` (was always "ok" on any truthy response); full independently-coloured widget redesign still WP-T8 | M |
| WP-T2 ✅ | MILP solve status badge (G-2) — **done**, branch `031-plan-solve-status`, `openspec/changes/wp-t2-plan-solve-status/`. Shipped as a two-state `solve_status: OPTIMAL \| INFEASIBLE` (no `fallback_heuristic` — no such code path exists; see design.md Non-Goals) | `PlanHeaderBar.tsx`: distinct infeasible chip vs. generic warning badge (Dashboard summary line deferred to WP-T8) | S |
| WP-T3 ✅ | Background task status (G-3) — **done**, branch `033-task-status`, `openspec/changes/wp-t3-task-status/`. `GET /tasks/status` reports `{name, last_run_ts, last_success, restart_count}` per task actually spawned (`supervised_spawn` now records it — it tracked nothing before this WP) | New Tasks page shipped (Diagnostics-adjacent nav); Dashboard summary line deferred to WP-T8 | M |
| WP-T4 ✅ | Event log (G-4) — **done**, branch `035-event-log`, `openspec/changes/wp-t4-event-log/`. Bounded in-memory ring (no persistence, no `/events/log/history` — see design.md) + `GET /events/log` + `GET /events/log/events` (SSE), independent of Notifications — `vtn_connection`/`storage`/`task_supervisor` producers wired | New Event Log page shipped (polling, not yet SSE-wired) | M |
| WP-T5 | VTN report submission status (G-5) | No backend change — `reports_sent_total` already exists; add a per-report `vtn_accepted: bool` field at creation time if not already tracked, else just surface the existing counter contextually | Reports page: per-report submission status chip, not just a raw counter elsewhere | S |
| WP-T6 ✅ | Wire unused routes (G-6) — **done**, branch `037-wire-unused-routes`, `openspec/changes/wp-t6-wire-unused-routes/`. Wired `/capability/:asset_id` + `/forecast` (Controller), `/history/plans` (History), `/obligations` (Reports) | New `FlexibilityForecastPanel` on Controller; new Plans section on History; new Pending Obligations section on Reports | M |
| WP-T7 ✅ | Metrics page labeling (G-7) — **done**, branch `036-metrics-labeling`, `openspec/changes/wp-t7-metrics-labeling/`. Only 2 real categories exist (VTN Polling, Reports) — grep-confirmed no "tasks"/"HTTP" metrics are emitted, contra the original 4-category sketch | Grouped-by-default `MetricsPage.tsx` with human labels + raw-name captions; raw view toggle reproduces prior exact behavior | S |
| WP-T8 | Tab re-architecture (§3) | None | Implement primary/secondary/tertiary nav grouping; Dashboard status-row redesign | M |

### 4.1 WP-T1 detail — multi-state `/health`

Splitting `/health` into components is not bad design — it's the standard shape for
any system with more than one thing that can independently fail (Kubernetes
readiness/liveness detail, most `healthz` conventions). The failure mode to avoid is
the *opposite*: multiplying it into a dozen ad hoc booleans nobody aggregates. The
plan here is one endpoint, structured, with a small fixed set of components:

```json
GET /health
{
  "status": "degraded",              // overall: ok | degraded | down — for simple consumers (Docker healthcheck, fleet.sh)
  "components": {
    "ven_process":   { "status": "ok" },
    "vtn_connection":{ "status": "degraded", "detail": "backoff 45s, 3 consecutive failures" },
    "storage":       { "status": "ok" },
    "planner":       { "status": "ok" }
  }
}
```

- `ven_process`: the process is up and answering HTTP at all (trivially `ok` if this
  response is returned) — kept as its own key rather than assumed, so a future check
  (e.g. event-loop lag, memory pressure) has somewhere to report without a new field.
- `vtn_connection`: derived from the same poll-task state as WP-T1's `/vtn/status`.
- `storage`: derived from the SQLite/history-store health (last write succeeded).
- `planner`: derived from WP-T2's solve status (infeasible/fallback → `degraded`).

`status` at the top level is the worst of the four components, so existing consumers
that only check the top-level field (Docker healthchecks, `fleet.sh` health checks —
see open question 2 below) keep working unchanged; the UI reads `components` for the
Dashboard's per-widget colouring. This stays at four components — do not add a fifth
without a concrete failure mode it needs to distinguish.

Suggested order: **WP-T2 → WP-T1 → WP-T3 → WP-T4 → WP-T5 → WP-T7 → WP-T6 → WP-T8**.
Reasoning: T2 and T1 are the highest-value, most safety-relevant gaps (a misleading
health chip and no infeasibility signal) and are also the cheapest/most contained.
T8 (nav re-architecture) goes last because it should be informed by where the new
Tasks/Event Log surfaces actually landed, not designed twice.

Each WP follows the standing per-WP workflow (`docs/plans/roadmap/README.md`):
propose via `/openspec-propose` → branch `NNN-<slug>` → test-first → all four suites
green → PR to `main`. Architecture constraints apply as usual: new routes stay in
`routes/`, no new port needed since these read existing in-process task/poll state
rather than a new external integration.

---

## 5. Open questions — resolved

1. **WP-T4 Event Log vs. Notifications — resolved: fully separate (Option A).**
   The two feeds differ on enough independent axes that sharing one buffer/route
   causes real problems, not just conceptual untidiness:
   - **Frequency**: personal notifications are rare by design; system events (VTN
     retries during an outage) can fire every few seconds.
   - **Dedup semantics**: the existing `Notifier` (`VEN/src/services/notify.rs`)
     collapses repeats within a 30-min window into one bumped `count` — correct for
     "tell the resident once," wrong for diagnosing an outage where every retry's
     timestamp matters.
   - **Retention pressure**: a shared bounded ring means a noisy system-event burst
     can evict personal notifications the resident hasn't seen yet.
   - **Vocabulary/audience**: personal notifications read in resident language;
     system events are technical (`VtnUnreachable`, `backoff_s`, `restart_count`) —
     a shared list still has to branch per-row on category, so merging saves no
     rendering complexity.
   - **Consumption pattern**: personal notifications are "read once, dismiss";
     system events are a scrollable diagnostic log, closer to a log viewer than a
     notification list.

   Decision: two stores, two routes, two pages, as originally scoped in WP-T4 — the
   existing resident-facing Notifications feed (including its VTN-reachability
   producer per BL-35/BACKLOG.md) is untouched; the new Event Log is an independent
   mechanism for VEN-operational events. This does **not** collide with BL-35's
   planned producers (tier fallback / deadline-at-risk / packet abandoned) — those
   are unambiguously personal-plan concerns and stay in Notifications.

2. **WP-T1 `/health` response-shape change — resolved.** Checked every consumer:
   `fleet.sh:60`, `VEN/docker-compose.yml`, and `tests/docker-compose.test.yml` all
   use `curl --fail http://.../health`, which only checks the HTTP status code, never
   the body — safe for the shape change in §4.1. The one consumer that reads the
   literal body is `tests/features/ven_health.feature` (`the VEN response body is
   "ok"`), which needs updating to assert on the new JSON's `status` field instead.
   Additionally: `/health` must keep returning HTTP 200 whenever `ven_process` itself
   is healthy, even if `vtn_connection`/`storage`/`planner` are `degraded` — a VTN
   outage is not fixed by restarting the VEN container, and Docker's healthcheck exit
   code is a restart trigger, not a status display. `status: degraded` in the JSON
   body is how the UI learns about it; the HTTP code stays 2xx so `fleet.sh` and
   Docker don't start cycling containers during an outage the poll tasks are already
   retrying through on their own.

No question remains on hiding the Diagnostics group — decided: always visible,
top-level, no gating (§3.2), since transparency is this plan's goal, not an
optionally-revealed mode.

---

## 6. Bookkeeping

On completion of this plan's WPs: update `docs/BACKLOG.md` if any WP spawns a
follow-on item, `docs/history/project_journal.md`, `docs/reference/KEY_LEARNINGS.md`
if anything non-obvious surfaces (e.g. around `/health` semantics), and re-check
`docs/reference/TECHNICAL_DEBTS.md` for any items touched by the routes/tasks work.

---

## 7. Implementation order & plan

Each WP is its own branch/PR per the standing workflow (`docs/plans/roadmap/README.md`):
propose via `/openspec-propose` → branch `NNN-<slug>` → test-first → all four suites
green → PR to `main`, rebase + fast-forward merge. Effort tags: S ≤ ½ day · M ≈ 1–2
days · L ≈ 3–5 days.

### Dependency chain

```
WP-T2 ──► WP-T1 ──► WP-T3 ──► WP-T4
                        │         │
WP-T5 ──────────────────┤         │
WP-T7 ──────────────────┤         │
WP-T6 ──────────────────┴─────────┴──► WP-T8
```

T2/T1/T3/T4 are sequenced (each Dashboard widget in §3.3 needs the previous one's data
shape settled first: plan status → connection status → task summary → the separate
Event Log feed). T5/T6/T7 have no dependency on each other or on T2–T4 and can be
picked up in parallel by a second contributor, or interleaved between T1–T4 if solo.
T8 (nav re-architecture) is last because it surfaces WP-T3's Tasks page and WP-T4's
Event Log inside the Diagnostics group, and WP-T1/T2's Dashboard widgets — it cannot
be meaningfully built before those exist.

### WP-T2 — MILP solve status badge (S) — ✅ done

Branch `031-plan-solve-status`; OpenSpec change
`openspec/changes/wp-t2-plan-solve-status/` (proposal/design/specs/tasks all
complete). Journal entry in `docs/history/project_journal.md`.

Shipped as scoped, with one deliberate narrowing from the original wording below:
`solve_status: Optimal | Infeasible` — **no** `FallbackHeuristic` variant, because
code investigation found no distinct heuristic-solve path exists anywhere in this
codebase (`fallback_plan` *is* the infeasibility path, not a separate heuristic
substitute). Documented in the OpenSpec proposal/design's Non-Goals; a third state
is a small follow-up once a real heuristic-solve path exists (candidate: BL-13).

1. ~~In `controller/milp_planner`, locate where solve status/objective are already
   computed internally (solver returns them) but discarded before reaching the `Plan`
   returned to callers.~~ Added `solve_status: SolveStatus` (`entities/plan.rs`) —
   `objective_eur`/`friction_eur` already existed, just weren't reaching the SSE/UI
   types (fixed in step 3/4).
2. Test-first, done: `test_plan_carries_infeasible_status_on_unsolvable_constraints`
   (extends the existing `run_planner_infeasible_constraints_fallback_no_panic`
   fixture) and `test_plan_carries_optimal_status_and_objective_value`
   (`controller/milp_planner/tests/planner.rs`), plus
   `solve_status_serializes_as_screaming_snake_case` (`entities/plan.rs`) and
   `test_plan_ready_event_solve_status_matches_plan` (`services/planning.rs`).
3. Extended the `PlannerEvent::PlanReady` SSE payload (`/plan/events`) with
   `solve_status` (and surfaced the previously-undeclared `friction_eur`).
4. UI: `PlanHeaderBar.tsx` renders a distinct `plan-infeasible-chip` separate from
   the generic `warnings[]` badge. Dashboard's first status line is deferred to
   WP-T8 (needs the Dashboard rebuild itself, not just this data).
5. BDD: no scenario added — the existing infeasibility test double
   (`InfeasibleBatCtx`) is unit-test-only and not exposed at the BDD/E2E layer.
   Deferred as **GB-12** in `docs/BACKLOG.md` rather than forced into this WP.

### WP-T1 — Multi-component `/health` + `/vtn/status` (M) — ✅ done

Branch `032-vtn-health-status`; OpenSpec change
`openspec/changes/wp-t1-vtn-health-status/` (proposal/design/specs/tasks all
complete). Journal entry in `docs/history/project_journal.md`.

Investigation found the plan doc's assumption in step 1 below (originally: "confirm
what per-task state already exists ... expose it via a small shared read accessor")
did not hold — `Backoff` and the poll loop's `vtn_ok` flag are stack-local variables
with no external visibility today, and `state_persist.rs` only logs failures.
Shipped by adding new **in-memory, process-lifetime-only** shared state (not
persistence) on `AppState`, written from `poll_events.rs` — the existing canonical
outage-detection loop in this codebase (it already drives `notify_outage_edge`).

1. ~~confirm what per-task state already exists~~ → added `VtnConnectionStatus`
   (`state/connection.rs`, extracted there to stay under `state/mod.rs`'s file-size
   cap) + `storage_ok: bool`, both on `AppState`.
2. New route `GET /vtn/status` → `{connected, last_success_ts, last_error,
   current_backoff_s, token_expires_at}`. `token_expires_at` required a new
   `VtnClient::token_expires_at()` accessor deriving wall-clock time from the
   existing monotonic `Instant`-based token expiry.
3. Rewrote `GET /health` to the `{status, components: {ven_process, vtn_connection,
   storage, planner}}` shape from §4.1. `planner` component reads WP-T2's
   `solve_status` (infeasible → `degraded`) — no new state needed there, a direct
   payoff of WP-T2 landing first. HTTP status stays 200 regardless of component
   status (`ven_process` being reachable at all is the only thing a restart could
   fix) — see §5 Q2 resolution.
4. Test-first, done: 8 unit tests across `routes/system.rs` (health/vtn_status pure
   builders, kept separate from the handlers for testability without constructing a
   full `AppCtx`), `state/connection.rs`, and `vtn.rs`.
5. Updated `tests/features/ven_health.feature` + step defs to assert on the JSON
   `status` field and all four component keys, replacing the literal-`"ok"`
   assertion.
6. **Empirically re-verified on Pi4** — deployed via scp, rebuilt `ven-1/2/3`;
   `docker ps` reported all three `Up ... (healthy)`, `curl --fail` returned HTTP
   200/exit 0, `/health` and `/vtn/status` returned the expected shapes with real
   data. Confirms the healthcheck-compatibility reasoning live, not just on paper.
7. UI: fixed the existing Dashboard health chip (`App.tsx`'s `HealthChip`) — it
   previously rendered `"ok"` whenever *any* truthy response arrived, which is
   exactly the misleading-chip bug this WP targets (the old plain-string body was
   always truthy). Now reads `data.status` with an added `"degraded"` state. A
   dedicated separate connection widget + VEN-process widget is deferred to WP-T8's
   Dashboard rebuild — this WP made the existing chip truthful, not yet redesigned.

**Notable deviation**: implementing this required a file-size-driven refactor —
adding the new state/route logic pushed `state/mod.rs` and `tasks/poll_events.rs`
over their respective caps (500 and 200 production lines). Fixed by extracting
`state/connection.rs` (mirrors the existing `state/heuristics.rs`/`obligations.rs`
pattern) and moving `poll_events.rs`'s two new call sites into `tasks/backoff.rs`
helpers (`record_success`/`record_fail_sleep`) rather than inlining them in the
poll loop.

### WP-T3 — Background task status (M) — ✅ done

Branch `033-task-status`; OpenSpec change `openspec/changes/wp-t3-task-status/`
(proposal/design/specs/tasks all complete). Journal entry in
`docs/history/project_journal.md`.

Investigation found `supervised_spawn` tracked **nothing** queryable before this WP
(confirmed — no counter, no timestamp, purely local to each restart loop's stack
frame) — step 1's "if not already tracked" resolved to "not tracked, added from
scratch," same pattern as WP-T1's `VtnConnectionStatus` finding.

1. Added `TaskStatus { last_run_ts, last_success, restart_count }` on `AppState`
   (new `state/task_status.rs` submodule), threaded `AppState` into
   `supervised_spawn` itself (one parameter, `state.clone()` at all 9 call sites in
   `main.rs`, all already had `state` in scope).
2. New route `GET /tasks/status` → array, one entry per task **actually spawned**
   (not a fixed 9-or-10 — `state_persist`/`history_sampler`/`heuristics_job` are
   config-conditional). **`progress_ticker` excluded** — it's not a top-level
   supervised task; it's spawned/cancelled per plan-solve-cycle inside
   `spawn_planning`'s own loop with a cancel-and-await lifecycle, not a restart
   lifecycle, so the plan doc's original list of 10 was one item too many.
3. Test-first, done — plus one deviation: the exact-count assertion
   (`restart_count == 1`) in the updated `supervised_spawn_restarts_after_panic`
   test proved flaky (the test's `cooldown_s = 0` lets the supervisor loop race
   far ahead of the test's 10ms polling; observed `restart_count == 9` on a real
   run). Relaxed to `>= 1` — the exact count was never a real invariant.
4. UI: new Tasks page shipped (table: name/last-run/outcome/restart-count,
   `restart_count == 0` as the healthy signal since `last_success` is legitimately
   `null` for a still-running task). Wired into nav next to Raw Data/Metrics: full
   Diagnostics-group nav restructuring is still WP-T8. Dashboard's third status
   line ("Tasks: N/N running") also deferred to WP-T8's Dashboard rebuild.

**Unplanned incident during this WP**: a background `cargo test` run coincided
with an unrelated concurrent `wsl cargo check` from a different worktree
(`.claude/worktrees/034-vtn-report-status`), dropping free host memory to 0.2 GB —
under the memory-budget rule's ~1 GB floor. Resolved by stopping this session's own
WSL process (not the other worktree's). Prompted a new `wsl-lock` rule +
`scripts/wsl_lock.sh` in `.claude/CLAUDE.md`, mirroring `pi4_lock.sh` — outside this
WP's scope but noted here since it happened during it.

### WP-T4 — Event Log, separate from Notifications (M) — ✅ done

Branch `035-event-log`; OpenSpec change `openspec/changes/wp-t4-event-log/`
(proposal/design/specs/tasks all complete). Journal entry in
`docs/history/project_journal.md`.

Per the resolved Option A design (§5 Q1): an independent mechanism, not a shared
buffer with Notifications. Shipped with two scope narrowings from the original
sketch, both documented in design.md:

1. Domain: `EventLogEntry { id, created_at, category, message }` — **no separate
   `detail` field**. Every real producer (connection failure, storage error, task
   panic) has exactly one string worth recording; the sketch's `message`+`detail`
   split had no site that actually needed two fields, so adding it would have been
   speculative.
2. ~~New service mirroring `Notifier`'s shape as its own struct~~ → plain
   `AppState` methods instead (`state/event_log.rs`, mirrors `state/connection.rs`/
   `task_status.rs`). Every producer site already receives `AppState` (threaded
   there by WP-T1/WP-T3 for the same reason) — a separate service struct would have
   meant new `AppCtx` fields and new clone captures at every `main.rs` spawn block
   for no benefit "Option A: separate" actually required. Bounded ring + broadcast,
   no dedup, as planned — but **no persistence** for this first cut (see design.md
   Non-Goals: a `HistoryPort` trait method + migration is a bigger yak-shave than
   this WP's effort budget for diagnostic, not personal, notices).
3. Producers wired exactly where WP-T1/WP-T3 already touched: `tasks/backoff.rs`
   (`record_fail_sleep` — the *only* place, since `tasks/poll_events.rs` was at
   **exactly** 200/200 lines with zero headroom), `tasks/state_persist.rs`'s two
   failure branches, `tasks/mod.rs`'s `supervised_spawn` completion branches.
4. Routes: `GET /events/log` (snapshot) + `GET /events/log/events` (SSE, copying
   `/notifications/events`'s exact bridge pattern). **No `/events/log/history`** —
   with no persistence, it would return exactly what `/events/log` already does;
   dead API surface deferred until persistence is added, if ever.
5. Test-first, done — unit tests in `state/event_log.rs` (ring/broadcast behavior),
   `tasks/backoff.rs` and `tasks/mod.rs` (producer sites recording correctly).
6. UI: new Event Log page shipped, polling (`useEventLog`, 10s refetch) rather than
   consuming the SSE stream — consistent with every other Diagnostics page; SSE
   wiring into the UI is a follow-up, not dropped, just not this WP's scope.

**Unplanned incident during this WP**: a second resource-contention episode with
the same other worktree as WP-T3's — this time it compiled `HiGHS` (a C++ solver)
from scratch concurrently with this session's `wsl_lock`-held test run, dropping
free memory to 1.0 GB. The other session did not appear to respect `wsl_lock`.
Resolved by killing this session's own (already-redundant, post-fmt) re-test run.

### WP-T5 — VTN report submission status (S)

1. In `routes/reports.rs`, check whether per-report acceptance is tracked anywhere
   beyond the aggregate `reports_sent_total` counter; if not, add a `vtn_accepted:
   bool` field set at submission time alongside the existing counter increment.
2. Test-first: `test_report_submission_marks_vtn_accepted_on_success_and_false_on_failure`.
3. UI: Reports page gets a per-report status chip instead of the status only being
   visible via raw `/metrics`.

### WP-T7 — Metrics page labeling (S) — ✅ done

Branch `036-metrics-labeling`; OpenSpec change
`openspec/changes/wp-t7-metrics-labeling/`. UI-only, no backend change.

**Scope correction**: grep across `VEN/src`'s `counter!`/`histogram!`/`gauge!`
call sites found only two real metric families — `poll_success_total`/
`poll_error_total` (VTN Polling) and `reports_sent_total` (Reports) — not the
four categories ("VTN polling / reports / tasks / HTTP") originally sketched.
`PrometheusBuilder::new()` has no HTTP-instrumentation middleware installed and
no per-task metrics are recorded anywhere (WP-T3's task status lives on
`/tasks/status`, not the Prometheus registry). Built a grouping map covering
what's real, with an "Other" fallback (raw name, ungrouped-looking but still
under a heading) for anything else — including any future metric — so nothing
is ever hidden by an incomplete map.

1. Grouped `MetricsPage.tsx` by meaning via a static name→`{group, label}`
   lookup (not a naming-convention parser — see design.md D1 for why), default
   view.
2. Raw-view `Switch` toggle reproduces the page's exact pre-change flat/raw-name
   rendering — same `MetricTable` component reused by both views, which is why
   every pre-existing test in `Metrics.test.tsx` passed unmodified (373/373
   total after adding 3 new tests for grouping/toggle/fallback behavior).

### WP-T6 — Wire unused routes (M) — ✅ done

Branch `037-wire-unused-routes`; OpenSpec change
`openspec/changes/wp-t6-wire-unused-routes/`. Journal entry in
`docs/history/project_journal.md`.

1. Added client methods for `/capability/:asset_id`, `/forecast`,
   `/history/plans`, `/obligations`. **`/forecast/:asset_id` excluded** — it's a
   different concept from the bare `/forecast` despite sharing a path prefix
   (a physics-model forward sample series requiring a timespan control, vs. the
   bare route's plan-cycle `AssetForecast[]`) and materially overlaps with the
   existing Timeline/RawDiagnostics charts — wiring it would be new chart UI,
   not a contained "surface existing data" change.
2. Placement, per the "favour an existing page" principle: `/capability`+
   `/forecast` → new standalone `FlexibilityForecastPanel` on Controller
   (deliberately not new `AssetCell` props — see design.md D1); `/history/plans`
   → new Plans section on History (reuses the existing `JsonDialog` for plan
   detail); `/obligations` → new Pending Obligations section on Reports
   (client-computed Pending/Overdue/Fulfilled status, no server field for it).
3. `/notifications/events` SSE **not wired into the UI** — consistent with
   WP-T4's precedent (backend route exists and works; UI consumption is a
   follow-up, not dropped).

### WP-T8 — Nav re-architecture + Dashboard redesign (M)

1. Implement the primary/secondary/Diagnostics grouping exactly as ordered in §3.2:
   Dashboard, Devices, Controller, History, Planner, Notifications (primary); Reports,
   Programs, Events (VTN Feed group); Metrics, RawDiagnostics, Tasks, Event Log
   (Diagnostics group, always visible, no gating).
2. Rebuild the Dashboard per §3.3: the three status lines from WP-T2 (plan status),
   WP-T1 (VTN connection + VEN-process), WP-T3 (task summary), each collapsed to a
   single green line when healthy and expanding inline with detail only when
   degraded.
3. Update `App.test.tsx` and any routing/nav tests for the new tab structure.
4. Manual pass: verify the "glance-and-go" property holds — a healthy system shows
   three short lines on the Dashboard, not a wall of gauges (design principle 2, §2).
