# Tasks

Ordered so the shared derivation exists before either producer is moved onto it, and so the six
old derivations are deleted in the same step that replaces them — never left beside the new one.
`test-first` applies throughout: write the failing test, confirm it fails, then implement.

## 1. The shared derivation

- [ ] 1.1 Write failing unit tests in a new `VEN/src/assets/ev_trip_series.rs` for the three
      outputs of one `ExpectedVehicleUse` (window overlap in the in-progress slot, drop at the
      stated return, one obligation at the departure slot); confirm they fail to compile for want
      of the type
- [ ] 1.2 Define `ExpectedVehicleUse` and `ExpectedTripConsumption` per design Decision 1 and
      verify `cargo check -p ven-app` compiles with the tests still failing on behaviour
- [ ] 1.3 Implement availability as the union of windows with overlap semantics (design
      Decision 2, first half) and verify the in-progress-slot test passes and a slot wholly inside
      an away interval is false
- [ ] 1.4 Implement the trailing window after a known final return, and no tail when the final
      return is unknown (design Decision 2, second half); verify with two tests — a final use
      with consumption is chargeable to the horizon end, one without is not
- [ ] 1.5 Implement consumption placement at each use's own return with the slot-0 exclusion
      (design Decision 3) and verify a lone use's drop lands, which `trip_drops_between_sessions`
      cannot do today
- [ ] 1.6 Implement one obligation per `firm` use, carrying `session_id`, skipping departures
      beyond the horizon; verify against the `ev-trip-series-planning` scenario "A departure
      beyond the horizon binds nothing yet"
- [ ] 1.7 Run `python scripts/audit_file_sizes.py` and confirm the new file is inside the
      500-production-line cap

## 2. The EV's conversion, with the default removed

- [ ] 2.1 Change `EvCharger::expected_trip_drop` to take a required `distance_km: f64` returning
      `f64`, deleting `ExpectedTripDrop` and its `defaulted`/`distance_km` fields; verify the four
      existing `expected_trip_drop_*` tests are updated to the new signature and pass, and that
      the "falls back to the configured default" test is deleted rather than weakened
- [ ] 2.2 Remove `default_trip_distance_km` from `EvCharger`, `entities/asset_params.rs` and
      `profile/{schema,defaults,validate}.rs`; verify `cargo check -p ven-app` is clean and that a
      profile still declaring it is rejected with a clear error
- [ ] 2.3 Remove the `any_defaulted` plumbing and whatever plan-warning surface reported an
      assumed distance; verify no production reference to `defaulted` remains
      (`grep -rn "defaulted" VEN/src --include=*.rs`)

## 3. The session producer

- [ ] 3.1 Add the expected return time to `EvSession` beside `expected_trip_distance_km`, both
      `#[serde(default)]`; verify a persisted session from before this change still deserializes
      (round-trip test with the old JSON shape)
- [ ] 3.2 Write a failing test that a session mapping to `ExpectedVehicleUse` pairs the two fields
      or carries `None`, and that one-without-the-other is unconstructible; implement the mapping
      and verify
- [ ] 3.3 Add the `DomainError` variant for an incomplete trip estimate per
      `docs/guidelines/ERROR_HANDLING.md` (typed fields, terse Display) and reject the half-stated
      pair at the `/user-requests` boundary; verify against the two refusal scenarios in
      `ev-user-trip-estimate`
- [ ] 3.4 Replace `availability_from_sessions`, `obligations_from_sessions` and
      `trip_drops_between_sessions` in `ev_session_context.rs` with one call to the shared
      derivation, deleting all three; verify the full `cargo test -p ven-app` suite still passes
      and `grep` finds no caller of the deleted names
- [ ] 3.5 State each simulated trip's real generated consumption and return in
      `usage_sim_plan_ahead.rs` instead of `None`, and delete the comment claiming `soc_drop_pct`
      is "carried straight through"; verify a queued simulated session projects the generator's
      own per-trip drop, not a uniform one

## 4. The forecast producer

- [ ] 4.1 Write a failing test that an EV with two predicted trips in the horizon yields two
      obligations; confirm it fails against today's single-obligation code
- [ ] 4.2 Replace `target_next_predicted_departure` with the horizon walk of design Decision 4,
      building `ExpectedVehicleUse` per predicted trip and calling the shared derivation; verify
      4.1 passes and the existing `ev_milp.rs` forecast tests still pass
