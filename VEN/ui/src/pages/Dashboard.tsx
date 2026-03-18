import { Chip, Grid, Paper, Stack, Typography } from "@mui/material";
import { useHealth, usePrograms, useEvents, useSensor, useReports, useSim } from "../api/hooks";

function fmtNum(v: number | undefined | null, decimals = 1): string {
  if (v == null) return "—";
  return v.toFixed(decimals);
}

function ModeBadge({ mode }: { mode?: string }) {
  if (!mode || mode === "IDLE") return <Chip label="IDLE" size="small" />;
  const color = mode === "EXPORT_CAP" ? "warning" : mode === "IMPORT_CAP" ? "info" : mode === "PRICE" ? "secondary" : "default";
  return <Chip label={mode} size="small" color={color} />;
}

export function DashboardPage() {
  const health = useHealth();
  const programs = usePrograms();
  const events = useEvents();
  const sensor = useSensor();
  const reports = useReports();
  const sim = useSim();

  const healthStatus = health.isError ? "offline" : health.data ? "ok" : "unknown";

  return (
    <Grid container spacing={2}>
      <Grid item xs={12} md={3}>
        <Paper sx={{ p: 2 }} data-testid="dash-health-card">
          <Typography variant="h6">
            Health
          </Typography>
          <Typography data-testid="dash-health-value">{healthStatus}</Typography>
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

      <Grid item xs={12} md={3}>
        <Paper sx={{ p: 2 }} data-testid="dash-sensor-card">
          <Typography variant="h6">
            Latest Sensor
          </Typography>
          <Stack spacing={0.5} mt={1}>
            <Typography data-testid="dash-sensor-power">
              Power (W): {sensor.data?.power_w ?? "—"}
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

      {/* Simulation card */}
      <Grid item xs={12} md={6}>
        <Paper sx={{ p: 2 }} data-testid="dash-sim-card">
          <Stack direction="row" alignItems="center" spacing={1} mb={1}>
            <Typography variant="h6">Simulation</Typography>
            {sim.data && <ModeBadge mode={"pv" in sim.data.assets ? "active" : undefined} />}
          </Stack>
          {sim.isError ? (
            <Typography color="text.secondary">Simulator not available</Typography>
          ) : sim.data ? (
            <Grid container spacing={1}>
              <Grid item xs={6}>
                <Stack spacing={0.5}>
                  <Typography variant="subtitle2">Power</Typography>
                  <Typography data-testid="sim-net-power">
                    Net: {fmtNum(sim.data.grid.net_power_w, 0)} W
                  </Typography>
                  <Typography data-testid="sim-import">
                    Import: {fmtNum(sim.data.grid.net_power_w > 0 ? sim.data.grid.net_power_w : 0, 0)} W
                  </Typography>
                  <Typography data-testid="sim-export">
                    Export: {fmtNum(sim.data.grid.net_power_w < 0 ? -sim.data.grid.net_power_w : 0, 0)} W
                  </Typography>
                </Stack>
              </Grid>
              <Grid item xs={6}>
                <Stack spacing={0.5}>
                  <Typography variant="subtitle2">Energy</Typography>
                  <Typography data-testid="sim-import-kwh">
                    Import: {fmtNum(sim.data.grid.import_kwh, 3)} kWh
                  </Typography>
                  <Typography data-testid="sim-export-kwh">
                    Export: {fmtNum(sim.data.grid.export_kwh, 3)} kWh
                  </Typography>
                </Stack>
              </Grid>
              {"ev" in sim.data.assets && (
                <Grid item xs={4}>
                  <Stack spacing={0.5}>
                    <Typography variant="subtitle2">EV Charger</Typography>
                    <Typography>SOC: {((sim.data.assets["ev"].soc ?? 0) * 100).toFixed(1)}%</Typography>
                    <Typography>Power: {fmtNum(sim.data.assets["ev"].power_kw)} kW</Typography>
                    <Typography>Plugged: {(sim.data.assets["ev"].plugged ?? 0) !== 0 ? "Yes" : "No"}</Typography>
                  </Stack>
                </Grid>
              )}
              {"heater" in sim.data.assets && (
                <Grid item xs={4}>
                  <Stack spacing={0.5}>
                    <Typography variant="subtitle2">Heater</Typography>
                    <Typography>Temp: {fmtNum(sim.data.assets["heater"].temp_c)}°C</Typography>
                    <Typography>Power: {fmtNum(sim.data.assets["heater"].power_kw)} kW</Typography>
                  </Stack>
                </Grid>
              )}
              {"pv" in sim.data.assets && (
                <Grid item xs={4}>
                  <Stack spacing={0.5}>
                    <Typography variant="subtitle2">PV Inverter</Typography>
                    <Typography>Output: {fmtNum(sim.data.assets["pv"].power_kw)} kW</Typography>
                    <Typography>Irradiance: {((sim.data.assets["pv"].irradiance ?? 0) * 100).toFixed(0)}%</Typography>
                    <Typography>Export limit: {"export_limit_kw" in sim.data.assets["pv"] ? `${sim.data.assets["pv"].export_limit_kw.toFixed(1)} kW` : "none"}</Typography>
                  </Stack>
                </Grid>
              )}
            </Grid>
          ) : (
            <Typography color="text.secondary">Loading...</Typography>
          )}
        </Paper>
      </Grid>
    </Grid>
  );
}
