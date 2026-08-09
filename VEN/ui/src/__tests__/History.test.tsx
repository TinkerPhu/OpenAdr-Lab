import { render, screen, fireEvent } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { BrowserRouter } from "react-router-dom";
import { describe, it, expect, vi } from "vitest";
import { HistoryPage, dayRangeIso } from "../pages/History";

const mockTicks = [
  { ts: Date.UTC(2026, 0, 1, 6), asset_id: "ev", power_kw: 3.5, soc_pct: 42, temperature_c: null },
  { ts: Date.UTC(2026, 0, 1, 7), asset_id: "heater", power_kw: 1.2, soc_pct: null, temperature_c: 55 },
];
const mockGrid = [
  {
    ts: Date.UTC(2026, 0, 1, 6),
    import_kw: 2.0,
    export_kw: 0.0,
    import_tariff_eur_kwh: 0.25,
    export_tariff_eur_kwh: 0.05,
    co2_g_kwh: 300,
  },
];
const mockEvents = [
  { received_at: Date.UTC(2026, 0, 1, 5), event_id: "evt-1", event_type: "PRICE", payload_json: "{}" },
];
const mockReports = [
  { sent_at: Date.UTC(2026, 0, 1, 8), report_type: "USAGE", event_id: "evt-1", payload_json: "{}" },
];

const mockRefetch = vi.hoisted(() => ({
  ticks: vi.fn(),
  grid: vi.fn(),
  events: vi.fn(),
  reports: vi.fn(),
  forecastAccuracy: vi.fn(),
}));

vi.mock("../api/hooks", () => ({
  useSignals: () => ({ data: undefined }),
  useHistoryTicks: () => ({ data: mockTicks, refetch: mockRefetch.ticks }),
  useHistoryGrid: () => ({ data: mockGrid, refetch: mockRefetch.grid }),
  useHistoryEvents: () => ({ data: mockEvents, refetch: mockRefetch.events }),
  useHistoryReports: () => ({ data: mockReports, refetch: mockRefetch.reports }),
  useHistoryForecastAccuracy: () => ({ data: [], refetch: mockRefetch.forecastAccuracy }),
}));

vi.mock("../App", () => ({
  useVenContext: () => ({ venUrl: "http://localhost:8081", venName: "ven-1", setVenUrl: vi.fn(), api: {} }),
}));

function renderHistory() {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={queryClient}>
      <BrowserRouter>
        <HistoryPage />
      </BrowserRouter>
    </QueryClientProvider>,
  );
}

describe("dayRangeIso", () => {
  it("returns a 24h [from, to) window for a UTC calendar day", () => {
    const { fromIso, toIso } = dayRangeIso("2026-01-01");
    expect(fromIso).toBe("2026-01-01T00:00:00.000Z");
    expect(toIso).toBe("2026-01-02T00:00:00.000Z");
  });
});

describe("HistoryPage", () => {
  it("renders one chart section per asset present in the ticks data", () => {
    renderHistory();
    expect(screen.getByTestId("history-asset-chart-ev")).toBeInTheDocument();
    expect(screen.getByTestId("history-asset-chart-heater")).toBeInTheDocument();
  });

  it("renders events and reports tables with the mocked rows", () => {
    renderHistory();
    expect(screen.getByTestId("history-event-row-evt-1")).toBeInTheDocument();
    expect(screen.getByTestId("history-report-row-evt-1")).toBeInTheDocument();
  });

  it("updates the selected date when the date input changes", () => {
    renderHistory();
    const input = screen.getByTestId("history-date-input") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "2026-02-15" } });
    expect(input.value).toBe("2026-02-15");
  });

  it("defaults the date field to today's UTC date while in rolling last-24h mode", () => {
    renderHistory();
    const input = screen.getByTestId("history-date-input") as HTMLInputElement;
    const todayUtc = new Date().toISOString().slice(0, 10);
    expect(input.value).toBe(todayUtc);
    expect(screen.getByTestId("history-last-24h-btn")).not.toBeDisabled();
  });

  it("returns to rolling last-24h mode when 'Last 24h' is clicked", () => {
    renderHistory();
    const input = screen.getByTestId("history-date-input") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "2026-02-15" } });
    expect(input.value).toBe("2026-02-15");

    const button = screen.getByTestId("history-last-24h-btn");
    fireEvent.click(button);

    const todayUtc = new Date().toISOString().slice(0, 10);
    expect(input.value).toBe(todayUtc);
    expect(button).not.toBeDisabled();
  });

  it("clicking the date control switches out of rolling last-24h into the fixed day it's showing", () => {
    renderHistory();
    expect(
      screen.getByText("Showing the last 24 hours — pick a date above to view a specific UTC day")
    ).toBeInTheDocument();

    const input = screen.getByTestId("history-date-input") as HTMLInputElement;
    const todayUtc = new Date().toISOString().slice(0, 10);
    fireEvent.click(input);

    expect(screen.getByText(`Showing ${todayUtc} (UTC)`)).toBeInTheDocument();
  });

  it("refetches history data when the date control is clicked while already on that exact day", () => {
    renderHistory();
    const input = screen.getByTestId("history-date-input") as HTMLInputElement;
    // First click just switches out of rolling mode into today's fixed day — no manual
    // refetch needed there, since the query-key change already triggers one.
    fireEvent.click(input);

    mockRefetch.ticks.mockClear();
    mockRefetch.grid.mockClear();
    mockRefetch.events.mockClear();
    mockRefetch.reports.mockClear();
    mockRefetch.forecastAccuracy.mockClear();

    // Second click: already showing this exact day, so nothing about the selection changes —
    // this is the case that needs an explicit refetch.
    fireEvent.click(input);

    expect(mockRefetch.ticks).toHaveBeenCalledTimes(1);
    expect(mockRefetch.grid).toHaveBeenCalledTimes(1);
    expect(mockRefetch.events).toHaveBeenCalledTimes(1);
    expect(mockRefetch.reports).toHaveBeenCalledTimes(1);
    expect(mockRefetch.forecastAccuracy).toHaveBeenCalledTimes(3);
  });

  it("refetches history data when 'Last 24h' is clicked while already in that mode", () => {
    renderHistory();
    mockRefetch.ticks.mockClear();
    mockRefetch.grid.mockClear();
    mockRefetch.events.mockClear();
    mockRefetch.reports.mockClear();
    mockRefetch.forecastAccuracy.mockClear();

    fireEvent.click(screen.getByTestId("history-last-24h-btn"));

    expect(mockRefetch.ticks).toHaveBeenCalledTimes(1);
    expect(mockRefetch.grid).toHaveBeenCalledTimes(1);
    expect(mockRefetch.events).toHaveBeenCalledTimes(1);
    expect(mockRefetch.reports).toHaveBeenCalledTimes(1);
    expect(mockRefetch.forecastAccuracy).toHaveBeenCalledTimes(3);
  });
});
