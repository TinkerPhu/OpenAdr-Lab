import {
  Chip,
  Paper,
  Stack,
  Table,
  TableBody,
  TableCell,
  TableContainer,
  TableHead,
  TableRow,
  Typography,
} from "@mui/material";
import { useTrace } from "../api/hooks";

function ModeChip({ mode }: { mode: string }) {
  const color =
    mode === "EXPORT_CAP"
      ? "warning"
      : mode === "IMPORT_CAP"
        ? "info"
        : mode === "PRICE"
          ? "secondary"
          : "default";
  return <Chip label={mode} size="small" color={color} />;
}

function FsmChip({ state }: { state: string }) {
  const color = state.startsWith("Holding")
    ? "success"
    : state.startsWith("Ramp")
      ? "warning"
      : state.startsWith("Delay")
        ? "info"
        : "default";
  return <Chip label={state} size="small" color={color} variant="outlined" />;
}

export function TracePage() {
  const { data: entries, dataUpdatedAt } = useTrace();
  const lastUpdated = dataUpdatedAt ? new Date(dataUpdatedAt).toLocaleString() : "—";

  return (
    <Stack spacing={2}>
      <div>
        <Typography variant="h5" data-testid="trace-heading">
          Decision Trace
        </Typography>
        <Typography variant="body2" color="text.secondary">
          Last {entries?.length ?? 0} controller decisions (newest first). Updated: {lastUpdated}
        </Typography>
      </div>

      <TableContainer component={Paper}>
        <Table size="small" data-testid="trace-table">
          <TableHead>
            <TableRow>
              <TableCell>Time</TableCell>
              <TableCell>Mode</TableCell>
              <TableCell>FSM State</TableCell>
              <TableCell>Active Events</TableCell>
              <TableCell>Winning Intent</TableCell>
              <TableCell>Setpoints</TableCell>
              <TableCell>Reason</TableCell>
            </TableRow>
          </TableHead>
          <TableBody>
            {entries?.map((entry, i) => (
              <TableRow key={i} data-testid={`trace-row-${i}`}>
                <TableCell sx={{ whiteSpace: "nowrap", fontSize: "0.75rem" }}>
                  {new Date(entry.ts).toLocaleTimeString()}
                </TableCell>
                <TableCell>
                  {entry.mode != null ? <ModeChip mode={entry.mode} /> : "—"}
                </TableCell>
                <TableCell>
                  {entry.fsm_state != null ? <FsmChip state={entry.fsm_state} /> : "—"}
                </TableCell>
                <TableCell sx={{ fontSize: "0.75rem" }}>
                  {(entry.active_events ?? []).length > 0
                    ? (entry.active_events ?? []).join(", ")
                    : "—"}
                </TableCell>
                <TableCell sx={{ fontSize: "0.75rem" }}>
                  {entry.winning_intent ?? "—"}
                </TableCell>
                <TableCell sx={{ fontSize: "0.75rem" }}>
                  {entry.setpoints
                    ? `EV: ${entry.setpoints.ev_charge_kw.toFixed(1)}kW | Heat: ${entry.setpoints.heater_kw.toFixed(1)}kW | PV limit: ${entry.setpoints.pv_export_limit_kw != null ? `${entry.setpoints.pv_export_limit_kw.toFixed(2)} kW` : "—"}`
                    : "—"}
                </TableCell>
                <TableCell sx={{ fontSize: "0.75rem", maxWidth: 300 }}>
                  {entry.reason}
                </TableCell>
              </TableRow>
            ))}
            {(!entries || entries.length === 0) && (
              <TableRow>
                <TableCell colSpan={7} align="center">
                  <Typography color="text.secondary">
                    No trace entries yet
                  </Typography>
                </TableCell>
              </TableRow>
            )}
          </TableBody>
        </Table>
      </TableContainer>
    </Stack>
  );
}
