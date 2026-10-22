import { Fragment, useState } from "react";
import {
  Alert, Button, Chip, IconButton, MenuItem, Paper, Stack, Table, TableBody, TableCell,
  TableHead, TableRow, TextField, Typography,
} from "@mui/material";
import ExpandMoreIcon from "@mui/icons-material/ExpandMore";
import ExpandLessIcon from "@mui/icons-material/ExpandLess";
import {
  useFleetHistory, useFleetPlan, useFleetPower, useFleetSignals, useSetFleetObjective,
  useSetVenObjective, useVenPlan,
} from "../api/hooks";
import type { FleetVenLive } from "../api/types";
import { FleetPlanChart } from "../components/FleetPlanChart";
import { PlanComparison } from "../components/PlanComparison";
import { ServiceDelivered } from "../components/ServiceDelivered";
import { FleetPowerChart } from "../components/FleetPowerChart";
import { VenPlanChart } from "../components/VenPlanChart";
import {
  fleetObjectiveChoices, latestObjectiveSwitch, objectiveCounts, objectiveMix, sourceLabel,
} from "../utils/fleetObjective";
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

/** One VEN's objective as it reports it, who set it, and whether the fleet's command is still
 *  on its way (D12). */
function ObjectiveCell({ ven }: { ven: FleetVenLive }) {
  if (!ven.objective) {
    return <TableCell data-testid={`fleet-objective-${ven.venName}`}>—</TableCell>;
  }
  return (
    <TableCell data-testid={`fleet-objective-${ven.venName}`}>
      <Stack direction="row" spacing={0.5} alignItems="center">
        <span>{ven.objective.objective}</span>
        <Chip size="small" variant="outlined" label={sourceLabel(ven.objective.source)} />
        {ven.commandPending && (
          <Chip
            size="small"
            color="warning"
            label={`pending: ${ven.command?.objective ?? "release"}`}
            data-testid={`fleet-pending-${ven.venName}`}
          />
        )}
      </Stack>
    </TableCell>
  );
}

/** Command this VEN's objective, or release the fleet's command so it returns to its own. */
function CommandCell({ ven }: { ven: FleetVenLive }) {
  const setObjective = useSetVenObjective();
  const choices = ven.objective?.available_objectives ?? [];
  return (
    <TableCell>
      <Stack direction="row" spacing={1} alignItems="center">
        <TextField
          select
          size="small"
          value={ven.command?.objective ?? ""}
          disabled={choices.length === 0}
          onChange={(e) => setObjective.mutate({ venName: ven.venName, objective: e.target.value })}
          inputProps={{ "data-testid": `fleet-command-${ven.venName}` }}
          sx={{ minWidth: 150 }}
        >
          {choices.map((o) => (
            <MenuItem key={o} value={o}>
              {o}
            </MenuItem>
          ))}
        </TextField>
        <Button
          size="small"
          disabled={!ven.command}
          onClick={() => setObjective.mutate({ venName: ven.venName, objective: null })}
          data-testid={`fleet-release-${ven.venName}`}
        >
          Release
        </Button>
      </Stack>
      {setObjective.isError && (
        <Typography variant="caption" color="error">
          {(setObjective.error as Error).message}
        </Typography>
      )}
    </TableCell>
  );
}

/** The VEN's adopted plan, fetched only while the row is open. */
function VenPlanRow({ venName, nowMs }: { venName: string; nowMs: number }) {
  const plan = useVenPlan(venName, true);
  if (plan.isError) {
    return (
      <Typography variant="body2" data-testid={`fleet-plan-missing-${venName}`}>
        {venName} has published no plan yet.
      </Typography>
    );
  }
  return plan.data ? <VenPlanChart plan={plan.data} nowMs={nowMs} /> : null;
}

