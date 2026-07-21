import { render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { BrowserRouter } from "react-router-dom";
import { describe, it, expect, vi } from "vitest";
import { WeatherPage } from "../pages/Weather";
import { useWeather } from "../api/hooks";

vi.mock("../api/hooks", () => ({
  useWeather: vi.fn(),
}));

const mockRawForecast = {
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

const mockDerivedSlots = [
  { valid_at: "2026-07-19T06:00:00Z", forecast_ac_kw: 0.42, snow_covered: false },
];

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
    vi.mocked(useWeather).mockReturnValue({
      data: { status: "no_forecast", is_fresh: false, raw: null, derived: null },
      isLoading: false,
    } as ReturnType<typeof useWeather>);
    renderWeather();
    expect(screen.getByTestId("weather-no-forecast")).toBeVisible();
    expect(screen.queryByTestId("weather-raw-panel")).not.toBeInTheDocument();
  });

  it("shows raw and derived panels when both are available and fresh", () => {
    vi.mocked(useWeather).mockReturnValue({
      data: { status: "ok", is_fresh: true, raw: mockRawForecast, derived: mockDerivedSlots },
      isLoading: false,
    } as ReturnType<typeof useWeather>);
    renderWeather();
    expect(screen.getByTestId("weather-raw-panel")).toBeVisible();
    expect(screen.getByTestId("weather-derived-panel")).toBeVisible();
    expect(screen.queryByTestId("weather-stale-alert")).not.toBeInTheDocument();
  });

  it("shows a stale warning without hiding the raw forecast", () => {
    vi.mocked(useWeather).mockReturnValue({
      data: { status: "stale", is_fresh: false, raw: mockRawForecast, derived: null },
      isLoading: false,
    } as ReturnType<typeof useWeather>);
    renderWeather();
    expect(screen.getByTestId("weather-stale-alert")).toBeVisible();
    expect(screen.getByTestId("weather-raw-panel")).toBeVisible();
  });

  it("shows a derived-unavailable message when raw is present but no PV config exists", () => {
    vi.mocked(useWeather).mockReturnValue({
      data: { status: "ok", is_fresh: true, raw: mockRawForecast, derived: null },
      isLoading: false,
    } as ReturnType<typeof useWeather>);
    renderWeather();
    expect(screen.getByTestId("weather-raw-panel")).toBeVisible();
    expect(screen.getByTestId("weather-derived-unavailable")).toBeVisible();
    expect(screen.queryByTestId("weather-derived-panel")).not.toBeInTheDocument();
  });
});
