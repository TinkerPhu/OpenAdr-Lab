import { useState } from "react";
import {
  Alert, MenuItem, Paper, Stack, Table, TableBody, TableCell, TableHead, TableRow,
  TextField, Typography,
} from "@mui/material";
import { useFleetHistory, useFleetPower, useFleetSignals } from "../api/hooks";
import { FleetPowerChart } from "../components/FleetPowerChart";
import { FleetTariffChart } from "../components/FleetTariffChart";
import { FleetReactions } from "../components/FleetReactions";
import { formatAge } from "../utils/relativeTime";
import { formatKw } from "../utils/power";
import { byVenName } from "../utils/venOrder";

/**
 * Windows an operator actually asks for, each with a bucket width that keeps
 * the point count near 200 — fine enough to see a reaction, coarse enough that
 * a day does not arrive as 17 000 points per VEN.
 */
const WINDOWS = [
  { minutes: 15, stepSeconds: 5, tickMinutes: 5, label: "15 min" },
  { minutes: 60, stepSeconds: 60, tickMinutes: 10, label: "1 hour" },
  { minutes: 360, stepSeconds: 300, tickMinutes: 60, label: "6 hours" },
  { minutes: 1440, stepSeconds: 900, tickMinutes: 180, label: "24 hours" },
];

export function FleetPage() {
  const [windowIndex, setWindowIndex] = useState(1);
  const chosen = WINDOWS[windowIndex];

  const live = useFleetPower();
  const history = useFleetHistory(chosen.minutes, chosen.stepSeconds);
  const signals = useFleetSignals(chosen.minutes);
  const now = new Date();

  return (
    <Stack spacing={2}>
      <Paper sx={{ p: 2 }} data-testid="fleet-total-card">
        <Typography variant="h6">Fleet power</Typography>
        {live.isError && (
          <Alert severity="error" data-testid="fleet-live-error">
            The fleet feed could not be read.
          </Alert>
        )}
        {live.data && (
          <>
            <Typography variant="h4" data-testid="fleet-total">
              {formatKw(live.data.fleet.netPowerW)}
            </Typography>
            {/* The sum is only as complete as the VENs behind it, and saying
                so is what lets a reader judge it rather than trust it. */}
            <Typography variant="body2" data-testid="fleet-contributors">
              {live.data.fleet.contributingVens} of {live.data.fleet.knownVens} VENs reporting
            </Typography>
          </>
        )}
      </Paper>

      <Paper sx={{ p: 2 }} data-testid="fleet-history-card">
        <Stack direction="row" spacing={2} alignItems="center" mb={1}>
          <Typography variant="h6">Fleet response</Typography>
          <TextField
            select
            size="small"
            value={windowIndex}
            onChange={(e) => setWindowIndex(Number(e.target.value))}
            inputProps={{ "data-testid": "fleet-window-select" }}
            sx={{ minWidth: 120 }}
          >
            {WINDOWS.map((w, i) => (
              <MenuItem key={w.minutes} value={i}>
                {w.label}
              </MenuItem>
            ))}
          </TextField>
        </Stack>

        {signals.data && signals.data.rejectedEvents > 0 && (
          <Alert severity="warning" data-testid="fleet-signals-rejected">
            {signals.data.rejectedEvents} event(s) could not be read — the tariff below
            may be incomplete.
          </Alert>
        )}
        {/* Price above power, on the same axis and the same window, because a
            dip means something different depending on whether the hour got
            expensive or a limit landed. */}
        <Typography variant="subtitle2" color="text.secondary">
          Tariff
        </Typography>
        <FleetTariffChart
          signals={signals.data}
          windowMinutes={chosen.minutes}
          tickMinutes={chosen.tickMinutes}
          nowMs={now.getTime()}
        />

        <Typography variant="subtitle2" color="text.secondary" sx={{ mt: 1 }}>
          Per-VEN power
        </Typography>
        {history.isError && (
          <Alert severity="info" data-testid="fleet-history-error">
            No stored history — the telemetry store is not connected.
          </Alert>
        )}
        {history.data && (
          <FleetPowerChart
            history={history.data}
            windowMinutes={chosen.minutes}
            tickMinutes={chosen.tickMinutes}
            nowMs={now.getTime()}
          />
        )}
      </Paper>

      <FleetReactions />

      <Paper sx={{ p: 2 }} data-testid="fleet-vens-card">
        <Typography variant="h6">VENs</Typography>
        <Table size="small">
          <TableHead>
            <TableRow>
              <TableCell>VEN</TableCell>
              <TableCell align="right">Net power</TableCell>
              <TableCell>State</TableCell>
              <TableCell>Last message</TableCell>
            </TableRow>
          </TableHead>
          <TableBody>
            {byVenName(live.data?.vens ?? [], (v) => v.venName).map((v) => (
              <TableRow key={v.venName} data-testid={`fleet-ven-${v.venName}`}>
                <TableCell>{v.venName}</TableCell>
                <TableCell align="right">{formatKw(v.netPowerW)}</TableCell>
                {/* `offline` is the broker's last will, published on a VEN's
                    behalf when it died without saying goodbye — the only way
                    to tell "gone" from "quiet". */}
                <TableCell>{v.state ?? "unknown"}</TableCell>
                <TableCell>{formatAge(v.receivedAt, now)}</TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
        {live.data?.vens.length === 0 && (
          <Typography variant="body2" data-testid="fleet-no-vens">
            No VEN has published yet.
          </Typography>
        )}
      </Paper>
    </Stack>
  );
}
