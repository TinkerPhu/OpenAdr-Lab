import { Grid, Paper, Stack, Typography } from "@mui/material";
import { useHealth, usePrograms, useEvents, useSensor, useReports } from "../api/hooks";

export function DashboardPage() {
  const health = useHealth();
  const programs = usePrograms();
  const events = useEvents();
  const sensor = useSensor();
  const reports = useReports();

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
    </Grid>
  );
}
