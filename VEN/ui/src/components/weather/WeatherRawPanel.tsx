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
import type { WeatherForecast, SkyCondition } from "../../api/types";

function skyLabel(sky: SkyCondition | null): string {
  if (sky === null) return "—";
  return sky
    .split("_")
    .map((w) => w[0].toUpperCase() + w.slice(1))
    .join(" ");
}

/** Raw forecast as received from the configured weather source — one row
 * per hour, up to the full available horizon (currently up to 48h). */
export function WeatherRawPanel({ forecast }: { forecast: WeatherForecast }) {
  return (
    <Paper variant="outlined" sx={{ p: 2 }} data-testid="weather-raw-panel">
      <Typography variant="subtitle1" sx={{ mb: 1 }}>
        Received forecast — {forecast.source_id}
      </Typography>
      <Typography variant="body2" color="text.secondary" sx={{ mb: 1 }}>
        Fetched {new Date(forecast.fetched_at).toLocaleString()} · {forecast.location.latitude_deg.toFixed(4)},{" "}
        {forecast.location.longitude_deg.toFixed(4)}
      </Typography>
      <TableContainer sx={{ maxHeight: 480 }}>
        <Table size="small" stickyHeader>
          <TableHead>
            <TableRow>
              <TableCell>Time</TableCell>
              <TableCell align="right">Temp (°C)</TableCell>
              <TableCell align="right">GHI (W/m²)</TableCell>
              <TableCell>Sky</TableCell>
              <TableCell align="right">Variability</TableCell>
            </TableRow>
          </TableHead>
          <TableBody>
            {forecast.samples.map((s) => (
              <TableRow key={s.valid_at}>
                <TableCell>{new Date(s.valid_at).toLocaleString()}</TableCell>
                <TableCell align="right">{s.temperature_c.toFixed(1)}</TableCell>
                <TableCell align="right">{s.ghi_w_m2.toFixed(0)}</TableCell>
                <TableCell>
                  {s.sky_condition ? <Chip size="small" label={skyLabel(s.sky_condition)} /> : "—"}
                </TableCell>
                <TableCell align="right">
                  {s.irradiance_variability !== null ? s.irradiance_variability.toFixed(2) : "—"}
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </TableContainer>
    </Paper>
  );
}
