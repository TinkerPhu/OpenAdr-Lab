# Tasks

## 0. Base

- [x] 0.1 Confirm `049-ev-session-queue-foundation` is merged to main, or branch from it. Verify `git log` of the working branch contains 049's head commit before touching `ev_session_context.rs`.

## 1. Pre-refactor

- [x] 1.1 R-73 (dead `future_state_values*` duplicates) is already resolved on main. Verify grep finds no `future_state_values` method in `VEN/src/assets/`.
- [ ] 1.2 Add `docs/reference/TECHNICAL_DEBTS.md` entries for the History-vs-timeline `curtailment_source` vocabulary mismatch (string vs numeric code) and for non-deadline sessions keeping the EV chargeable past their stated departure. Verify both entries are present.

## 2. Predicted presence (backend)

- [ ] 2.1 Test first, in `asset_port.rs` tests: `ev_future_state_at(0.5, false)` returns `{"soc": 0.5, "plugged": 0.0}` and `(…, true)` returns `plugged` 1.0. Run, see it fail, then extend the signature.
- [ ] 2.2 Test first, in `controller/milp_planner/tests/planner.rs`: a plan for a plugged EV with a deadline session departing mid-horizon has `planned_state_by_asset["ev"]["plugged"]` = 1.0 on the slots before the departure and 0.0 after it; an unplugged EV gives 0.0 on every slot; a plugged EV without a session gives 1.0 on every slot. Then pass `inputs.a_ev` from `results.rs` into `fill_planned_state`. Verify the tests pass and all existing planner tests pass unchanged.
- [ ] 2.3 Add a test to `controller/timeline.rs`: future EV points carry `values.plugged` from the slot's planned state, and a plan slot without the key yields a point without it (old-snapshot case). Verify it passes.
- [ ] 2.4 Add a `resample_to_grid` test: a bucket that is half plugged and half unplugged by time gives `plugged` 0.5 (±0.01). Verify it passes. No code change is expected; this pins existing behaviour.

## 3. Persisted history (backend)

- [ ] 3.1 Test first, in the `services/history_sampling.rs` tests: an EV window with `plugged` samples 1,1,0,0 gives `TickSample.plugged == Some(0.5)`, and a PV window gives `None`. Add `plugged: Option<f64>` to `entities/history.rs::TickSample` (`#[serde(default)]`) and the sum/count to the accumulator. Verify the tests pass.
- [ ] 3.2 Add schema v12 (`ALTER TABLE tick_samples ADD COLUMN plugged REAL;`), bump `SCHEMA_VERSION`, and extend the insert/select in `history_store/mod.rs` and `MockHistoryPort`. Extend `test_append_tick_samples_roundtrip` to round-trip `plugged`, and add a migration test where a v11 row reads back `plugged: None`. Verify with `cargo test -p ven-app history`.
- [ ] 3.3 Confirm `/history/ticks` serializes `plugged` on every row with a route-level test. Fix every `TickSample { … }` construction site (heuristics.rs, history_sampler, test support) so `cargo check --tests` is clean.

## 4. UI: generic state shading and per-asset declarations

- [ ] 4.1 Test first, a unit test for the pure run builder in `ui-charts` (plain numbers, no rendering, like `dayNightBands`): runs split on kind or weight change and on a data gap, alpha scales with weight, no run for a `null` classification, the final run extends one step. Implement `ui-charts/src/StateShading.tsx` with `fillOpacity={1}` on its areas. Verify with `npm test`.
- [ ] 4.2 Give `AssetTimelineChart` the `shadings` prop (background specs to `backgroundAreas`, overlay specs to `extraReferenceAreas`). Move `classifyPvPoint`/`CURTAILMENT_COLORS` into a PV declaration and delete `buildCurtailmentZones` and the `pvCurtailment` prop. Update the existing "PV curtailment shading" tests in `AssetTimelineChart.test.tsx` only in how they pass the declaration; their expected areas and colours stay unchanged. Verify with `npm test`.
- [ ] 4.3 Add a per-asset chart declaration table (`stateKey`, `shadings`) in `components/controller/types.ts` with the EV declaration (kinds `unplugged` / `predicted_away`, weight `1 − plugged`, EV blue, predicted with lower alpha and dashed outline). Make `AssetMidSection.tsx` and `History.tsx` look it up instead of branching on `assetId` or inferring from data. Add tests: unplugged past and predicted-away future give two differently styled background areas; `plugged` 1 and a missing key give none; `plugged` 0.5 gives half the alpha. Verify with `npm test`.
- [ ] 4.4 Add `plugged: number | null` to `HistoryTickSample` in `api/types.ts`, and pass it through in `ticksByAsset` (`History.tsx`). Add a test that an EV history row with `plugged` 0 produces a band and a `null` row produces none. Verify with `npm test` and zero eslint errors.
- [ ] 4.5 Visual check on a running VEN UI (Control and History, EV cell): the band is readable behind the EV's blue lines and distinct from the plan-zone shading on the future side. Switch to navy if it isn't. Verify with a screenshot of both pages.

## 5. Use case, BDD, docs

- [ ] 5.1 BDD in `tests/features/ven_timeline.feature`: "Future EV timeline points carry predicted presence" (future point with `soc` and `plugged`), and "Unplugging the EV shows in the timeline" (inject `ev_plugged=false` → poll until the EV now-point has `plugged` = 0, then clear the inject). Add any missing steps. Verify with `DOCKER_HOST=Node2 bash run_all_tests.sh --e2e` (node2-lock).
- [ ] 5.2 BDD in `tests/features/ven_history.feature`: after a sampling window, `/history/ticks?asset_id=ev` rows carry a numeric `plugged`. Add a UI scenario (controller steps, Playwright): with the EV unplugged, the EV cell chart shows the unplugged band, selected by a `className` set on its `ReferenceArea`. Verify in the same E2E run.
- [ ] 5.3 Update `docs/use-cases/HEMS-USE-CASE-OBSERVATION-MANUAL.md` UC-01 "What to observe": the "Unplugged" and "Predicted away" bands, no band means plugged or no data, and without a usage forecast the future follows the current plug state. Describe the state-shading helper and the `plugged` key in `docs/architecture/chart_diagrams.md` and `VEN_ARCHITECTURE.md` (timeline/history sections). Replace stale `openspec/changes/pv-curtailment-history/` references in code comments with the doc location. Verify that grep finds no remaining reference to that path.
- [ ] 5.4 Add a `docs/history/project_journal.md` entry, update `docs/BACKLOG.md`, and check `scripts/audit_file_sizes.py`, `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and the UI unit suites (VEN/ui and VTN/ui, since `ui-charts` is shared).
- [ ] 5.5 Run `/wiki-sync`, then delete this change directory (workflow rule 3) and the empty `openspec/changes/pv-curtailment-history/` leftover. Verify `openspec list` no longer shows the change.
