import {
  Alert, Paper, Stack, Table, TableBody, TableCell, TableHead, TableRow, Typography,
} from "@mui/material";
import { useFleetHistory, useFleetPower } from "../api/hooks";
import { Sparkline } from "../components/Sparkline";
import { formatAge } from "../utils/relativeTime";
import { formatKw } from "../utils/power";
import { FleetReactions } from "../components/FleetReactions";

/** The window the page opens on. Long enough to contain a dispatch window. */
const HISTORY_MINUTES = 60;
const HISTORY_STEP_S = 60;

export function FleetPage() {
  const live = useFleetPower();
  const history = useFleetHistory(HISTORY_MINUTES, HISTORY_STEP_S);
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
        <Typography variant="h6">Last hour</Typography>
        {history.isError && (
          <Alert severity="info" data-testid="fleet-history-error">
            No stored history — the telemetry store is not connected.
          </Alert>
        )}
        {history.data && (
          <>
            <Sparkline
              samples={history.data.fleet.map((s) => ({ ts: s.ts, value: s.netPowerW }))}
              label={`Fleet power over the last ${HISTORY_MINUTES} minutes`}
            />
            {/* A 1-minute rollup and a 5-second raw series are both true and
                not the same resolution; a reader comparing two windows has to
                be told which one they have. */}
            <Typography variant="caption" data-testid="fleet-history-source">
              {history.data.source === "raw" ? "raw samples" : "1-minute means"}, {" "}
              {history.data.stepSeconds}s buckets
            </Typography>
          </>
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
            {live.data?.vens.map((v) => (
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