/** Switch every VEN to one objective, one VEN at a time, or release them all. */
function FleetObjectiveControl({ vens }: { vens: FleetVenLive[] }) {
  const setFleet = useSetFleetObjective();
  const [choice, setChoice] = useState("");
  const choices = fleetObjectiveChoices(vens);
  return (
    <Stack direction="row" spacing={1} alignItems="center" mb={1} data-testid="fleet-objective-control">
      <TextField
        select
        size="small"
        label="Fleet objective"
        value={choice}
        onChange={(e) => setChoice(e.target.value)}
        inputProps={{ "data-testid": "fleet-objective-select" }}
        sx={{ minWidth: 180 }}
      >
        {choices.map((o) => (
          <MenuItem key={o} value={o}>
            {o}
          </MenuItem>
        ))}
      </TextField>
      <Button
        variant="contained"
        size="small"
        disabled={!choice}
        onClick={() => setFleet.mutate(choice)}
        data-testid="fleet-objective-apply"
      >
        Apply to all
      </Button>
      <Button size="small" onClick={() => setFleet.mutate(null)} data-testid="fleet-objective-release">
        Release all
      </Button>
      {setFleet.isError && (
        <Typography variant="caption" color="error">
          {(setFleet.error as Error).message}
        </Typography>
      )}
    </Stack>
  );
}

export function FleetPage() {
  const [windowIndex, setWindowIndex] = useState(3);
  const [openVen, setOpenVen] = useState<string | null>(null);
  const chosen = WINDOWS[windowIndex];

  const live = useFleetPower();
  const history = useFleetHistory(chosen.minutes, chosen.stepSeconds);
  const signals = useFleetSignals(chosen.minutes);
  const plan = useFleetPlan();
  // The newest objective switch, compared the way a published tariff is: the plan as it stood just
  // before the first VEN changed against the plan now, with how many VENs have replanned.
  const changedAt = latestObjectiveSwitch(live.data?.vens ?? []);
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
            {live.data && (
              <Typography variant="body2" data-testid="fleet-plan-objectives">
                Objectives: {objectiveMix(objectiveCounts(live.data.vens)) || "none reported"}
                {changedAt && <> · objective switched at {new Date(changedAt).toLocaleTimeString()}</>}
              </Typography>
            )}
            {changedAt ? (
              <PlanComparison
                changedAt={changedAt}
                what="the objective switch"
                targets={[]}
                nowMs={now.getTime()}
              />
            ) : (
              <FleetPlanChart plan={plan.data} nowMs={now.getTime()} />
            )}
          </>
        )}
      </Paper>

      {changedAt && (
        <Paper sx={{ p: 2 }} data-testid="fleet-service-card">
          <Typography variant="h6">Service delivered</Typography>
          <ServiceDelivered changedAt={changedAt} nowMs={now.getTime()} />
        </Paper>
      )}

      <FleetReactions />

      <Paper sx={{ p: 2 }} data-testid="fleet-vens-card">
        <Typography variant="h6">VENs</Typography>
        <FleetObjectiveControl vens={live.data?.vens ?? []} />
        <Table size="small">
          <TableHead>
            <TableRow>
              <TableCell />
              <TableCell>VEN</TableCell>
              <TableCell align="right">Net power</TableCell>
              <TableCell>State</TableCell>
              <TableCell>Objective</TableCell>
              <TableCell>Fleet command</TableCell>
              <TableCell>Last message</TableCell>
            </TableRow>
          </TableHead>
          <TableBody>
            {byVenName(live.data?.vens ?? [], (v) => v.venName).map((v) => (
              <Fragment key={v.venName}>
                <TableRow data-testid={`fleet-ven-${v.venName}`}>
                  <TableCell padding="checkbox">
                    <IconButton
                      size="small"
                      aria-label={`plan of ${v.venName}`}
                      onClick={() => setOpenVen(openVen === v.venName ? null : v.venName)}
                      data-testid={`fleet-expand-${v.venName}`}
                    >
                      {openVen === v.venName ? <ExpandLessIcon /> : <ExpandMoreIcon />}
                    </IconButton>
                  </TableCell>
                  <TableCell>{v.venName}</TableCell>
                  <TableCell align="right">{formatKwOrDash(v.netPowerKw)}</TableCell>
                  {/* `offline` is the broker's last will, published on a VEN's
                      behalf when it died without saying goodbye — the only way
                      to tell "gone" from "quiet". */}
                  <TableCell>{v.state ?? "unknown"}</TableCell>
                  <ObjectiveCell ven={v} />
                  <CommandCell ven={v} />
                  <TableCell>{formatAge(v.receivedAt, now)}</TableCell>
                </TableRow>
                {openVen === v.venName && (
                  <TableRow data-testid={`fleet-plan-row-${v.venName}`}>
                    <TableCell colSpan={7}>
                      <VenPlanRow venName={v.venName} nowMs={now.getTime()} />
                    </TableCell>
                  </TableRow>
                )}
              </Fragment>
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
