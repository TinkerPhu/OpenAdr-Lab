import {
  Alert,
  Chip,
  Paper,
  Table,
  TableBody,
  TableCell,
  TableContainer,
  TableHead,
  TableRow,
  Typography,
} from "@mui/material";
import type { WeatherPvForecastSlot } from "../../api/types";

/** Weather-sourced PV forecast derived from the raw forecast — read-only
 * diagnostic (not yet the planner's own PV input, see R-50). */
export function WeatherDerivedPanel({
  slots,
  snowCoveredNow,
}: {
  slots: WeatherPvForecastSlot[];
  snowCoveredNow: boolean;
}) {
  return (
    <Paper variant="outlined" sx={{ p: 2 }} data-testid="weather-derived-panel">
      <Typography variant="subtitle1" sx={{ mb: 1 }}>
        Derived PV forecast
      </Typography>
      {snowCoveredNow && (
        <Alert severity="info" sx={{ mb: 1 }} data-testid="weather-pv-snow-now">
          The PV panels are covered with snow right now (the array delivers next to nothing under
          the predicted sun), so the forecast keeps them covered until the air warms past the melt
          threshold.
        </Alert>
      )}
      <TableContainer sx={{ maxHeight: 480 }}>
        <Table size="small" stickyHeader>
          <TableHead>
            <TableRow>
              <TableCell>Time</TableCell>
              <TableCell align="right">Forecast (kW)</TableCell>
              <TableCell>Snow</TableCell>
            </TableRow>
          </TableHead>
          <TableBody>
            {slots.map((slot) => (
              <TableRow key={slot.valid_at}>
                <TableCell>{new Date(slot.valid_at).toLocaleString()}</TableCell>
                <TableCell align="right">{slot.forecast_ac_kw.toFixed(2)}</TableCell>
                <TableCell>
                  {slot.snow_covered ? <Chip size="small" color="info" label="Covered" /> : "—"}
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </TableContainer>
    </Paper>
  );
}
