# Tasks: Deduplicate Live-Tick Physics

## 1. PV irradiance

- [ ] 1.1 Replace `SimState::tick()`'s inline irradiance formula
      (`VEN/src/simulator/mod.rs:208-213`) with a call to
      `entities::solar::natural_irradiance_at`. Run existing tick tests to
      confirm identical output (the formulas are already meant to be the same
      curve — this should be a no-op behaviorally).
- [ ] 1.2 Extract `tick()`'s irradiance-override/smoothing logic (the
      `pv_smoothing.update(...)` call and surrounding logic, currently
      duplicated by hand in `pv_preview.rs::peek_pv_kw`) into a function both
      call. **Constraint:** `tick()` *mutates* (`PvSmoothingState::update`
      writes back the new offset) while `peek_pv_kw` must stay read-only — so
      the shared piece has to be a **pure** function (inputs → decayed offset /
      resulting irradiance), with `tick()` persisting the result itself. A
      literal lift-and-share of the mutating call would break the preview's
      read-only contract.
- [ ] 1.2b Note that `peek_pv_kw` duplicates *two* things, not one: `tick()`'s
      offset decay (1.2) **and** `PvInverter::step_inner`'s
      forced/measured/weather precedence plus the `generation_limit_kw` clamp.
      `tick()` itself never computes PV power — it assigns `pv.irradiance` etc.
      and lets `cfg.step()` do it. Deduplicate the precedence half against
      `PvInverter` (the owner of that logic), not against `tick()`.
- [ ] 1.3 Confirm `peek_pv_kw_matches_tick_output_for_same_now` still passes
      — it should now be testing genuinely shared code, not two independent
      implementations that happen to agree.
- [ ] 1.4 While in this file: fix `pv_preview.rs`'s own doc comment (line 19),
      which cites this guard test as living "in `simulator/tests.rs`" — it
      actually lives in `VEN/src/simulator/tests/peek_pv_kw_tests.rs` (a stale
      cross-reference, found during this change's drafting;
      `base_load_preview.rs`'s equivalent comment already cites its own test
      file correctly).

## 2. Base load

- [ ] 2.1 Extract `tick()`'s base-load override/EMA-decay logic
      (`VEN/src/simulator/mod.rs`, the `AssetConfig::BaseLoad` match arm) into
      a function both `tick()` and `base_load_preview.rs::peek_base_load_kw`
      call, removing the hand-copied duplicate in the preview file. Same
      mutate-vs-read-only constraint as 1.2: `tick()` writes
      `self.base_load_smoothing.load_offset_kw`, the preview must not — extract
      a pure offset/`natural_base_kw` computation and let `tick()` persist.
- [ ] 2.2 Confirm `peek_base_load_kw_matches_tick_output_for_same_now` still
      passes for the same reason as 1.3.

## 3. Verify

- [ ] 3.1 `wsl cargo test -p ven-app` (acquire `wsl_lock.sh` first).
- [ ] 3.2 `cargo fmt --check` and
      `cargo clippy --all-targets --all-features -- -D warnings`.
- [ ] 3.2b `python scripts/audit_file_sizes.py` — `simulator/mod.rs` gains the
      `BaseLoadSmoothingState` impl block, so confirm it stays under the
      500-production-line cap (this is a CI check; it was missing from this
      list's original draft).
- [ ] 3.3 Confirm this lands **before** starting
      `openspec/changes/asset-dispatch-trait-objects/`'s tasks.md 4.4 (PV) and
      4.5 (BaseLoad) — check that change's status before beginning this one's
      section 1/2, and coordinate if it's already in progress.
- [ ] 3.4 Remove the R-70 row from `docs/reference/TECHNICAL_DEBTS.md` (the
      debt this change discharges). Do **not** decrement that file's "Next ID"
      line — per its own rule, resolved IDs are never reused.
- [ ] 3.5 Add a `docs/history/project_journal.md` entry (workflow rule 1):
      what was consolidated, why (three-way physics fork found in the 2026-09-03
      audit), and the mutate-vs-read-only constraint that shaped the
      extraction — that constraint is the non-obvious part worth recording.
- [ ] 3.6 Once merged: delete `openspec/changes/tick-physics-deduplication/`
      per this repo's workflow rule.
