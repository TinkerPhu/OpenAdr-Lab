## 1. Preparation

- [x] 1.1 Follow `docs/reference/SESSION_START.md`. Check `docs/reference/TECHNICAL_DEBTS.md` for
      entries touching `capacity_headroom.rs`, `simulator/forecast.rs`, `SiteHeadroomChart.tsx`,
      or `TimeSeriesChart.tsx`, and refactor any Small/Trivial one first
- [x] 1.2 Create branch `044-movable-capacity-curve-start` (next free openspec feature ID) in a
      worktree if the shared main checkout is busy

## 2. Backend core: one curve computation, parameterized by starting states (D4)

- [x] 2.1 Test first: a unit test asserting the new core, called with `t1 = now` and live states,
      reproduces `compute_site_capacity_curve`'s output for a mixed fleet (battery, EV, heater,
      PV, base load, shiftable load), in both directions. Confirm it fails to compile/run
- [x] 2.2 Split `compute_site_capacity_curve` into a core taking `t1`, `t2_max` and a per-asset
      starting-state source, and keep the existing signature as a thin wrapper over live states.
      Test 2.1 green, all existing `capacity_headroom` tests unchanged and green
- [x] 2.3 Expose which boundary `resolve_plan_state_at` snapped to (return value or small
      sibling), without re-implementing the snapping. Extend its unit tests to cover the returned
      boundary (mid-slot, exact boundary, past last slot)
- [x] 2.4 Test first: `compute_site_capacity_curves_at` unit tests covering mid-slot snapping,
      horizon end = plan `horizon.end_time − t1`, clamping past the last slot, and "plan
      charges battery before `t1` → shorter import curve than at now"
- [x] 2.5 Implement `compute_site_capacity_curves_at(sim, plan, start, now, phys_imp_kw,
      phys_exp_kw)` in `controller/capacity_headroom.rs` on top of `resolve_plan_state_at` + the
      core, and remove `resolve_plan_state_at`'s `#[allow(dead_code)]`
- [x] 2.6 Test first: seam-at-future-slot test, where for each asset kind (and a mixed fleet) the
      curve's `steps[0].power_kw` at a future slot equals `compute_site_headroom_forecast`'s
      `down_kw` (import) / `up_kw` (export) for that slot
- [x] 2.7 Make 2.6 green. If PV or base load diverge, find which side answers the question
      wrongly and fix it. If the requirement itself looks wrong, stop and raise it with the user
      (spec change). Don't add a tolerance

## 3. Backend route (D2, D5, D6)

- [x] 3.1 Add `grid_max_import_kw` / `grid_max_export_kw` to `AppCtx`, filled from
      `profile.grid` in `main.rs`. Update every `AppCtx` construction in tests
- [x] 3.2 Test first: route cases for `get_capacity_curves`: no `start` → stored curves +
      top-level `start`; past `start` → same; future `start` with a plan → snapped curves;
      future `start` without a plan → now curves with their own `start`; malformed `start` →
      400; still 204 before the first tick. (VEN has no router-level test harness: the
      decision logic is `compute_site_capacity_curves_at`'s `None` contract, unit-tested in
      group 2; the HTTP cases are the BDD scenarios in 7.1.)
- [x] 3.3 Implement the optional `start` query in `routes/hems/sessions.rs`: lock `ctx.sim`,
      compute synchronously, drop the lock before serializing, no `.await` while held. Pass the
      wall clock as the injected `now`. Add a `debug!` timing log around the computation
- [x] 3.4 Update the route's doc comment and check `scripts/audit_file_sizes.py` (sessions.rs,
      capacity_headroom.rs under 500 production lines)

## 4. Compute measurement (Risks)

- [ ] 4.1 Build the branch on Node2 and run ven-1-like profile, then time a sweep of
      future-start requests (e.g. one per slot across the next 8 h) with `curl -w
      '%{time_total}'` and the debug timing log. Record the numbers
- [ ] 4.2 Repeat the per-request timing on Node1 hardware against a branch container (Node1
      lock held, production containers untouched). Acceptance: typical request ≤ ~20 ms, and no
      visible tick delay in the tick-duration metric/logs during the sweep. If not met, switch D5
      to off-lock computation before continuing

## 5. UI: chart primitive + data hook

- [x] 5.1 Check the pinned Recharts version for chart-root `onDoubleClick` with an active label
      (design Open Question). Note the result in design.md or the component doc.
      Result: recharts 2.15.4's `generateCategoricalChart` forwards `onDoubleClick` with the
      same mouse state (`activeLabel`) as `onMouseMove`; no wrapper fallback needed.
