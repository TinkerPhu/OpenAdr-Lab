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
      }),
    );
    renderWeather();
    expect(screen.getByTestId("weather-source-alive")).toHaveTextContent("Source: Offline");
  });
});
