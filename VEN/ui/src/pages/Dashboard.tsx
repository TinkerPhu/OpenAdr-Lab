import { assetsOfType } from "../api/assets";
import { useMemo } from "react";
import { Link as RouterLink } from "react-router-dom";
import { Box, Chip, Grid, Paper, Stack, Table, TableBody, TableCell, TableHead, TableRow, Typography } from "@mui/material";
import { useHealth, usePlan, usePrograms, useEvents, useRequests, useSensor, useReports, useSim, useCapacity, useLedger, useLedgerHistories, useVtnStatus, useTasksStatus } from "../api/hooks";
import type { OadrCapacityState, AssetLedgerEntry, AssetLedgerHistory } from "../api/types";
import { SessionProgressBoard } from "../components/sessions/SessionProgressBoard";
import { DashboardStatusPanel } from "../components/dashboard/StatusRows";
import { SimAssetTiles } from "../components/dashboard/SimAssetTiles";
import { formatFixedOrDash, formatKwOrDash } from "@lab/ui/unitFormat";
import { OBJECTIVE_LABELS } from "../components/planner/objectiveLabels";

function ModeBadge({ mode }: { mode?: string }) {
  if (!mode || mode === "IDLE") return <Chip label="IDLE" size="small" />;
  const color = mode === "EXPORT_CAP" ? "warning" : mode === "IMPORT_CAP" ? "info" : mode === "PRICE" ? "secondary" : "default";
  return <Chip label={mode} size="small" color={color} />;
}

function complianceChip(current_kw: number, limit_kw: number | null, label: string) {
  if (limit_kw == null) return null;
  const ratio = current_kw / limit_kw;
  const color = ratio > 1.0 ? "error" : ratio > 0.8 ? "warning" : "success";
  const text = ratio > 1.0 ? "Over limit" : ratio > 0.8 ? "Near limit" : "OK";
  return (
    <Chip
      label={`${label}: ${text} (${current_kw.toFixed(1)} / ${limit_kw.toFixed(1)} kW)`}
      size="small"
      color={color}
      sx={{ mt: 0.5 }}
    />
  );
}

function CapacityCard({
  capacity,
  netPowerKw,
}: {
  capacity: OadrCapacityState | undefined;
  netPowerKw: number | null;
}) {
  const importKw = netPowerKw != null && netPowerKw > 0 ? netPowerKw : 0;
  const exportKw = netPowerKw != null && netPowerKw < 0 ? -netPowerKw : 0;

  const fmtTs = (v: string | null | undefined) => {
    if (!v) return "—";
    return new Date(v).toLocaleTimeString();
  };

  return (
    <Paper sx={{ p: 2, height: "100%" }} data-testid="dash-capacity-card">
      <Typography variant="subtitle2" color="text.secondary" mb={1}>
        OpenADR Capacity
      </Typography>
      <Stack spacing={0.5}>
        <Typography variant="body2">Import limit: {formatKwOrDash(capacity?.import_limit_kw, 1)}</Typography>
        <Typography variant="body2">Export limit: {formatKwOrDash(capacity?.export_limit_kw, 1)}</Typography>
        <Typography variant="body2">Subscribed: {formatKwOrDash(capacity?.import_subscription_kw, 1)}</Typography>
        <Typography variant="body2">Reserved: {formatKwOrDash(capacity?.import_reservation_kw, 1)}</Typography>
        <Typography variant="caption" color="text.secondary">
          Updated: {fmtTs(capacity?.last_updated)}
        </Typography>
        {complianceChip(importKw, capacity?.import_limit_kw ?? null, "Import")}
        {complianceChip(exportKw, capacity?.export_limit_kw ?? null, "Export")}
      </Stack>
    </Paper>
  );
}

function ledgerDuration(startedAt: string): string {
  const elapsedMs = Date.now() - new Date(startedAt).getTime();
  const hours = Math.floor(elapsedMs / 3_600_000);
  const mins = Math.floor((elapsedMs % 3_600_000) / 60_000);
  return hours > 0 ? `${hours}h ${mins}m` : `${mins}m`;
}

