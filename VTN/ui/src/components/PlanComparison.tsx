import { Alert, Stack, Table, TableBody, TableCell, TableHead, TableRow, Typography } from "@mui/material";
import { useFleetPlan } from "../api/hooks";
import { planDelta, replanProgress, type DirectionDelta } from "../utils/planDelta";
import { FleetPlanChart } from "./FleetPlanChart";

const MINUS = "−";
const signed = (n: number) => `${n < 0 ? MINUS : "+"}${Math.abs(n).toFixed(1)}`;

function DirectionRow({ id, label, d }: { id: string; label: string; d: DirectionDelta }) {
  return (
    <TableRow data-testid={`plan-delta-${id}`}>
      <TableCell>{label}</TableCell>
      <TableCell>
        {d.before.energyKwh.toFixed(1)} → {d.after.energyKwh.toFixed(1)} kWh
        {d.deltaEnergyKwh !== null && ` (${signed(d.deltaEnergyKwh)} kWh)`}
      </TableCell>
      <TableCell>
        {d.before.peakKw.toFixed(1)} → {d.after.peakKw.toFixed(1)} kW
        {d.deltaPeakKw !== null && ` (${signed(d.deltaPeakKw)} kW)`}
      </TableCell>
    </TableRow>
  );
}

/**
 * What a publish did to the fleet's plans: the plan as of just before it, against the plan now.
 *
 * Both come from `GET /api/fleet/plan`, which sums stored plans as of an instant, so a reload
 * reproduces this and the page keeps no copy. The publish instant is the event's own
 * `createdDateTime`. Progress is stated, because the delta is only the effect of the tariff once
 * every targeted VEN has replanned; until then it is part way there.
 */
export function PlanComparison({
  publishedAt,
  targets,
  nowMs,
}: {
  /** The published event's `createdDateTime`. */
  publishedAt: string;
  /** The event's targets; empty means every VEN. */
  targets: string[];
  nowMs: number;
}) {
  const publishedMs = Date.parse(publishedAt);
  const reference = useFleetPlan(new Date(publishedMs - 1000).toISOString());
  const result = useFleetPlan();

  if (reference.isError || result.isError) {
    return (
      <Alert severity="info" data-testid="plan-comparison-unavailable">
        No plan history — the telemetry store is not connected, so there is nothing to compare.
      </Alert>
    );
  }
  if (!reference.data || !result.data) {
    return (
      <Typography variant="body2" data-testid="plan-comparison-loading">
        Reading the fleet's plans…
      </Typography>
    );
  }
  if (reference.data.points.length === 0) {
    const missing = reference.data.vensWithoutPlan;
    return (
      <Alert severity="info" data-testid="plan-comparison-no-reference">
        No VEN had a plan stored just before this was published
        {missing.length > 0 && ` (${missing.slice(0, 5).join(", ")}${missing.length > 5 ? ", …" : ""})`}
        , so there is no earlier plan to compare with.
      </Alert>
    );
  }

  const delta = planDelta(reference.data, result.data);
  const progress = replanProgress(result.data, publishedMs, targets);

  return (
    <Stack spacing={1}>
      <Typography variant="body2" data-testid="plan-replan-progress">
        {progress.label}
      </Typography>
      {!progress.settled && (
        <Alert severity="warning" data-testid="plan-delta-provisional">
          The change below is not final: it is part way there until every targeted VEN has replanned.
        </Alert>
      )}
      <FleetPlanChart plan={result.data} reference={reference.data} nowMs={nowMs} />
      {delta.window && (
        <Table size="small">
          <TableHead>
            <TableRow>
              <TableCell>Over the window both plans cover</TableCell>
              <TableCell>Planned energy</TableCell>
              <TableCell>Planned peak</TableCell>
            </TableRow>
          </TableHead>
          <TableBody>
            <DirectionRow id="import" label="Import" d={delta.import} />
            <DirectionRow id="export" label="Export" d={delta.export} />
          </TableBody>
        </Table>
      )}
      <Typography variant="caption" color="text.secondary">
        Faint: the plan as of just before the publish at {new Date(publishedMs).toLocaleTimeString()}.
        Solid: the plan now. Forecast changes move plans too, not only the tariff.
      </Typography>
    </Stack>
  );
}
