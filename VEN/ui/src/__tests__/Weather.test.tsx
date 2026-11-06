import { render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { BrowserRouter } from "react-router-dom";
import { describe, it, expect, vi } from "vitest";
import { WeatherPage } from "../pages/Weather";
import { useWeather } from "../api/hooks";
import type { WeatherForecast, WeatherPvForecastSlot, WeatherResponse } from "../api/types";

vi.mock("../api/hooks", () => ({
  useWeather: vi.fn(),
}));

const mockRawForecast: WeatherForecast = {
  source_id: "srf_meteo",
  location: { latitude_deg: 47.4491, longitude_deg: 7.8081 },
  fetched_at: "2026-07-19T05:54:48Z",
  samples: [
    {
      valid_at: "2026-07-19T06:00:00Z",
      age_h: 1,
      temperature_c: 16.0,
      ghi_w_m2: 97.0,
      wind_speed_kmh: 4.0,
      rain_prob_pct: 14.0,
      new_snowfall_cm: null,
      sky_condition: "partly_cloudy",
      irradiance_variability: 0.6,
    },
  ],
};

const mockDerivedSlots: WeatherPvForecastSlot[] = [
  {
    valid_at: "2026-07-19T06:00:00Z",
    forecast_ac_kw: 0.42,
    snow_covered: false,
    snow_free_ac_kw: 0.42,
    snow_possible: false,
  },
];

/** A loaded `useWeather` result whose data is exactly the wire type. */
function weatherResult(data: WeatherResponse): ReturnType<typeof useWeather> {
  return { data, isLoading: false } as ReturnType<typeof useWeather>;
}

function renderWeather() {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={queryClient}>
      <BrowserRouter>
        <WeatherPage />
      </BrowserRouter>
    </QueryClientProvider>,
  );
}

