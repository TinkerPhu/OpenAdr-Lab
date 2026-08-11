## 1. BL-40 — context plumbing (test-first)

- [ ] 1.1 Write a unit test for `resolve_tick_context` (or a focused helper
      test alongside it) asserting that when `state.asset_heuristics()`
      contains an entry for `ids::ASSET_BASE_LOAD`, the returned
      `TickContext.base_load_heuristic_kw_now` equals
      `heuristic.sample_kw(now)` for the injected `now`; and that it is
      `None` when no such entry exists. Confirm it fails (field doesn't
      exist yet).
- [ ] 1.2 Add `base_load_heuristic_kw_now: Option<f64>` to `TickContext`
      (`VEN/src/tasks/sim_tick/context.rs`).
- [ ] 1.3 In `resolve_tick_context`, add the async read:
      `state.asset_heuristics().await.get(ids::ASSET_BASE_LOAD).map(|h| h.sample_kw(now))`,
      placed alongside the existing weather/measurement pre-lock reads.
- [ ] 1.4 Run the test from 1.1 and confirm it now passes.

## 2. BL-40 — 3-tier fallback in `SimState::tick` (test-first)

- [ ] 2.1 Write a unit test in `simulator/tests.rs` (or its own test module,
      matching existing file layout): `SimState::tick` called with
      `base_load_measured_kw = None`, a `base_load_heuristic_kw = Some(x)`
      argument, and a known `now` — assert the `base_load` asset entry's
      `last_power_kw` equals `x`, not the synthetic
      `baseline_kw_profile + appliance_noise_kw(now)` value. Confirm it
      fails to compile/pass (parameter doesn't exist yet).
- [ ] 2.2 Write a second unit test: same call but with
      `base_load_heuristic_kw = None` (cold start) — assert `last_power_kw`
      equals the synthetic spike-model formula, confirming the last-resort
      tier still applies unchanged.
- [ ] 2.3 Add `base_load_heuristic_kw: Option<f64>` as a new trailing
      parameter to `SimState::tick` (`VEN/src/simulator/mod.rs`).
- [ ] 2.4 Change the `BaseLoad` arm's fallback chain to
      `base_load_measured_kw.or(base_load_heuristic_kw).unwrap_or_else(...)`
      per design.md D2.
- [ ] 2.5 Update every existing call site of `SimState::tick` (all tests in
      `simulator/tests.rs`, `simulator/tests/peek_base_load_kw_tests.rs`,
      `tasks/sim_tick/tick_tests.rs`, and any other test module found via
      `grep -rn "\.tick(" VEN/src`) to pass the new trailing argument
      (`None` unless the test specifically exercises this tier).
- [ ] 2.6 Run tests 2.1 and 2.2 and confirm both pass; run the full
      `simulator` test module to confirm no regressions.

## 3. BL-40 — 3-tier fallback in `peek_base_load_kw` (test-first, parity)

- [ ] 3.1 Write a unit test in
      `simulator/tests/peek_base_load_kw_tests.rs`: call
      `peek_base_load_kw` with `base_load_measured_kw = None` and a new
      `base_load_heuristic_kw = Some(x)` argument — assert the returned
      value reflects the heuristic tier (same formula shape as `tick`'s D2
      chain, including the existing override/decay blend on top). Confirm
      it fails (parameter doesn't exist yet).
- [ ] 3.2 Add the matching parity test: call both `peek_base_load_kw` and
      `SimState::tick` with the same `now`, override state, and
      `base_load_heuristic_kw = Some(x)` (measurement absent/stale) —
      assert the preview matches the committed tick's `last_power_kw`,
      extending the existing
      `peek_base_load_kw_matches_tick_output_for_same_now`-style coverage
      to the new tier. Confirm it fails.
- [ ] 3.3 Add `base_load_heuristic_kw: Option<f64>` as a new trailing
      parameter to `peek_base_load_kw`
      (`VEN/src/simulator/base_load_preview.rs`) and apply the identical
      D2 fallback chain.
- [ ] 3.4 Update every existing call site of `peek_base_load_kw`
      (`tasks/sim_tick/tick.rs`'s live dispatch call, and all tests in
      `peek_base_load_kw_tests.rs`) to pass the new argument, sourcing it
      from `ctx.base_load_heuristic_kw_now` in `tick.rs`'s production call
      site.
- [ ] 3.5 Run tests 3.1 and 3.2 and confirm both pass.

## 4. BL-40 — wiring cleanup and full-suite verification

- [ ] 4.1 In `tasks/sim_tick/tick.rs`, pass `ctx.base_load_heuristic_kw_now`
      into both the `sim_guard.peek_base_load_kw(...)` call and the
      `sim_guard.tick(...)` call.
- [ ] 4.2 Run `grep -r "use crate::assets::" VEN/src/entities` and the
      other `ven-architecture` invariant greps from CLAUDE.md — confirm
      still empty (this change should not touch entities/controller
      profile imports).
- [ ] 4.3 Run `python scripts/audit_file_sizes.py` — confirm
      `simulator/mod.rs` (and `base_load_preview.rs`, `context.rs`) stay
      under the 500/200 production-line caps; if `simulator/mod.rs` crosses
      500, split the `BaseLoad` arm's fallback resolution into a small free
      function per design.md's Risks section before proceeding.
- [ ] 4.4 `wsl cargo fmt --check && wsl cargo clippy --all-targets
      --all-features -- -D warnings` (acquire `wsl_lock.sh` first per
      project convention) — confirm clean.
- [ ] 4.5 `wsl cargo test -p ven-app` (same lock) — confirm full green,
      including all updated call sites from groups 2–3.

## 5. BL-40 — documentation

- [ ] 5.1 Update `docs/architecture/real_measurement_mqtt.md`'s "Baseline
      load: simple replace" section to "Baseline load: 3-tier
      (measured > learned heuristic > synthetic)", matching the PV section's
      structure, and update the "Indirect path into the forecast" section's
      framing now that a dropout's fallback is heuristic-derived rather
      than purely synthetic — keep the existing "no measured/synthetic
      provenance tag" caveat, since this change does not resolve it.
- [ ] 5.2 Update `docs/BACKLOG.md`: mark BL-40 resolved (link to this
      change's commit/PR once merged), following this repo's existing
      resolved-entry convention (see recent examples like BL-34's
      resolution note).

## 6. R-60 scoping gate

- [ ] 6.1 Before starting implementation, re-read design.md's D5. Attempt a
      throwaway spike of `learn_asset_heuristics`'s "compare each tick
      against the *previous* run's profile" computation against the actual
      `HistoryPort`/job-scheduling code (`tasks/heuristics_job/mod.rs`) to
      confirm the previous run's `AssetHeuristics` is actually available at
      that point without new persistence.
- [ ] 6.2 Decision point: if 6.1 shows the previous-run profile is readily
      available (e.g. already passed into the job, or trivially re-fetched
      from `state.asset_heuristics()` before it's overwritten), proceed
      with tasks 7.x below in this same change. If it requires new
      persistence/plumbing beyond what design.md scoped, stop — do not
      implement R-60 here; instead add a new backlog/openspec entry noting
      R-60 needs its own change, and skip to section 8 (BL-40 is complete
      and shippable without R-60).

## 7. R-60 — error-feedback field (test-first, only if 6.2 proceeds)

- [ ] 7.1 Write a unit test: `learn_asset_heuristics` run twice in sequence
      (simulating two daily job runs) over history with a stationary
      pattern — assert the second run's `recent_mean_abs_error_kw` is low
      (near the synthetic model's own noise floor). Confirm it fails
      (field doesn't exist yet).
- [ ] 7.2 Write a second unit test: same two-run setup but with a step
      change in the underlying power model between the two windows —
      assert the second run's `recent_mean_abs_error_kw` is measurably
      higher than in the stationary case.
- [ ] 7.3 Write a regression test confirming `sample_kw`'s output is
      unchanged by the new field's presence (same inputs → same output as
      before this field existed).
- [ ] 7.4 Add `pub recent_mean_abs_error_kw: Option<f64>` to
      `AssetHeuristics` (`entities/design_vocabulary.rs`), defaulting to
      `None`.
- [ ] 7.5 Implement the recency-weighted mean-absolute-error computation in
      `learn_asset_heuristics` per design.md D5, using the previous run's
      profile confirmed available in task 6.1.
- [ ] 7.6 Run tests 7.1–7.3 and confirm all pass; run the full
      `services::heuristics` test module to confirm no regressions.
- [ ] 7.7 `wsl cargo fmt --check && wsl cargo clippy --all-targets
      --all-features -- -D warnings && wsl cargo test -p ven-app` (lock
      per project convention) — confirm clean.
- [ ] 7.8 Update `docs/reference/TECHNICAL_DEBTS.md`: mark R-60 resolved
      (or, if split per 6.2, leave it open with a note pointing at the new
      follow-up change).

## 8. Close-out

- [ ] 8.1 Confirm all four test suites are green per
      `docs/guidelines/TESTING.md` (UI unit N/A — no UI change; Rust
      unit+integration; E2E BDD only if this change's scope ends up
      touching an observable use case — expected not to, since this is
      internal fallback plumbing with no new user-facing surface;
      Resilience suite if it exercises measurement-dropout scenarios).
- [ ] 8.2 Once merged and verified, wave this change's content into
      `docs/architecture/real_measurement_mqtt.md` (already done in 5.1)
      and delete this `openspec/changes/base-load-dropout-fallback/`
      directory per the project's no-lingering-plans workflow — do not
      archive.
