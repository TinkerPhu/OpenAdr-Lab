## 1. Pre-flight

- [x] 1.1 Grep all read sites of `AssetAllocation.cost_eur` in `VEN/src/` and `VEN/ui/src/`
      (decision matrix consumers, envelope aggregation, Planner tab components) and confirm none
      depend on the old (credit) sign convention in a way that would double-compensate after the
      flip.

## 2. EV allocation block

- [x] 2.1 Write a failing unit test in `VEN/src/controller/milp_planner/tests/` asserting a slot
      fully covered by PV surplus in the EV allocation block reports
      `cost_eur == surplus_power_kw * export_tariff_eur_kwh * dt_h` (positive).
- [x] 2.2 Confirm the test fails against current code (old `−` sign).
- [x] 2.3 Flip the sign in the EV block of `results.rs::translate_to_plan`.
- [x] 2.4 Confirm the new test passes; update any existing EV-block test that asserted the old
      sign, recomputing the expected value from the fixture's known `surplus_power_kw` /
      `export_tariff_eur_kwh` rather than just negating the old assertion.
      (No existing test asserted `AssetAllocation.cost_eur`'s sign — nothing to update.)

## 3. Heater allocation block

- [x] 3.1 Write a failing unit test for the heater block, same shape as 2.1.
- [x] 3.2 Confirm it fails against current code.
- [x] 3.3 Flip the sign in the heater block.
- [x] 3.4 Confirm the new test passes; update any existing heater-block test asserting the old
      sign (recompute, don't negate). (None existed.)

## 4. Shiftable-load allocation block

- [x] 4.1 Write a failing unit test for the shiftable-load block, same shape as 2.1.
- [x] 4.2 Confirm it fails against current code.
- [x] 4.3 Flip the sign in the shiftable-load block.
- [x] 4.4 Confirm the new test passes; update any existing shiftable-load-block test asserting
      the old sign (recompute, don't negate). (None existed.)

## 5. Battery-charging allocation block

- [x] 5.1 Write a failing unit test for the battery-charging block, same shape as 2.1.
- [x] 5.2 Confirm it fails against current code.
- [x] 5.3 Flip the sign in the battery-charging block.
- [x] 5.4 Confirm the new test passes; update any existing battery-charging-block test asserting
      the old sign (recompute, don't negate). (None existed.)

## 6. Cross-check and verification

- [x] 6.1 Add a test asserting the decision matrix's summed `cost_eur` and
      `FlexibilityEnvelope.estimated_cost_eur` (`solved_session_cost()`) agree in sign for a
      PV-surplus scenario spanning multiple asset types (EV, heater, shiftable-load,
      battery-charging).
- [x] 6.2 Run `wsl cargo test -j 2 -p ven-app` under `wsl_lock` (acquire lock first per
      `wsl-lock` rule) — all tests green. (855 passed, 0 failed.)
- [x] 6.3 Run `cargo fmt --check` and `cargo clippy --all-targets --all-features -- -D warnings`
      — clean.
- [x] 6.4 Run `scripts/audit_file_sizes.py` — `results.rs` still within the VEN/src/ 500
      production-line limit. (PASSED.)
- [x] 6.5 Update `docs/history/project_journal.md` with what changed, why, and any key learnings.
- [ ] 6.6 Remove the BL-40 entry (and its Implementation Task List section 1 checklist) from
      `docs/BACKLOG.md` once merged. (Pending: do after this branch merges to main.)
