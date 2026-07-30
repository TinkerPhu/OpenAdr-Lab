import { Alert, Chip, Stack, Typography } from "@mui/material";
import { useWeather } from "../api/hooks";
import { WeatherRawPanel } from "../components/weather/WeatherRawPanel";
import { WeatherDerivedPanel } from "../components/weather/WeatherDerivedPanel";

export function WeatherPage() {
  const { data, isLoading } = useWeather();

  return (
    <div data-testid="weather-page">
      <Stack direction="row" spacing={2} alignItems="center" sx={{ mb: 2 }}>
        <Typography variant="h5">Weather</Typography>
        {!isLoading && data && (
          <Chip
            size="small"
            data-testid="weather-source-alive"
            label={data.source_alive ? "Source: Live" : "Source: Offline"}
            color={data.source_alive ? "success" : "default"}
          />
        )}
      </Stack>

      {isLoading && <Typography color="text.secondary">Loading…</Typography>}

      {!isLoading && (!data || data.status === "no_forecast") && (
        <Alert severity="info" data-testid="weather-no-forecast">
          No weather feed configured for this VEN — nothing has been received yet.
        </Alert>
      )}

      {!isLoading && data && data.status !== "no_forecast" && data.raw && (
        <Stack spacing={2}>
          {data.status === "stale" && (
            <Alert severity="warning" data-testid="weather-stale-alert">
              The last received forecast is stale — showing it anyway, but it may not reflect
              current conditions.
            </Alert>
          )}
          <WeatherRawPanel forecast={data.raw} />
          {data.derived ? (
            <WeatherDerivedPanel slots={data.derived} />
          ) : (
            <Alert severity="info" data-testid="weather-derived-unavailable">
              No PV array configured for this site (<code>weather_pv</code> profile section) —
              derived PV forecast unavailable.
            </Alert>
          )}
        </Stack>
      )}
    </div>
  );
}
