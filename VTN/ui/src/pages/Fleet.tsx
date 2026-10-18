import { useState } from "react";
import {
  Alert, MenuItem, Paper, Stack, Table, TableBody, TableCell, TableHead, TableRow,
  TextField, Typography,
} from "@mui/material";
import { useFleetHistory, useFleetPlan, useFleetPower, useFleetSignals } from "../api/hooks";
import { FleetPlanChart } from "../components/FleetPlanChart";
import { FleetPowerChart } from "../components/FleetPowerChart";
import { FleetTariffChart } from "../components/FleetTariffChart";
import { FleetReactions } from "../components/FleetReactions";
import { formatAge } from "../utils/relativeTime";
import { formatKwOrDash } from "@lab/ui/unitFormat";
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
  const [windowIndex, setWindowIndex] = useState(3);
  const chosen = WINDOWS[windowIndex];

  const live = useFleetPower();
  const history = useFleetHistory(chosen.minutes, chosen.stepSeconds);
  const signals = useFleetSignals(chosen.minutes);
  const plan = useFleetPlan();
  const now = new Date();
  const newestPlan = plan.data?.plans
    .map((p) => p.createdAt)
    .reduce<string | undefined>((a, b) => (a === undefined || b > a ? b : a), undefined);

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
              {formatKwOrDash(live.data.fleet.netPowerKw)}
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
            reportingVens={live.data?.fleet.contributingVens ?? 0}
          />
        )}
      </Paper>

      <Paper sx={{ p: 2 }} data-testid="fleet-plan-card">
        <Typography variant="h6">Fleet plan</Typography>
        <Typography variant="body2" color="text.secondary">
          What the VENs plan to import and export, from the plans they last adopted. Where the
          fleet response above shows what sites did, this shows what they intend.
        </Typography>
        {plan.isError && (
          <Alert severity="info" data-testid="fleet-plan-error">
            No plan history — the telemetry store is not connected.
          </Alert>
        )}
        {plan.data && (
          <>
            {/* A total over eleven VENs and over twenty look the same once drawn; say which. */}
            <Typography variant="body2" data-testid="fleet-plan-contributors">
              {plan.data.plans.length} of {plan.data.plans.length + plan.data.vensWithoutPlan.length}{" "}
              VENs have a plan
              {newestPlan && <> · newest adopted {formatAge(newestPlan, now)}</>}
            </Typography>
            <FleetPlanChart plan={plan.data} nowMs={now.getTime()} />
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
            {byVenName(live.data?.vens ?? [], (v) => v.venName).map((v) => (
              <TableRow key={v.venName} data-testid={`fleet-ven-${v.venName}`}>
                <TableCell>{v.venName}</TableCell>
                <TableCell align="right">{formatKwOrDash(v.netPowerKw)}</TableCell>
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
