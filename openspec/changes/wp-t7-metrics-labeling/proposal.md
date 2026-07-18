## Why

The Metrics page (`VEN/ui/src/pages/Metrics.tsx`) renders raw Prometheus metric
names verbatim (`poll_success_total{resource="events"}`) with no indication of
what they mean or how they relate to each other — a resident/operator has to
already know the naming scheme to make sense of the page. WP-T7 of
`docs/plans/ven-ui-transparency.md`.

## What Changes

- Group metrics under human-readable category headings and labels, falling back
  to the raw name for anything unrecognized (grouping degrades gracefully, never
  hides a metric).
- Add a "Raw view" toggle that reproduces the page's exact pre-change behavior
  (flat, ungrouped, raw names) for anyone who wants it.

## Capabilities

### New Capabilities
- `metrics-labeling`: the Metrics page groups known metrics under human-readable
  categories/labels by default, with a raw-view toggle that reproduces the
  page's pre-change flat/raw-name behavior; unrecognized metrics fall back to
  an "Other" group under their raw name rather than being hidden.

### Modified Capabilities
(none)

## Impact

- **VEN UI only**: `pages/Metrics.tsx` (grouping/labeling + toggle),
  `__tests__/Metrics.test.tsx` (new tests for the grouped/raw views; all
  pre-existing tests pass unmodified — the underlying per-metric table markup
  and testids are unchanged).
- **No backend change.**
- **Non-goals / scope correction**: the plan doc's original sketch named four
  categories — "VTN polling / reports / tasks / HTTP". Grep-confirmed
  (`counter!`/`histogram!`/`gauge!` call sites in `VEN/src`) that only two
  categories of custom metric actually exist today: `poll_success_total`/
  `poll_error_total` (VTN Polling) and `reports_sent_total` (Reports). There is
  no "tasks" or generic "HTTP" metric emitted anywhere — `PrometheusBuilder::new()`
  is installed with no HTTP-instrumentation middleware (e.g. `axum-prometheus`)
  and no per-task metrics are recorded (WP-T3's task status lives on `/tasks/status`,
  not in the Prometheus registry). The grouping map only covers what's real; a
  third "Other" bucket catches everything else (including any future metric) by
  raw name rather than inventing categories nothing populates yet.