- [x] 5.2 Test first (vitest): `TimeSeriesChart` calls `onCursorMove(tsMs)` on hover,
      `onCursorMove(null)` on leave, and `onCursorDoubleClick(tsMs)` on double-click. Charts not
      passing the props are unaffected
- [x] 5.3 Implement the generic `onCursorMove` / `onCursorDoubleClick` props on `TimeSeriesChart`
      (wrapper-`div` fallback for double-click if 5.1 says so)
- [x] 5.4 Add `start` to `CapacityCurvesResponse` in `api/types.ts`, an `api.capacityCurvesAt(startMs)`
      client method, and a `useCapacityCurvesAt(slotStartMs | null)` hook (key includes the slot,
      `staleTime`/`refetchInterval` 10 s, disabled when `null`)
- [x] 5.5 (Superseded under one-concept-one-function: the helper was a second copy of the
      server's snap and was removed; the UI sends the rested cursor time.) Add a shared helper that snaps a cursor ts to a remaining plan slot start using the
      `GET /flexibility/forecast` slot list, with unit tests (mid-slot, exact boundary, past,
      beyond last slot, empty list)

## 6. UI: Site Headroom "Move commitment start" mode (D7)

- [x] 6.1 Test first: `GridHeadroomCell` / `SiteHeadroomChart` tests. Default mode ignores the
      cursor. In move mode, hovering a future ts renders curves from the at-start response. Past
      hover falls back to now curves. Double-click pins and a second double-click unpins.
      Switching to "Values" unpins. Moving within one slot issues no new request (debounced)
- [x] 6.2 Implement the mode switch in the `GridHeadroomCell` header, debounced hover → snapped slot
      → `useCapacityCurvesAt`, pin state, tap-to-pin on touch
- [x] 6.3 Render the vertical reference line at the server-returned `start`, plus the caption
      ("Commitment start: HH:MM, plan slot" / "pinned" / "no active plan, anchored at now").
      Update `SiteHeadroomChart`'s doc comment (dashed curves start at `capacity.import.start`,
      not necessarily now)
- [x] 6.4 `npm test` and eslint (zero errors) in `VEN/ui`, then `npm run build`

## 7. End-to-end use-case coverage

- [x] 7.1 Add BDD scenarios to `tests/features/isolated/capacity_envelope_absolute_quantities.feature`.
      A future-start request snaps to a slot and returns that slot as `start`. Its import/export
      first steps equal the Site Headroom forecast's `down_kw`/`up_kw` at that slot, fetched
      back-to-back. A request without `start` is unchanged. Run it first to see it fail
- [x] 7.2 Implement the step definitions, reusing the existing capacity/headroom steps where
      possible

## 8. Verification and merge gates

- [x] 8.1 `wsl_lock` + free-memory check, then `wsl cargo fmt --check`,
      `cargo clippy --all-targets --all-features -- -D warnings -j 2`, `cargo test -p ven-app -j 2`
- [x] 8.2 Architecture invariants from `.claude/CLAUDE.md` (no `use crate::profile` in
      routes/controller, no `use crate::assets::` in milp_planner/entities) and
      `scripts/audit_file_sizes.py`
- [x] 8.3 `DOCKER_HOST=Node2 bash run_all_tests.sh` (rust, e2e, resilience) all green
- [ ] 8.4 Manual check in the VEN UI on the deployed branch: sweep, pin, unpin, no-plan
      fallback, touch tap

## 9. Documentation and cleanup (workflow rule 3)

- [x] 9.1 `docs/architecture/VEN_ARCHITECTURE.md` §3.0b/§3.0c: `resolve_plan_state_at`'s first
      production caller, the fourth `(t1, t2)` slice, the `start` parameter, slot snapping, and
      the lock/compute decision with its measured numbers
- [x] 9.2 `docs/use-cases/HEMS-USE-CASE-OBSERVATION-MANUAL.md` "View sustained-commitment capacity
      forecast": future-start curl example + the Controller "Move commitment start" mode,
      pointing at the BDD feature by name
- [x] 9.3 Append a `docs/history/project_journal.md` entry. Add any durable lesson (e.g. seam
      divergence found in 2.7) to `docs/reference/KEY_LEARNINGS.md`
- [ ] 9.4 After merge to main: delete `openspec/changes/movable-capacity-curve-start/` and clean
      up the Node2/Node1 branch checkouts/worktrees
