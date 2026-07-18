## Context

`MetricsPage.tsx` parses the raw Prometheus text response from `GET /metrics`
into `{name, labels, value}` rows and renders one table per metric name, using
the raw name as the table header. Grep across `VEN/src` confirmed only two real
custom-metric families exist: `poll_success_total`/`poll_error_total` (tagged by
a `resource` label: events/programs/reports) and `reports_sent_total`. This is a
small, contained UI presentation change — no new architecture, no backend
contract change — captured here mainly to record the scope correction from the
plan doc's speculative four-category sketch to the two categories that actually
exist.

## Goals / Non-Goals

**Goals:** human-readable grouping/labeling for the metrics this VEN actually
emits, with a raw-view escape hatch, without losing any existing behavior.

**Non-Goals:** no "Tasks" or "HTTP" metric category — nothing populates them
today (see proposal.md Impact). No backend change to what's recorded.

## Decisions

**D1 — A static name→{group, label} lookup table, not a naming-convention
parser.** With only 3 known metric names, a small explicit map
(`METRIC_META`) is simpler and more honest than inferring groups from name
prefixes (`poll_*` → could accidentally catch an unrelated future metric named
`poll_something_else` that isn't VTN-related). Anything not in the map falls
back to an "Other" group under its raw name — new metrics are never hidden,
just unlabeled until someone adds them to the map.

**D2 — Raw view reuses the exact same `MetricTable` component, not a
duplicate render path.** The toggle changes only *which* names are passed to
`MetricTable` and whether they're wrapped in group headings — the per-metric
table markup, testids, and label/value formatting are identical in both views,
which is why every pre-existing test in `Metrics.test.tsx` continues to pass
unmodified.

## Risks / Trade-offs

- **[Risk] The static map needs a manual update whenever a new metric is
  added.** → Mitigation: acceptable — the fallback ("Other" group, raw name)
  means a forgotten update degrades to today's exact behavior for that one
  metric, never hides it.

## Migration Plan

UI-only, additive; no migration.

## Open Questions

None.
