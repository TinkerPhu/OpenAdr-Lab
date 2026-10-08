import { useMemo } from "react";
import {
  Paper, Stack, Table, TableBody, TableCell, TableContainer,
  TableHead, TableRow, Typography,
} from "@mui/material";
import { useMetrics } from "../api/hooks";
import { formatLabels, parsePrometheusText, type MetricRow } from "@lab/charts/prometheus";

export function MetricsPage() {
  const { data: raw = "", dataUpdatedAt } = useMetrics();

  const rows = useMemo(() => parsePrometheusText(raw), [raw]);
  const lastUpdated = dataUpdatedAt ? new Date(dataUpdatedAt).toLocaleString() : "—";

  // Group rows by metric name
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
