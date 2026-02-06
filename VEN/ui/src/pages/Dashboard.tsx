import { Grid, Paper, Stack, Typography } from "@mui/material";
import { Event, Program, SensorSnapshot } from "../api/types";

export function DashboardPage(props: {
  programs: Program[];
  events: Event[];
  sensor: SensorSnapshot | null;
  health: "ok" | "offline" | "unknown";
}) {
  const byStatus: Record<string, number> = {};
  props.events.forEach(e => {
    const s = e.status ?? "unknown";
    byStatus[s] = (byStatus[s] ?? 0) + 1;
  });

  return (
    <Grid container spacing={2}>
      <Grid item xs={12} md={4}>
        <Paper sx={{ p: 2 }}>
          <Typography variant="h6">Health</Typography>
          <Typography>{props.health}</Typography>
        </Paper>
      </Grid>

      <Grid item xs={12} md={4}>
        <Paper sx={{ p: 2 }}>
          <Typography variant="h6">Programs</Typography>
          <Typography variant="h4">{props.programs.length}</Typography>
          <Stack spacing={0.5} mt={1}>
            {props.programs.slice(0, 3).map(p => (
              <Typography key={p.id} variant="body2">
                {p.name ?? p.id}
              </Typography>
            ))}
          </Stack>
        </Paper>
      </Grid>

      <Grid item xs={12} md={4}>
        <Paper sx={{ p: 2 }}>
          <Typography variant="h6">Events</Typography>
          <Typography variant="h4">{props.events.length}</Typography>
          <Stack spacing={0.5} mt={1}>
            {Object.entries(byStatus).slice(0, 3).map(([k, v]) => (
              <Typography key={k} variant="body2">{k}: {v}</Typography>
            ))}
          </Stack>
        </Paper>
      </Grid>

      <Grid item xs={12}>
        <Paper sx={{ p: 2 }}>
          <Typography variant="h6">Latest sensor</Typography>
          <Typography variant="body2" color="text.secondary">{props.sensor?.ts ?? "—"}</Typography>
          <Stack direction={{ xs: "column", sm: "row" }} spacing={2} mt={1}>
            <Typography>Power (W): {props.sensor?.power_w ?? "—"}</Typography>
            <Typography>Temp (°C): {props.sensor?.temperature_c ?? "—"}</Typography>
            <Typography>Voltage (V): {props.sensor?.voltage_v ?? "—"}</Typography>
          </Stack>
        </Paper>
      </Grid>
    </Grid>
  );
}
