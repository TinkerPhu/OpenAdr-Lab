## 1. Metric grouping/labeling map

- [x] 1.1 Grep-confirmed the real metric names emitted by `VEN/src` (`counter!`/
      `histogram!`/`gauge!` call sites) before designing the grouping — found
      only `poll_success_total`, `poll_error_total`, `reports_sent_total`, not
      the plan doc's speculative four categories (see design.md D-context /
      proposal.md Non-goals).
- [x] 1.2 Added a static `METRIC_META` lookup (`{group, label}` per known name)
      to `VEN/ui/src/pages/Metrics.tsx`, with an "Other" fallback for anything
      unrecognized (never hides a metric).

## 2. Grouped view + raw-view toggle

- [x] 2.1 Extracted the existing per-metric table rendering into a
      `MetricTable` component, unchanged in markup/testids, reused by both
      views (design.md D2) — this is why every pre-existing test in
      `Metrics.test.tsx` passes unmodified.
- [x] 2.2 Added a grouped view (default): category headings
      (`metrics-group-<name>`), each listing its metrics' `MetricTable`s with
      human labels; a small monospace raw-name caption stays visible under any
      label that differs from the raw name, so the mapping is always
      discoverable, not hidden behind the label.
- [x] 2.3 Added a "Raw view" `Switch` toggle (`metrics-raw-toggle`) reproducing
      the exact pre-change flat/ungrouped/raw-name rendering.

## 3. UI suite gate

- [x] 3.1 `npx tsc --noEmit` clean.
- [x] 3.2 ESLint zero errors.
- [x] 3.3 `cd VEN/ui && npm test` — all pre-existing `Metrics.test.tsx` tests
      pass unmodified; added new tests: grouped-view category/label assertions,
      raw-view-toggle behavior, and the "Other" fallback for an unrecognized
      metric.

## 4. Bookkeeping

- [x] 4.1 Mark WP-T7 as done in `docs/plans/ven-ui-transparency.md` §4/§7.
- [x] 4.2 Note in `docs/history/project_journal.md`: the four-category →
      two-category scope correction and why (grep-confirmed, not assumed).
