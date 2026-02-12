import { useMemo } from "react";
import {
  Paper, Stack, Table, TableBody, TableCell, TableContainer,
  TableHead, TableRow, Typography,
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

export function MetricsPage() {
  const { data: raw = "", dataUpdatedAt } = useMetrics();

  const rows = useMemo(() => parsePrometheusText(raw), [raw]);
  const lastUpdated = dataUpdatedAt ? new Date(dataUpdatedAt).toLocaleString() : "—";

  const groups = useMemo(() => {
    const map = new Map<string, MetricRow[]>();
    for (const row of rows) {
      const list = map.get(row.name) ?? [];
      list.push(row);
      map.set(row.name, list);
    }
    return Array.from(map.entries());
  }, [rows]);

  return (
    <Stack spacing={2}>
      <div>
        <Typography variant="h5" data-testid="metrics-heading">
          Metrics
        </Typography>
        <Typography variant="body2" color="text.secondary" data-testid="metrics-last-updated">
          Last updated: {lastUpdated} (auto-refresh 10s)
        </Typography>
      </div>

      {groups.length === 0 && (
        <Paper sx={{ p: 2 }}>
          <Typography color="text.secondary" data-testid="metrics-empty">
            No metrics available
          </Typography>
        </Paper>
      )}

      {groups.map(([name, metricRows]) => (
        <TableContainer component={Paper} key={name}>
          <Table size="small" data-testid={`metrics-table-${name}`}>
            <TableHead>
              <TableRow>
                <TableCell colSpan={3}>
                  <Typography variant="subtitle2" sx={{ fontFamily: "monospace" }}>
                    {name}
                  </Typography>
                </TableCell>
              </TableRow>
              <TableRow>
                <TableCell>Labels</TableCell>
                <TableCell align="right">Value</TableCell>
              </TableRow>
            </TableHead>
            <TableBody>
              {metricRows.map((row, i) => (
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
      ))}
    </Stack>
  );
}
