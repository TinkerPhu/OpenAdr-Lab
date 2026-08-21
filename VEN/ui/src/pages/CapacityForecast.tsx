import { Paper, Stack, Typography } from "@mui/material";
import { useCapacityCurves } from "../api/hooks";
import { CapacityForecastChart } from "../components/controller/charts/CapacityForecastChart";

/** BL-flexibility-capacity-forecast: Diagnostics page for the
 * sustained-commitment power/duration/energy capacity curves — a distinct
 * signal from the instantaneous "Site Headroom" chart on the Dashboard, see
 * `openspec/changes/flexibility-capacity-forecast/design.md`. */
export function CapacityForecastPage() {
  const { data: curves = null, dataUpdatedAt } = useCapacityCurves();
  const lastUpdated = dataUpdatedAt ? new Date(dataUpdatedAt).toLocaleString() : "—";

  return (
    <Stack spacing={2}>
      <div>
        <Typography variant="h5" data-testid="capacity-forecast-heading">
          Capacity Forecast
        </Typography>
        <Typography variant="body2" color="text.secondary" data-testid="capacity-forecast-last-updated">
          Last updated: {lastUpdated} (auto-refresh 10s)
        </Typography>
        <Typography variant="body2" color="text.secondary" sx={{ mt: 1 }}>
          If the site committed now to sustained maximum import (or export), how does the
          achievable power step down over elapsed time, and how much energy is behind it — distinct
          from the instantaneous Site Headroom chart on the Dashboard, which only describes the
          current instant.
        </Typography>
      </div>

      <Paper sx={{ p: 2 }}>
        <CapacityForecastChart curves={curves} height={320} />
      </Paper>
    </Stack>
  );
}
