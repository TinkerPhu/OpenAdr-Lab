# Tasks

Every task is test-first: write the test, run it to see it fail, then implement. Part A and Part B are separate
hand-offs to the Node3 runner; B starts only after A is merged. Before touching an area, read the matching rows in
`docs/reference/TECHNICAL_DEBTS.md`. Before each hand-off run `python ci/audits/audit_ven_architecture.py`,
`audit_file_sizes.py`, `audit_duplication.py` and `audit_debt_gate.py`; run `audit_step_definitions.py` after editing
`tests/features/`. WSL cargo work takes `ci/locks/wsl_lock.sh` and a free-RAM check first, always with `-j 2`.

## Part A - cost beside the curve

### 1. Planned-cost function (one copy)

- [x] 1.1 Write the test for `planned_cost_eur(slots, asset_id, until)`: sums grid x import + surplus x export per slot, and a missing asset is `0.0` or `None` as decided in one place. Verify it fails, then `cargo test -p ven-app planned_cost` passes after 1.2.
- [x] 1.2 Move `solved_session_cost` out of `session_flexibility.rs`, add the `until` parameter, point `session_entry` at it. Verify `cargo test -p ven-app session_flexibility` stays green.

### 2. Asset-declared bands

- [x] 2.1 Write the test for `EvCharger::comfort_bands`: 50 kWh at 40 % SoC with limit 80 % and a flat 0.20 curve gives 20 kWh and EUR 4.00; a 0.40 to 0 ramp from 0 % at limit 100 % gives EUR 10.00. Verify it fails first.
- [x] 2.2 Add `Asset::comfort_bands(&self, rates) -> Option<ComfortBands>` (default `None`), implement it on `EvCharger` over `ev_energy_segments` with `w_ghg = 0` and `EvBandRange { init: current, max: limit }`; add a file header comment to any touched module that lacks one. Verify 2.1 passes and `audit_ven_architecture.py` is clean.
- [x] 2.3 Add `comfort_bands` to `SimReadPort`, its `SimHandle` implementation and the mock. Verify `cargo check --tests -j 2` in WSL.

### 3. Route and types

- [x] 3.1 Write the tests for `POST /assets/:id/comfort_curve/cost`: the pure `comfort_curve_cost` (maximum paired with the plan's spend; no bands gives `null`, no allocation gives `0.0`) in `entities/comfort.rs`, and the route's 404 / 422 / nothing-stored behaviour in the BDD scenario of 5.1 (no handler-level test harness exists for these routes). Verify the unit tests fail first.
- [x] 3.2 Implement the handler (extract the parts, not `AppCtx`), register the route, add the response type to `ui_types.rs`, regenerate TS types. Verify 3.1 passes and `VEN/ui/src/api/generated/` holds the new type.

### 4. UI surface (same piece of work)

- [x] 4.1 Write the `ComfortCurveCard.test.tsx` cases: an EV shows "Most this curve can spend" and "Planned (current plan)"; a heater shows neither; planned above max shows the forced-by-deadline note. Verify they fail first.
- [x] 4.2 Add `useComfortCurveCost(assetId, rows)` (debounced) to `api/hooks.ts` and `api/client.ts` and render it beside `CurveChart`. Verify `cd VEN/ui && npm test` and eslint report zero errors.

### 5. Use case, docs, hand-off (A)

- [x] 5.1 Add a `@use_case` scenario to `tests/features/ven_comfort_curve.feature` (steps in `steps/comfort_steps.py`, numeric arguments as `{x:g}`): a flat 0.20 curve on `ev` reports a maximum equal to 0.20 x the energy ahead, and an unsaved preview leaves `source` unchanged. Verify `python ci/audits/audit_step_definitions.py`.
- [x] 5.2 Wave Part A into `docs/architecture/ven_milp_planner.md` section 10 and `docs/use-cases/COMFORT-PERSONAS-USE-CASE-MANUAL.md`; add journal and KEY_LEARNINGS entries; rewrite GB-53 in `docs/BACKLOG.md` to the input half only. Verify the docs-style rule in `docs/reference/DOCUMENTATION_STYLE.md`, then hand off with the `ci-handoff` skill.

## Part B - cumulative-spend input (after A is merged)

### 6. Domain conversion

- [ ] 6.1 Write the `entities/comfort.rs` tests: 50 kWh at 20 % with "60 % for EUR 2.00, 80 % for EUR 2.50" gives 0.10 then 0.05 EUR/kWh; round-trip spend is within EUR 0.01 at each knot; a non-concave curve errors naming the point in spend terms; falling spend errors; a 3-point curve yields at most 8 planner bands. Verify they fail first.
- [ ] 6.2 Implement `rates_from_cumulative_spend` (pure; energy per fill passed in; result validated by `validate_curve`) and the inverse view as a running sum over the asset's bands, with no second integral. Verify 6.1 passes and `audit_duplication.py` reports nothing new.

### 7. Route and UI

- [ ] 7.1 Write and then pass the route tests for `POST /assets/:id/comfort_curve/spend`: 201 with the stored marginal curve and source `override`; non-concave 422; heater 422 "not available for that asset". Register the route and add the types. Verify `cargo test -p ven-app comfort`.
- [ ] 7.2 Add the read-back: the card shows an EV's stored curve as cumulative spend rows labelled "from X % (current charge)", a heater keeps the marginal table, saving spend rows calls the spend route, and a 422 message is displayed. Write the `ComfortCurveCard.test.tsx` cases first. Verify `npm test` and eslint.
- [ ] 7.3 Update the card's help text (marginal-versus-total sentence and the concavity rule). The marginal editor stays for non-EV assets; verify by the heater test from 7.2.

### 8. Use case, docs, hand-off (B)

- [ ] 8.1 Add a `@use_case` scenario: set the EV curve by cumulative spend, read back the stored marginal bids with `source: override`; a non-concave spend curve is rejected with 422. Verify `audit_step_definitions.py`.
- [ ] 8.2 Wave Part B into the docs, journal and KEY_LEARNINGS (the D4 and D5 reasoning), remove GB-53 from `docs/BACKLOG.md`, run `audit_debt_gate.py`, hand off. After the merge, delete this change folder and the branch and worktree.
