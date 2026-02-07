import { Grid, Paper, Stack, Typography } from "@mui/material";
import { useHealth, usePrograms, useEvents, useVens } from "../api/hooks";

export function DashboardPage() {
  const health = useHealth();
  const programs = usePrograms();
  const events = useEvents();
  const vens = useVens();

  const vtnOk = health.data?.vtn?.reachable && health.data?.vtn?.authOk;
  const healthStatus = health.isError ? "offline" : vtnOk ? "ok" : health.data ? "degraded" : "unknown";

  return (
    <Grid container spacing={2}>
      <Grid item xs={12} md={3}>
        <Paper sx={{ p: 2 }} data-testid="dash-health-card">
          <Typography variant="h6">VTN Health</Typography>
          <Typography data-testid="dash-health-value">{healthStatus}</Typography>
          {health.data && (
            <Stack spacing={0.5} mt={1}>
              <Typography variant="body2" data-testid="dash-health-reachable">
                Reachable: {health.data.vtn.reachable ? "yes" : "no"}
              </Typography>
              <Typography variant="body2" data-testid="dash-health-auth">
                Auth: {health.data.vtn.authOk ? "ok" : "failed"}
              </Typography>
            </Stack>
          )}
        </Paper>
      </Grid>

      <Grid item xs={12} md={3}>
        <Paper sx={{ p: 2 }} data-testid="dash-programs-card">
          <Typography variant="h6">Programs</Typography>
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
        <Paper sx={{ p: 2 }} data-testid="dash-vens-card">
          <Typography variant="h6">VENs</Typography>
          <Typography variant="h4" data-testid="dash-vens-count">
            {vens.data?.length ?? 0}
          </Typography>
          <Stack spacing={0.5} mt={1}>
            {vens.data?.slice(0, 3).map((v) => (
              <Typography key={v.id} variant="body2">
                {v.venName ?? v.id}
              </Typography>
            ))}
          </Stack>
        </Paper>
      </Grid>

      <Grid item xs={12} md={3}>
        <Paper sx={{ p: 2 }} data-testid="dash-events-card">
          <Typography variant="h6">Events</Typography>
          <Typography variant="h4" data-testid="dash-events-count">
            {events.data?.length ?? 0}
          </Typography>
        </Paper>
      </Grid>
    </Grid>
  );
}
