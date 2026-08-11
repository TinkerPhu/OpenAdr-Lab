## 1. BASELINE report builder (backend, test-first)

- [x] 1.1 Write failing unit tests in `VEN/src/controller/reporter.rs` for a new
      `build_baseline_report_intervals` (or similarly named) function: given an obligation's
      requested interval grid and an `AssetHeuristics` map, returns one interval per
      requested slot with a `BASELINE` payload equal to the summed `sample_kw()` across all
      relevant assets (mirroring how `build_net_site_power_ts` sums assets for measurement
      reports) — confirm red (function doesn't exist).
- [x] 1.2 Implement `build_baseline_report_intervals`, confirm the tests from 1.1 go green.
- [x] 1.3 Add a `"BASELINE"` arm to `build_measurement_report_for_obligation`'s
      `payload_type` match, calling the new builder — new test: obligation with
      `payload_type: "BASELINE"` produces a report whose values match the heuristic, not
      `net_power_ts` (regression-shape test: construct a case where heuristic and measured
      power clearly differ, assert the report used the heuristic one).
- [x] 1.4 Add the quality-tag payload entry (`ForecastSource` variant name, e.g.
      `"HEURISTIC"`) to each BASELINE interval — new test asserting the tag is present with
      the expected value. Scope: BASELINE payloads only for this change (per design.md's
      resolved open question — not `USAGE_FORECAST`/capacity-reservation payloads, those are
      an easy follow-up once this pattern is proven).
- [x] 1.5 `cargo fmt`/`clippy --all-targets --all-features -D warnings`/full VEN Rust suite
      green (via `wsl cargo test -p ven-app`, respecting `wsl_lock.sh` and the memory-budget
      rule).

## 2. Experiment KPI evaluation

- [x] 2.1 `experiments/kpi.py`: add `event_impact_kwh` computation — for each event window,
      sum `(baseline_kw − actual_kw) × interval_hours` from the recorder's archived
      `BASELINE` and `USAGE` rows (`report_type` column) for that event/window; `None` when
      no BASELINE rows exist for the window (per spec's "no BASELINE archived" scenario).
- [x] 2.2 Unit-test the new `kpi.py` logic (the file has no test harness today — add a small
      one, or a `__main__` self-check block matching the style of `scripts/personas.py`'s
      self-check, whichever fits the file better) covering both spec scenarios: BASELINE
      above actual, and no BASELINE archived.

## 3. BDD scenario

- [x] 3.1 Write a failing BDD scenario in `tests/features/` (new or existing report-related
      `.feature` file — check for one before creating a new file) exercising: an active event
      + a report obligation requesting `payloadType: "BASELINE"` → the recorder archives a
      BASELINE report whose value differs from the concurrently-archived USAGE report during
      the event window. Confirm red against current `main` (no BASELINE support), then green
      after 1.1–1.4 land.

## 4. Verification & bookkeeping

- [ ] 4.1 Full test suite green: VEN Rust (`wsl cargo test -p ven-app`), VEN UI (unaffected —
      confirm no incidental breakage), BDD (`bash run_all_tests.sh --e2e`, Node1 lock
      required), file-size audit (`scripts/audit_file_sizes.py`).
- [ ] 4.2 Exit demonstration: re-run one experiment scenario (e.g. `s3_capacity_limit`, which
      already has an active `IMPORT_CAPACITY_LIMIT` event — see `docs/plans/roadmap/phase-5-forecast-and-baseline.md`'s
      original exit criterion) with a BASELINE report obligation configured, confirm
      `kpi.py` produces a non-null `event_impact_kwh` for that run.
- [ ] 4.3 Update `docs/architecture/VEN_ARCHITECTURE.md` (report-payload types section) and
      `docs/reference/DOCUMENTATION_STYLE.md`-compliant wiki page (`wiki/components/` —
      extend the existing forecasting/heuristics page rather than creating a new one, unless
      none fits) for the new BASELINE report type and its quality tag.
- [ ] 4.4 Append a `docs/history/project_journal.md` entry (what shipped, the design
      decisions from design.md, verification results) and record any new debt discovered
      along the way in `docs/reference/TECHNICAL_DEBTS.md` (e.g. if the real
      sample-count/variance confidence model from Non-Goals is worth tracking as a future
      item, file it in `docs/BACKLOG.md` instead — don't build it here).
- [ ] 4.5 Delete `docs/plans/roadmap/phase-5-forecast-and-baseline.md` (WP5.4 was its last
      open item) once 4.1–4.4 are done and tested, per the project's plan-lifecycle rule —
      its still-relevant substance already folded into 4.3's docs updates.
- [ ] 4.6 Delete this openspec change directory (`openspec/changes/wp5-4-baseline-reports/`)
      once merged and verified, per the same rule.
