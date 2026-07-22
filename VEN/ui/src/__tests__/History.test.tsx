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
// WP-T6 (docs/history/project_journal.md, search "WP-T"): wires GET /history/plans.
const mockPlans = [
  {
    created_at: Date.UTC(2026, 0, 1, 9),
    horizon_start: "2026-01-01T09:00:00Z",
    horizon_end: "2026-01-02T09:00:00Z",
    plan_json: '{"slots":[]}',
  },
];

vi.mock("../api/hooks", () => ({
  useSignals: () => ({ data: undefined }),
  useHistoryTicks: () => ({ data: mockTicks }),
  useHistoryGrid: () => ({ data: mockGrid }),
  useHistoryEvents: () => ({ data: mockEvents }),
  useHistoryReports: () => ({ data: mockReports }),
  useHistoryPlans: () => ({ data: mockPlans }),
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

  it("renders a plans table with the mocked snapshot", () => {
    renderHistory();
    expect(screen.getByTestId(`history-plan-row-${mockPlans[0].created_at}`)).toBeInTheDocument();
  });

  it("opens a JSON dialog with the plan detail when View is clicked", () => {
    renderHistory();
    fireEvent.click(screen.getByTestId(`history-plan-view-${mockPlans[0].created_at}`));
    expect(screen.getByTestId("json-dialog")).toBeInTheDocument();
    expect(screen.getByTestId("json-dialog-content")).toHaveTextContent("slots");
  });
});
