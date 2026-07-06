# Phase 4 — Comfort & Personas

> **Goal:** the resident's intent, comfort preferences and trust become first-class
> (SG-5), and exactly those features turn the fleet *diverse* — personas re-run the
> Phase-3 experiments with measurably different collective behaviour.
> **Items:** BL-28 (UserRequestMode), BL-19 (DefaultValueCurve), BL-20
> (notifications), BL-07 (StaleRatePolicy), NEW personas + experiment re-run.
> **Prerequisites:** Phase 3 (experiment harness exists to measure the diversity).
> **Exit demonstration:** S-2/S-3/S-4 re-run with a fleet of 3 personas × N VENs;
> the experiment report shows persona-segmented KPIs with visible behavioural spread.
> **Total effort:** ~3–4 weeks.

## WP4.1 — BL-28: `UserRequestMode` (M–L, the core of this phase)

Six modes are documented (REQUIREMENTS §3.2.1): ASAP, ASAP_FREE, BY_DEADLINE,
BY_DEADLINE_FREE, MAX_COST, OPPORTUNISTIC. Implement **incrementally, three PRs**:

1. **PR-a (plumbing):** add `mode: UserRequestMode` to `UserRequest` / `EvSession` /
   `HeaterTarget` / `ShiftableLoad` construction, default `BY_DEADLINE` (today's
   implicit behaviour — zero behavioural change, all existing tests stay green).
   Route + UI accept the field.
2. **PR-b (the two poles):** `ASAP` (allocate at maximum feasible rate from now,
   cost-blind) and `OPPORTUNISTIC` (no deadline constraint; allocate only in slots
   where marginal cost ≈ 0, e.g. PV surplus or negative tariff). Unit tests per the
   BL-28 verify clause: same session parameters, distinguishably different solver
   allocations — `test_mode_asap_vs_opportunistic_allocations_differ`.
3. **PR-c (remaining variants):** `MAX_COST` (budget cap as hard constraint +
   `PlanInfeasible`-style warning when unreachable → produces a BL-20 notification),
   `*_FREE` variants (only zero-marginal-cost energy, with/without deadline).
   Each mode: one planner unit test + one BDD scenario on the EV path.

Planner work lands in `controller/milp_planner`'s session-intent translation — check
`docs/reference/TECHNICAL_DEBTS.md` for that area first (refactoring rule) since the
3-tier refactor branch touches the same code; **merge the current
`refactor/3-tier-milp` branch before starting this WP.**

## WP4.2 — BL-19: user comfort-curve override (S–M)

1. `POST /assets/{id}/comfort_curve` accepting a `Vec<ComfortRate>` (wire shape =
   existing `ComfortRate`, DTO-passthrough); validation: monotonic, bounded.
2. Persist in the Phase-1 SQLite store (new `user_settings` table:
   `(key, asset_id, value_json, updated_at)`) so overrides survive restarts —
   extend `HistoryPort` (or a sibling `SettingsPort` if HistoryPort would bloat;
   decide at proposal time, lean to sibling for single-responsibility).
3. Planner prefers the override over `default_comfort_rates()` when present
   (BL-19 verify clause); `DELETE` route restores default.
4. UI: curve editor on the asset page (simple table of rate points is enough — no
   need for a graphical editor yet).

## WP4.3 — BL-20: notification feed (M)

1. Domain: `UserNotification { created_at, severity, message, asset_id?, event_id? }`
   with the existing `UserNotificationSeverity` (Info/Warn/Alert).
2. Bounded in-memory ring (mirror the `/trace/events` pattern) **plus** append to the
   Phase-1 `notifications` table so history survives restarts.
3. Producers at the trigger points the enum's doc comments name: tier fallback,
   budget warning (WP4.1 MAX_COST), deadline at risk, packet abandoned, grid
   emergency shed (Phase 3 WP3.1), stale-rate fallback (WP4.4), VTN unreachable
   (Phase 2). Producer calls go through a small application-layer service so inner
   rings don't gain outward deps.
4. Routes: `GET /notifications?since=` + SSE stream (mirror `/plan/events`); UI:
   badge + feed panel. Test-first: each producer condition yields exactly one
   notification of the expected severity (use-case tests with mocks).

## WP4.4 — BL-07: `StaleRatePolicy` dispatch (M)

1. In planner Phase 1 (`build_grid`): detect slots beyond tariff coverage; apply the
   profile-configured policy: `LAST_KNOWN` (repeat), `DEFER_TO_FLEXIBLE` (force
   FLEXIBLE), `SAFE_AVERAGE` (configurable percentile of known rates),
   `HEURISTIC_FORECAST` (**stub until Phase 5 BL-14** — falls back to LAST_KNOWN with
   a Warn notification; document the stub in code + BACKLOG).
2. Unit test per the BL-07 verify clause: 2 h of rates on a 6 h horizon → each policy
   yields different slot classifications/costs.
3. Emits a WP4.3 notification when the policy activates (comfort-trust link).

## WP4.5 — Personas + diverse-fleet experiment re-run (M)

1. Three persona presets as profile fragments (pure config — no new code beyond
   WP4.1/4.2 fields):
   - **eco-optimizer:** OPPORTUNISTIC/`*_FREE` defaults, aggressive comfort curves,
     small temp band.
   - **comfort-first:** ASAP defaults, flat comfort curves, wide budget.
   - **absent-commuter:** EV BY_DEADLINE 07:00, low daytime base load, MAX_COST cap.
2. `fleet.sh up N --personas eco:0.4,comfort:0.4,commuter:0.2` — generator assigns
   persona fragments by ratio (seeded).
3. Re-run S-2 (dynamic tariff), S-3 (capacity limit), S-4 (emergency) with the mixed
   fleet; extend `kpi.py` with persona segmentation. The exit report should show e.g.
   eco-optimizers shifting hard on price while comfort-first barely move — if the
   spread is *not* visible, that's a real finding about the control method, write it
   up either way.

## Order & risks

```
WP4.1-a → WP4.1-b → WP4.1-c        (core track)
WP4.2, WP4.3, WP4.4                 (independent, parallelizable)
WP4.5 last (needs 4.1-b minimum; better after 4.1-c + 4.2)
```

Risks: (a) mode semantics in the MILP can interact with the 3-tier/adoption-gate
logic — the PR-a plumbing-first approach isolates that risk to PR-b/c; (b) merge
conflict with `refactor/3-tier-milp` — sequence this phase strictly after that branch
lands; (c) `MAX_COST` infeasibility UX — the notification (not a hard error) is the
designed behaviour, test it explicitly.

Bookkeeping: mark BL-28/19/20/07 resolved; persona presets documented in
`VEN/profiles/README` (create if missing); journal + `/wiki-sync`
([[milp-planner]], [[ven-ui]], new personas/notifications coverage).
