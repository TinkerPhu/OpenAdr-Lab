import {
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
export function WeatherDerivedPanel({ slots }: { slots: WeatherPvForecastSlot[] }) {
  return (
    <Paper variant="outlined" sx={{ p: 2 }} data-testid="weather-derived-panel">
      <Typography variant="subtitle1" sx={{ mb: 1 }}>
        Derived PV forecast
      </Typography>
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