describe("WeatherPage", () => {
  it("shows a no-forecast empty state when no weather feed is configured", () => {
    vi.mocked(useWeather).mockReturnValue(
      weatherResult({
        status: "no_forecast",
        is_fresh: false,
        source_alive: false,
        raw: null,
        derived: {},
        pv_snow_covered_now: false,
        forecast_sources: {},
      }),
    );
    renderWeather();
    expect(screen.getByTestId("weather-no-forecast")).toBeVisible();
    expect(screen.queryByTestId("weather-raw-panel")).not.toBeInTheDocument();
  });

  it("shows raw and derived panels when both are available and fresh", () => {
    vi.mocked(useWeather).mockReturnValue(
      weatherResult({
        status: "ok",
        is_fresh: true,
        source_alive: true,
        raw: mockRawForecast,
        derived: { pv: mockDerivedSlots },
        pv_snow_covered_now: false,
        forecast_sources: {},
      }),
    );
    renderWeather();
    expect(screen.getByTestId("weather-raw-panel")).toBeVisible();
    expect(screen.getByTestId("weather-derived-panel")).toBeVisible();
    expect(screen.queryByTestId("weather-stale-alert")).not.toBeInTheDocument();
    expect(screen.queryByTestId("weather-pv-snow-now")).not.toBeInTheDocument();
  });

  it("says so when the PV asset concludes its panels are covered with snow right now", () => {
    vi.mocked(useWeather).mockReturnValue(
      weatherResult({
        status: "ok",
        is_fresh: true,
        source_alive: true,
        raw: mockRawForecast,
        derived: { pv: [{ ...mockDerivedSlots[0], snow_covered: true, forecast_ac_kw: 0 }] },
        pv_snow_covered_now: true,
        forecast_sources: {},
      }),
    );
    renderWeather();
    expect(screen.getByTestId("weather-pv-snow-now")).toHaveTextContent(/snow/i);
  });

  it("shows a stale warning without hiding the raw forecast", () => {
    vi.mocked(useWeather).mockReturnValue(
      weatherResult({
        status: "stale",
        is_fresh: false,
        source_alive: true,
        raw: mockRawForecast,
        derived: {},
        pv_snow_covered_now: false,
        forecast_sources: {},
      }),
    );
    renderWeather();
    expect(screen.getByTestId("weather-stale-alert")).toBeVisible();
    expect(screen.getByTestId("weather-raw-panel")).toBeVisible();
  });

  it("shows a derived-unavailable message when raw is present but no PV config exists", () => {
    vi.mocked(useWeather).mockReturnValue(
      weatherResult({
        status: "ok",
        is_fresh: true,
        source_alive: true,
        raw: mockRawForecast,
        derived: {},
        pv_snow_covered_now: false,
        forecast_sources: {},
      }),
    );
    renderWeather();
    expect(screen.getByTestId("weather-raw-panel")).toBeVisible();
    expect(screen.getByTestId("weather-derived-unavailable")).toBeVisible();
    expect(screen.queryByTestId("weather-derived-panel")).not.toBeInTheDocument();
  });

  it("shows a live source chip when source_alive is true", () => {
    vi.mocked(useWeather).mockReturnValue(
      weatherResult({
        status: "ok",
        is_fresh: true,
        source_alive: true,
        raw: mockRawForecast,
        derived: { pv: mockDerivedSlots },
        pv_snow_covered_now: false,
        forecast_sources: {},
      }),
    );
    renderWeather();
    expect(screen.getByTestId("weather-source-alive")).toHaveTextContent("Source: Live");
  });

  it("shows an offline source chip when source_alive is false, independent of forecast freshness", () => {
    vi.mocked(useWeather).mockReturnValue(
      weatherResult({
        status: "ok",
        is_fresh: true,
        source_alive: false,
        raw: mockRawForecast,
        derived: { pv: mockDerivedSlots },
        pv_snow_covered_now: false,
        forecast_sources: {},
      }),
    );
    renderWeather();
    expect(screen.getByTestId("weather-source-alive")).toHaveTextContent("Source: Offline");
  });

  it("shows the forecaster as the source in use, with its model and the age of the newest message", () => {
    vi.mocked(useWeather).mockReturnValue(
      weatherResult({
        status: "ok",
        is_fresh: true,
        source_alive: true,
        raw: mockRawForecast,
        derived: { pv: mockDerivedSlots },
        pv_snow_covered_now: false,
        forecast_sources: {
          "pv/pv": {
            source: "forecaster",
            model_version: "pv-weather-1",
            age_s: 720,
            valid_until: "2026-07-19T07:30:00Z",
            rejected_reason: null,
            rejected_at: null,
          },
          base_load: {
            source: "in_process",
            model_version: null,
            age_s: null,
            valid_until: null,
            rejected_reason: null,
            rejected_at: null,
          },
        },
      }),
    );
    renderWeather();
    expect(screen.getByTestId("forecast-source-kind-pv/pv")).toHaveTextContent("Forecaster");
    expect(screen.getByTestId("forecast-source-age-pv/pv")).toHaveTextContent("12 min");
    expect(screen.getByTestId("forecast-source-pv/pv")).toHaveTextContent("pv-weather-1");
    expect(screen.getByTestId("forecast-source-kind-base_load")).toHaveTextContent("In-process");
    expect(screen.queryByTestId("forecast-source-rejected-pv/pv")).toBeNull();
  });

  it("shows why the newest forecaster message was refused and that the VEN computes its own", () => {
    vi.mocked(useWeather).mockReturnValue(
      weatherResult({
        status: "no_forecast",
        is_fresh: false,
        source_alive: false,
        raw: null,
        derived: {},
        pv_snow_covered_now: false,
        forecast_sources: {
          "pv/pv": {
            source: "in_process",
            model_version: null,
            age_s: null,
            valid_until: null,
            rejected_reason: "unsupported schema_version: 2.0.0",
            rejected_at: "2026-07-19T06:00:00Z",
          },
        },
      }),
    );
    renderWeather();
    expect(screen.getByTestId("forecast-source-kind-pv/pv")).toHaveTextContent("In-process");
    expect(screen.getByTestId("forecast-source-rejected-pv/pv")).toHaveTextContent(
      "unsupported schema_version: 2.0.0",
    );
  });
});