/** One asset's answer to `GET /ledger?asset_id=`, as `useLedgerHistories` holds it. */
type LedgerHistoryResult = { data?: AssetLedgerHistory; isError: boolean };

/** The closed (archived monthly) ledger periods of every asset, newest first. */
function ClosedLedgerPeriods({ histories }: { histories: LedgerHistoryResult[] }) {
  const periods = histories
    .flatMap((h) => h.data?.closed_periods ?? [])
    .sort((a, b) => b.period_start.localeCompare(a.period_start) || a.asset_id.localeCompare(b.asset_id));
  // A failed or unanswerable query is not "none": say which, and never show the empty state for it.
  const failed = histories.some((h) => h.isError);
  const unknown = histories.some((h) => h.data?.closed_periods === null);
  const loaded = histories.length > 0 && histories.every((h) => h.data !== undefined);

  return (
    <Box mt={2} data-testid="dash-ledger-closed">
      <Typography variant="subtitle2" color="text.secondary" mb={0.5}>
        Closed periods
      </Typography>
      {failed && (
        <Typography variant="body2" color="error">Closed periods could not be loaded</Typography>
      )}
      {unknown && (
        <Typography variant="body2" color="text.secondary">
          Unknown: the history store is disabled on this VEN
        </Typography>
      )}
      {periods.length > 0 ? (
        <Table size="small">
          <TableHead>
            <TableRow>
              <TableCell>Period</TableCell>
              <TableCell>Asset</TableCell>
              <TableCell align="right">Energy kWh</TableCell>
              <TableCell align="right">Cost €</TableCell>
              <TableCell align="right">CO₂ kg</TableCell>
            </TableRow>
          </TableHead>
          <TableBody>
            {periods.map((p) => (
              <TableRow key={`${p.asset_id}-${p.period_start}`} data-testid="dash-ledger-closed-period">
                <TableCell>{p.period_start.slice(0, 10)} → {p.period_end.slice(0, 10)}</TableCell>
                <TableCell>{p.asset_id}</TableCell>
                <TableCell align="right">{p.energy_kwh.toFixed(3)}</TableCell>
                <TableCell align="right">€{p.cost_eur.toFixed(4)}</TableCell>
                <TableCell align="right">{p.co2_kg.toFixed(3)}</TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      ) : (
        loaded && !failed && !unknown && (
          <Typography variant="body2" color="text.secondary">No closed periods yet</Typography>
        )
      )}
    </Box>
  );
}

function LedgerCard({ entries, histories }: { entries: AssetLedgerEntry[]; histories: LedgerHistoryResult[] }) {
  const earliest = useMemo(() => {
    return entries.reduce<string | null>((min, e) => {
      if (!e.started_at) return min;
      return min === null || e.started_at < min ? e.started_at : min;
    }, null);
  }, [entries]);

  const sinceLabel = earliest
    ? `${new Date(earliest).toLocaleTimeString()} (${ledgerDuration(earliest)})`
    : null;

  return (
    <Paper sx={{ p: 2 }} data-testid="dash-ledger-card">
      <Stack direction="row" alignItems="baseline" spacing={1} mb={1}>
        <Typography variant="subtitle2" color="text.secondary">
          Energy Ledger
        </Typography>
        {sinceLabel && (
          <Typography variant="caption" color="text.secondary" data-testid="dash-ledger-since">
            running since {sinceLabel}
          </Typography>
        )}
      </Stack>
      {entries.length === 0 ? (
        <Typography variant="body2" color="text.secondary">No data yet</Typography>
      ) : (
        <Table size="small">
          <TableHead>
            <TableRow>
              <TableCell>Asset</TableCell>
              <TableCell align="right">Energy kWh</TableCell>
              <TableCell align="right">Cost €</TableCell>
              <TableCell align="right">CO₂ g</TableCell>
            </TableRow>
          </TableHead>
          <TableBody>
            {entries.map((l) => (
              <TableRow key={l.asset_id}>
                <TableCell>{l.asset_id}</TableCell>
                <TableCell align="right">{l.energy_kwh.toFixed(3)}</TableCell>
                <TableCell align="right">€{l.cost_eur.toFixed(4)}</TableCell>
                <TableCell align="right">{l.co2_g.toFixed(1)}</TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      )}
      <ClosedLedgerPeriods histories={histories} />
    </Paper>
  );
}

export function DashboardPage() {
  const health = useHealth();
  const programs = usePrograms();
  const events = useEvents();
  const sensor = useSensor();
  const reports = useReports();
  const sim = useSim();
  const capacity = useCapacity();
  const ledger = useLedger();
  const ledgerAssetIds = useMemo(() => (ledger.data ?? []).map((l) => l.asset_id), [ledger.data]);
  const ledgerHistories = useLedgerHistories(ledgerAssetIds);
  const plan = usePlan();
  const requests = useRequests();
  const vtnStatus = useVtnStatus();
  const tasksStatus = useTasksStatus();

  // WP-T1 (docs/history/project_journal.md, search "WP-T"): /health now returns
  // {status, components} — read the real status instead of assuming "ok"
  // whenever a response merely arrived (same bug App.tsx's HealthChip had).
  const healthStatus = health.isError ? "offline" : health.data ? health.data.status : "unknown";
  const netPowerKw = sim.data?.grid.net_power_kw ?? null;

  return (
    <Grid container spacing={2}>
      {/* WP-T8 (docs/history/project_journal.md, search "WP-T" §3.3): three traffic-light
          status rows combining WP-T1/T2/T3's signals into the "is everything
          okay right now" answer the Dashboard didn't have before. */}
      <Grid item xs={12}>
        <DashboardStatusPanel
          vtnStatus={vtnStatus.data}
          plan={plan.data}
          tasks={tasksStatus.data ?? []}
        />
      </Grid>

      <Grid item xs={12} md={3}>
        <Paper sx={{ p: 2 }} data-testid="dash-health-card">
          <Typography variant="h6">
            Health
          </Typography>
          <Typography data-testid="dash-health-value">{healthStatus}</Typography>
          {/*
            Name what is actually wrong. The card used to show only the
            overall word, so a degraded component was visible as "degraded"
            with no way to tell which -- and `fleet_telemetry` deliberately
            does not change the overall status (a fleet view going dark is an
            observability problem, not an operational one), so it would not
            have shown here at all.
          */}
          <Stack spacing={0.25} mt={1}>
            {Object.entries(health.data?.components ?? {})
              .filter(([, c]) => c && c.status !== "ok")
              .map(([name, c]) => (
                <Typography
                  key={name}
                  variant="body2"
                  color="warning.main"
                  data-testid={`dash-health-component-${name}`}
                >
                  {name.replace(/_/g, " ")}
                  {c.detail ? `: ${c.detail}` : ""}
                </Typography>
              ))}
            {/* GB-50: not a fault, so not in the list above and not a warning colour. A unit
                the VTN left undeclared was read from the profile default; said here because
                nothing else would show it (`ui-transparency`). */}
            {Object.keys(health.data?.wire_assumptions ?? {}).length > 0 && (
              <Typography variant="body2" color="text.secondary" data-testid="dash-wire-assumptions">
                wire assumptions: unit not declared, profile default used for{" "}
                {Object.entries(health.data?.wire_assumptions ?? {})
                  .map(([payloadType, count]) => `${payloadType} (${count})`)
                  .join(", ")}
              </Typography>
            )}
          </Stack>
        </Paper>
      </Grid>

      <Grid item xs={12} md={3}>
        <Paper sx={{ p: 2 }} data-testid="dash-programs-card">
          <Typography variant="h6">
            Programs
          </Typography>
          <Typography variant="h4" data-testid="dash-programs-count">
            {programs.data?.length ?? 0}
          </Typography>
          <Stack spacing={0.5} mt={1}>
            {programs.data?.slice(0, 3).map((p) => (
              <Typography key={p.id} variant="body2">
                {p.programName ?? p.id}
              </Typography>
            ))}
          </Stack>
        </Paper>
      </Grid>

      <Grid item xs={12} md={3}>
        <Paper sx={{ p: 2 }} data-testid="dash-events-card">
          <Typography variant="h6">
            Events
          </Typography>
          <Typography variant="h4" data-testid="dash-events-count">
            {events.data?.length ?? 0}
          </Typography>
        </Paper>
      </Grid>

      <Grid item xs={12} md={3}>
        <Paper sx={{ p: 2 }} data-testid="dash-reports-card">
          <Typography variant="h6">Reports</Typography>
          <Typography variant="h4" data-testid="dash-reports-count">
            {reports.data?.length ?? 0}
          </Typography>
        </Paper>
      </Grid>

      {/* Session progress strip + active planner objective (BL-36) */}
      <Grid item xs={12}>
        <Paper sx={{ p: 2 }} data-testid="dash-session-strip">
          <Stack direction="row" alignItems="center" spacing={1} mb={1}>
            <Typography variant="subtitle2" color="text.secondary">Sessions</Typography>
            <Chip
              data-testid="dash-objective-chip"
              size="small"
              variant="outlined"
              clickable
              component={RouterLink}
              to="/planner"
              label={`Objective: ${plan.data?.objective ? OBJECTIVE_LABELS[plan.data.objective] : "—"}`}
            />
          </Stack>
          <SessionProgressBoard
            variant="condensed"
            requests={requests.data ?? []}
            plan={plan.data ?? undefined}
            sim={sim.data ?? undefined}
          />
        </Paper>
      </Grid>

      <Grid item xs={12} md={3}>
        <Paper sx={{ p: 2 }} data-testid="dash-sensor-card">
          <Typography variant="h6">
            Latest Sensor
          </Typography>
          <Stack spacing={0.5} mt={1}>
            <Typography data-testid="dash-sensor-power">
              Power (kW): {sensor.data?.power_kw ?? "—"}
            </Typography>
            <Typography data-testid="dash-sensor-temp">
              Temp (C): {sensor.data?.temperature_c ?? "—"}
            </Typography>
            <Typography data-testid="dash-sensor-voltage">
              Voltage (V): {sensor.data?.voltage_v ?? "—"}
            </Typography>
          </Stack>
        </Paper>
      </Grid>

      <Grid item xs={12} md={3}>
        <CapacityCard capacity={capacity.data} netPowerKw={netPowerKw} />
      </Grid>

      {/* Simulation card */}
      <Grid item xs={12} md={6}>
        <Paper sx={{ p: 2 }} data-testid="dash-sim-card">
          <Stack direction="row" alignItems="center" spacing={1} mb={1}>
            <Typography variant="h6">Simulation</Typography>
            {sim.data && (
              <ModeBadge mode={assetsOfType(sim.data, "pv").length > 0 ? "active" : undefined} />
            )}
          </Stack>
          {sim.isError ? (
            <Typography color="text.secondary">Simulator not available</Typography>
          ) : sim.data ? (
            <Grid container spacing={1}>
              <Grid item xs={6}>
                <Stack spacing={0.5}>
                  <Typography variant="subtitle2">Power</Typography>
                  <Typography data-testid="sim-net-power">
                    Net: {formatKwOrDash(sim.data.grid.net_power_kw)}
                  </Typography>
                  <Typography data-testid="sim-import">
                    Import: {formatKwOrDash(Math.max(0, sim.data.grid.net_power_kw))}
                  </Typography>
                  <Typography data-testid="sim-export">
                    Export: {formatKwOrDash(Math.max(0, -sim.data.grid.net_power_kw))}
                  </Typography>
                </Stack>
              </Grid>
              <Grid item xs={6}>
                <Stack spacing={0.5}>
                  <Typography variant="subtitle2">Energy</Typography>
                  <Typography data-testid="sim-import-kwh">
                    Import: {formatFixedOrDash(sim.data.grid.import_kwh, 3)} kWh
                  </Typography>
                  <Typography data-testid="sim-export-kwh">
                    Export: {formatFixedOrDash(sim.data.grid.export_kwh, 3)} kWh
                  </Typography>
                </Stack>
              </Grid>
              <SimAssetTiles assets={sim.data.assets} />
            </Grid>
          ) : (
            <Typography color="text.secondary">Loading...</Typography>
          )}
        </Paper>
      </Grid>

      {/* Energy Ledger */}
      <Grid item xs={12} md={6}>
        <LedgerCard entries={ledger.data ?? []} histories={ledgerHistories} />
      </Grid>
    </Grid>
  );
}
