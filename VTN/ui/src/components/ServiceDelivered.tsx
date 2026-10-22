import { Alert, Stack, Table, TableBody, TableCell, TableHead, TableRow, Typography } from "@mui/material";
import { formatEnergyKwh } from "@lab/ui/unitFormat";
import { useFleetService } from "../api/hooks";
import type { FleetServiceReport } from "../api/types";
import { joinAssets, serviceWindows, type BeforeAfter } from "../utils/fleetService";

/** The two quantities the card shows, declared once: how each is read off a report and named. */
const QUANTITIES = [
  {
    id: "unserved",
    label: "Unserved energy",
    testId: "service-unserved",
    total: (r: FleetServiceReport) => r.unservedEnergyKwh,
    asset: (row: { unserved: BeforeAfter }) => row.unserved,
  },
  {
    id: "carry-over",
    label: "Carry-over (change in stored energy)",
    testId: "service-carry-over",
    total: (r: FleetServiceReport) => r.carryOverKwh,
    asset: (row: { carryOver: BeforeAfter }) => row.carryOver,
  },
] as const;

const kwh = (value: number | null) => (value === null ? "no data" : formatEnergyKwh(value));
const beforeAfter = (b: BeforeAfter) => `${kwh(b.before)} → ${kwh(b.after)}`;

/**
 * What the fleet delivered to its owners over the span since an objective switch, against the equal
 * span before it: the energy its owners were left short of and the change in what its stores hold.
 * An objective that serves less saves energy and shows up here, which a plan comparison cannot say.
 *
 * Both figures are what each asset declared (`unserved_energy_kwh`, `stored_energy_kwh`), read from
 * the stored telemetry by `GET /api/fleet/service`. A window older than the raw telemetry is
 * unavailable, never zero. Carry-over depends on where in the day a window starts and ends.
 */
export function ServiceDelivered({ changedAt, nowMs }: { changedAt: string; nowMs: number }) {
  const windows = serviceWindows(changedAt, nowMs);
  const before = useFleetService(windows?.before.from ?? changedAt, windows?.before.to ?? changedAt, !!windows);
  const after = useFleetService(windows?.after.from ?? changedAt, windows?.after.to ?? changedAt, !!windows);

  if (!windows) {
    return (
      <Typography variant="body2" data-testid="service-too-early">
        The objective switch is too recent to compare what was delivered.
      </Typography>
    );
  }
  if (before.isError || after.isError) {
    return (
      <Alert severity="info" data-testid="service-error">
        The service level could not be read — the telemetry store is not connected.
      </Alert>
    );
  }
  if (!before.data || !after.data) {
    return (
      <Typography variant="body2" data-testid="service-loading">
        Reading what the fleet delivered…
      </Typography>
    );
  }
  const unavailable = [before.data, after.data].find((d) => !d.available);
  if (unavailable || !before.data.report || !after.data.report) {
    return (
      <Alert severity="info" data-testid="service-unavailable">
        Not available for this window: {unavailable?.reason ?? "no report"}.
      </Alert>
    );
  }
  const b = before.data.report;
  const a = after.data.report;
  const hours = (Date.parse(windows.after.to) - Date.parse(windows.after.from)) / 3_600_000;
  const venCount = (r: FleetServiceReport) => r.unservedEnergyKwh.contributingVens;
  const rows = joinAssets(b, a);

  return (
    <Stack spacing={1}>
      <Table size="small">
        <TableHead>
          <TableRow>
            <TableCell>Over {hours.toFixed(1)} h before → after the switch</TableCell>
            <TableCell>Fleet</TableCell>
          </TableRow>
        </TableHead>
        <TableBody>
          {QUANTITIES.map((q) => (
            <TableRow key={q.id}>
              <TableCell>{q.label}</TableCell>
              <TableCell data-testid={q.testId}>
                {kwh(q.total(b).kwh)} → {kwh(q.total(a).kwh)}
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
      <Typography variant="body2" data-testid="service-contributors">
        Built from {venCount(a)} VENs after, {venCount(b)} before.
      </Typography>
      <Typography variant="caption" color="text.secondary">
        Unserved energy is what owners were left short of (an EV leaving below its target, a tank below
        its comfort floor, a job past its deadline). Carry-over is the change in energy held in batteries,
        EVs and tanks, and depends on where in the day each window starts and ends.
      </Typography>
      <Table size="small" data-testid="service-drill-down">
        <TableHead>
          <TableRow>
            <TableCell>VEN</TableCell>
            <TableCell>Asset</TableCell>
            {QUANTITIES.map((q) => (
              <TableCell key={q.id}>{q.label}</TableCell>
            ))}
          </TableRow>
        </TableHead>
        <TableBody>
          {rows.map((row) => (
            <TableRow key={`${row.venName}/${row.assetId}`} data-testid={`service-asset-${row.venName}-${row.assetId}`}>
              <TableCell>{row.venName}</TableCell>
              <TableCell>{row.assetId}</TableCell>
              {QUANTITIES.map((q) => (
                <TableCell key={q.id}>{beforeAfter(q.asset(row))}</TableCell>
              ))}
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </Stack>
  );
}
