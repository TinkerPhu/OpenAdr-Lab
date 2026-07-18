import { useMemo, useState } from "react";
import {
  FormControlLabel, Paper, Stack, Switch, Table, TableBody, TableCell,
  TableContainer, TableHead, TableRow, Typography,
} from "@mui/material";
import { useMetrics } from "../api/hooks";

interface MetricRow {
  name: string;
  labels: Record<string, string>;
  value: number;
}

function parsePrometheusText(text: string): MetricRow[] {
  const rows: MetricRow[] = [];
  for (const line of text.split("\n")) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith("#")) continue;
    const braceIdx = trimmed.indexOf("{");
    if (braceIdx === -1) {
      const parts = trimmed.split(/\s+/);
      if (parts.length >= 2) {
        rows.push({ name: parts[0], labels: {}, value: parseFloat(parts[1]) });
      }
      continue;
    }
    const name = trimmed.slice(0, braceIdx);
    const closeBrace = trimmed.indexOf("}");
    const labelsStr = trimmed.slice(braceIdx + 1, closeBrace);
    const labels: Record<string, string> = {};
    for (const pair of labelsStr.match(/(\w+)="([^"]*)"/g) ?? []) {
      const eq = pair.indexOf("=");
      labels[pair.slice(0, eq)] = pair.slice(eq + 2, -1);
    }
    const value = parseFloat(trimmed.slice(closeBrace + 1).trim());
    rows.push({ name, labels, value });
  }
  return rows;
}

function formatLabels(labels: Record<string, string>): string {
  const entries = Object.entries(labels);
  if (entries.length === 0) return "";
  return entries.map(([k, v]) => `${k}="${v}"`).join(", ");
}

// WP-T7 (docs/plans/ven-ui-transparency.md): human labels/grouping for the
// metrics this VEN actually emits (grep-confirmed — VEN/src only calls
// `counter!` for these three names; there is no generic "tasks" or "HTTP"
// category emitted today, despite the plan doc's original sketch naming
// them speculatively). Any metric not listed here (including anything a
// future WP adds) falls back to the "Other" group under its raw name —
// grouping degrades gracefully instead of hiding unrecognized metrics.
const METRIC_META: Record<string, { group: string; label: string }> = {
  poll_success_total: { group: "VTN Polling", label: "Poll successes" },
  poll_error_total: { group: "VTN Polling", label: "Poll failures" },
  reports_sent_total: { group: "Reports", label: "Reports sent" },
};

const GROUP_ORDER = ["VTN Polling", "Reports", "Other"];

function metaFor(name: string): { group: string; label: string } {
  return METRIC_META[name] ?? { group: "Other", label: name };
}

function MetricTable({
  name,
  label,
  rows,
}: {
  name: string;
  label: string;
  rows: MetricRow[];
}) {
  return (
    <TableContainer component={Paper}>
      <Table size="small" data-testid={`metrics-table-${name}`}>
        <TableHead>
          <TableRow>
            <TableCell colSpan={3}>
              <Typography variant="subtitle2">{label}</Typography>
              {label !== name && (
                <Typography
                  variant="caption"
                  color="text.secondary"
                  sx={{ fontFamily: "monospace" }}
                  data-testid={`metrics-raw-name-${name}`}
                >
                  {name}
                </Typography>
              )}
            </TableCell>
          </TableRow>
          <TableRow>
            <TableCell>Labels</TableCell>
            <TableCell align="right">Value</TableCell>
          </TableRow>
        </TableHead>
        <TableBody>
          {rows.map((row, i) => (
            <TableRow key={i}>
              <TableCell sx={{ fontFamily: "monospace", fontSize: "0.85rem" }}>
                {formatLabels(row.labels) || "—"}
              </TableCell>
              <TableCell align="right" sx={{ fontFamily: "monospace" }}>
                {Number.isNaN(row.value) ? "NaN" : row.value}
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </TableContainer>
  );
}

export function MetricsPage() {
  const { data: raw = "", dataUpdatedAt } = useMetrics();
  const [rawView, setRawView] = useState(false);

  const rows = useMemo(() => parsePrometheusText(raw), [raw]);
  const lastUpdated = dataUpdatedAt ? new Date(dataUpdatedAt).toLocaleString() : "—";

  const byName = useMemo(() => {
    const map = new Map<string, MetricRow[]>();
    for (const row of rows) {
      const list = map.get(row.name) ?? [];
      list.push(row);
      map.set(row.name, list);
    }
    return map;
  }, [rows]);

  const grouped = useMemo(() => {
    const byGroup = new Map<string, string[]>();
    for (const name of byName.keys()) {
      const { group } = metaFor(name);
      const names = byGroup.get(group) ?? [];
      names.push(name);
      byGroup.set(group, names);
    }
    return GROUP_ORDER.filter((g) => byGroup.has(g)).map((g) => ({
      group: g,
      names: (byGroup.get(g) ?? []).sort(),
    }));
  }, [byName]);

  return (
    <Stack spacing={2}>
      <Stack direction="row" alignItems="center" justifyContent="space-between">
        <div>
          <Typography variant="h5" data-testid="metrics-heading">
            Metrics
          </Typography>
          <Typography variant="body2" color="text.secondary" data-testid="metrics-last-updated">
            Last updated: {lastUpdated} (auto-refresh 10s)
          </Typography>
        </div>
        <FormControlLabel
          control={
            <Switch
              data-testid="metrics-raw-toggle"
              checked={rawView}
              onChange={(e) => setRawView(e.target.checked)}
            />
          }
          label="Raw view"
        />
      </Stack>

      {byName.size === 0 && (
        <Paper sx={{ p: 2 }}>
          <Typography color="text.secondary" data-testid="metrics-empty">
            No metrics available
          </Typography>
        </Paper>
      )}

      {byName.size > 0 && rawView && (
        <Stack spacing={2} data-testid="metrics-raw-view">
          {Array.from(byName.keys())
            .sort()
            .map((name) => (
              <MetricTable key={name} name={name} label={name} rows={byName.get(name) ?? []} />
            ))}
        </Stack>
      )}

      {byName.size > 0 && !rawView && (
        <Stack spacing={3} data-testid="metrics-grouped-view">
          {grouped.map(({ group, names }) => (
            <Stack key={group} spacing={1}>
              <Typography variant="h6" data-testid={`metrics-group-${group}`}>
                {group}
              </Typography>
              <Stack spacing={2}>
                {names.map((name) => (
                  <MetricTable
                    key={name}
                    name={name}
                    label={metaFor(name).label}
                    rows={byName.get(name) ?? []}
                  />
                ))}
              </Stack>
            </Stack>
          ))}
        </Stack>
      )}
    </Stack>
  );
}
