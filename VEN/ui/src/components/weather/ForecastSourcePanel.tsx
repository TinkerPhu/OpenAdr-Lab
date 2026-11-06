import {
  Alert,
  Chip,
  Paper,
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableRow,
  Typography,
} from "@mui/material";
import type { ForecastSourceView } from "../../api/types";

/** "pv/pv" -> "PV array pv", "base_load" -> "Base load". */
function seriesLabel(key: string): string {
  if (key === "base_load") return "Base load";
  if (key.startsWith("pv/")) return `PV array ${key.slice(3)}`;
  return key;
}

/** Seconds as the largest sensible unit: "45 s", "12 min", "3 h". */
function ageLabel(ageS: number | null): string {
  if (ageS === null) return "—";
  if (ageS < 90) return `${ageS} s`;
  if (ageS < 5400) return `${Math.round(ageS / 60)} min`;
  return `${Math.round(ageS / 3600)} h`;
}

/** Where each forecast the planner uses comes from: the lab-forecaster's retained message or this VEN's
 * own computation, how old the newest forecaster message is, and why one was refused. */
export function ForecastSourcePanel({ sources }: { sources: Record<string, ForecastSourceView> }) {
  const rows = Object.entries(sources);
  return (
    <Paper variant="outlined" sx={{ p: 2 }} data-testid="forecast-source-panel">
      <Typography variant="subtitle1" sx={{ mb: 1 }}>
        Forecast source
      </Typography>
      <Table size="small">
        <TableHead>
          <TableRow>
            <TableCell>Series</TableCell>
            <TableCell>Source in use</TableCell>
            <TableCell>Model</TableCell>
            <TableCell align="right">Newest message</TableCell>
          </TableRow>
        </TableHead>
        <TableBody>
          {rows.map(([key, view]) => (
            <TableRow key={key} data-testid={`forecast-source-${key}`}>
              <TableCell>{seriesLabel(key)}</TableCell>
              <TableCell>
                <Chip
                  size="small"
                  data-testid={`forecast-source-kind-${key}`}
                  label={view.source === "forecaster" ? "Forecaster" : "In-process"}
                  color={view.source === "forecaster" ? "success" : "default"}
                />
              </TableCell>
              <TableCell>{view.model_version ?? "—"}</TableCell>
              <TableCell align="right" data-testid={`forecast-source-age-${key}`}>
                {ageLabel(view.age_s)}
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
      {rows
        .filter(([, view]) => view.rejected_reason !== null)
        .map(([key, view]) => (
          <Alert
            key={key}
            severity="warning"
            sx={{ mt: 1 }}
            data-testid={`forecast-source-rejected-${key}`}
          >
            {seriesLabel(key)}: the newest forecaster message was refused ({view.rejected_reason}).
          </Alert>
        ))}
    </Paper>
  );
}