- [ ] 4.3 Make `engage_charge_planning: false` set `firm: false` on every use while keeping mask
      and consumption (design Decision 4, last paragraph); verify the existing
      "engage_charge_planning: false" test still asserts no charging is driven
- [ ] 4.4 Delete `ev_schedule::availability_per_slot`, `soc_drop_frac_per_slot` and the already-dead
      `most_recently_ended_trip`; verify `cargo clippy --all-targets --all-features -- -D warnings`
      reports no dead code and the suite passes
- [ ] 4.5 Confirm the shortfall warning names a predicted departure usefully when no `session_id`
      exists, per design Risks; adjust `ev_diagnostics::firm_shortfall` text if it reads as a bug

## 5. UI

- [ ] 5.1 Add the optional "Back by" input to the EV plan dialog beside "Trip after departure
      (km)", presented as one optional estimate; verify `cd VEN/ui && npm test` passes with a new
      test asserting both-or-neither is enforced before submit
- [ ] 5.2 Surface the stated return on a queued session's card block next to the distance; verify
      the `Devices.test.tsx` suite covers it
- [ ] 5.3 Update `VEN/ui/src/api/types.ts` and the four affected suites under
      `VEN/ui/src/__tests__/`; verify `npm test` and `npx eslint` are clean

## 6. Behaviour tests

- [ ] 6.1 Add to `tests/features/ev_usage_forecast.feature`: a VEN with two trips in 48 h charges
      before both, with the SoC curve rising, falling, rising and falling
- [ ] 6.2 Add to `tests/features/ven_user_request.feature`: a session with no estimate holds the
      plan flat across its departure; a session with a complete estimate shows the drop at the
      stated return; a half-stated estimate is refused
- [ ] 6.3 Add to `tests/features/ev_usage_simulation.feature`: queued simulated sessions project
      the generator's own per-trip consumption
- [ ] 6.4 Run `DOCKER_HOST=Node2 bash run_all_tests.sh --e2e` and verify green; read the log's
      scenario counts rather than the exit code, which reports the launcher, not the suite

## 7. Documentation, rewritten

- [ ] 7.1 Rewrite the session→MILP field table and the "EV usage simulation" / "EV usage forecast"
      sections of `docs/architecture/VEN_ARCHITECTURE.md` from the corrected model, with no
      bridging sentence; verify by grepping the file for "the solver can plan the recharge" and
      finding nothing
- [ ] 7.2 Rewrite UC-17, UC-18 and the usage-class observation guide in
      `docs/use-cases/HEMS-USE-CASE-OBSERVATION-MANUAL.md`, including what a plan with no trip
      estimate looks like; verify each referenced `.feature` file exists
- [ ] 7.3 Rewrite the R-92/R-93 resolution note in `docs/reference/TECHNICAL_DEBTS.md` so it no
      longer reads as though both producers were widened, and add entries for the
      `usage_sim`/`usage_forecast` naming inversion and ven-2's horizon-edge knife edge
- [ ] 7.4 Rewrite the 048/049 entries in `docs/history/project_journal.md` to the corrected model
      and add this change's entry (what was wrong, why it survived review, what it cost to find)
- [ ] 7.5 Add the KEY_LEARNINGS entry for the general shape: a producer that supplies two of three
      planner inputs looks correct in every unit test, because the missing third is a motivation,
      not a value — nothing is wrong, only absent

## 8. Verification and rollout

- [ ] 8.1 Re-run the 048 solve benchmark with one, two and three obligations and record timings
      in the journal with the `GapLimit` caveat; verify no solve regresses beyond the recorded
      envelope
- [ ] 8.2 Run the full pre-merge gate: `cargo fmt --check`, `cargo clippy --all-targets
      --all-features -- -D warnings`, `cargo audit`, `python scripts/audit_file_sizes.py`,
      `python scripts/audit_ven_architecture.py`, `python scripts/audit_debt_gate.py`
- [ ] 8.3 Deploy ven-12 alone and verify its live 48 h EV timeline rises, falls at the first
      return, rises again, and falls at the second — the recorded pre-change shape was
      `0.800 → 0.698 → 0.570` flat between
- [ ] 8.4 Roll out to the remaining EV VENs and spot-check three for charging in inter-trip gaps
- [ ] 8.5 Wave the two capability specs into `docs/` and delete
      `openspec/changes/ev-multi-trip-planning/` per workflow rule 3
