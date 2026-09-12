## 1. Failing tests first (unit, `VEN/src/controller/openadr_interface.rs` test module)

- [x] 1.1 `parse_rate_snapshots_intra_hour_high_priority_splits_hour` — priority-5 hourly 0.09 + priority-1 10-min 0.45 inside it → three segments 0.09 / 0.45 / 0.09 with exact boundaries (spec: intra-hour scenario, no-leak scenario)
- [x] 1.2 `parse_rate_snapshots_earlier_start_lower_priority_loses` — priority-5 05:00–06:00 vs priority-1 05:29–05:59 → every instant 05:29–05:59 resolves to priority-1
- [x] 1.3 `parse_rate_snapshots_partial_overlap_equal_priority_newer_wins` and `..._absent_priority_ranks_lowest_on_partial_overlap`
- [x] 1.4 `parse_rate_snapshots_resolves_each_payload_type_independently` — PRICE-only p1 over PRICE+GHG p5
- [x] 1.5 `parse_rate_snapshots_output_never_overlaps` — mixed looping hourly event + short events: assert sorted, pairwise non-overlapping, and union of segments equals union of inputs
- [x] 1.6 `parse_capacity_schedule_short_high_priority_limit_splits_longer_one` (spec: capacity scenario)
- [x] 1.7 Consumer agreement test: build `TariffTimeSeries::from_snapshots` from the resolved output of 1.1 and assert its value at 05:25 / 05:35 equals the tick-time `.find` lookup at the same instants
- [x] 1.8 Run the new tests under `wsl_lock` (`-j 2`, free-RAM check) and confirm they fail for the right reason; confirm every existing `parse_rate_snapshots_*` / `parse_capacity_schedule_*` / BL-02 priority test still passes unchanged

## 2. Implementation (`VEN/src/controller/rate_schedule.rs`)

- [x] 2.1 Split `collect_interval_groups` into (a) candidate collection — existing looping expansion and event-level interval fallback unchanged, each candidate carrying the event's BL-02 rank — and (b) a `resolve_segments` step
- [x] 2.2 Implement `resolve_segments`: sorted union of boundaries; per segment and per requested payload type pick the highest-ranked covering candidate carrying that type (D2–D5); skip uncovered segments
- [x] 2.3 Reuse the existing BL-02 comparator for rank, extended only with input-order as the final tie-break; update the module/`CONFLICT NOTE` comments to describe segment resolution
- [x] 2.4 Tests from group 1 green; `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `python scripts/audit_file_sizes.py` (rate_schedule.rs stays ≤ 500 production lines), architecture invariant greps

## 3. Use-case BDD (`tests/features/ven_rate_system.feature`, `steps/rate_steps.py`)

- [x] 3.1 Scenario "An intra-hour high-priority PRICE event overrides the day-ahead price for exactly its window": create a priority-5 1-hour PRICE event and a priority-1 10-minute PRICE event starting 20 min into that hour; wait for `/tariffs`; assert the three resolved segments and that no two `/tariffs` snapshots overlap
- [ ] 3.2 Run the scenario against the pre-fix VEN image first if cheap (expected fail), then green on the fixed build via `DOCKER_HOST=Node2 bash run_all_tests.sh --e2e` (lock held by the script)

## 4. Full verification

- [x] 4.1 `wsl cargo test -p ven-app -j 2` full suite green
- [ ] 4.2 E2E + resilience on Node2 green (`DOCKER_HOST=Node2 bash run_all_tests.sh --e2e` and `--resilience`)

## 5. Docs, merge, deploy

- [x] 5.1 `docs/architecture/VEN_ARCHITECTURE.md`: replace the BL-02 "identical-interval priority merge" description with segment resolution + the single-resolved-schedule guarantee (point to the tests by name)
- [x] 5.2 `docs/use-cases/`: add the overlapping day-ahead + intra-hour price behavior to the rate/tariff use case
- [ ] 5.3 `docs/BACKLOG.md`: close GB-45 (keep the experiment-hygiene half referenced to GB-46); `docs/history/project_journal.md` entry; correct `docs/history/fleet_run_journal.md`'s S-1/S-2 rows (planner saw the scenario price; recorded tariff/cost did not)
- [ ] 5.4 Delete this openspec change; rebase on `origin/main`, fast-forward merge, push
- [ ] 5.5 Deploy: rebuild VEN images on Node1 (ven-1..3, restart `ui`) and Node2 (`VEN/scale_out/node2`), under the host locks; verify health; remove the worktree and branch
