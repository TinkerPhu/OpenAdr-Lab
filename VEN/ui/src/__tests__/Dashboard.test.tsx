import { render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { BrowserRouter } from "react-router-dom";
import { describe, it, expect, vi } from "vitest";
import { DashboardPage } from "../pages/Dashboard";

const mockPrograms = [
  { id: "p1", programName: "Program Alpha" },
  { id: "p2", programName: "Program Beta" },
];

const mockEvents = [
  { id: "e1", programID: "p1", eventName: "ev-1", createdDateTime: "2024-01-01" },
  { id: "e2", programID: "p1", eventName: "ev-2", createdDateTime: "2024-01-02" },
];

const mockSensor = {
  id: "s1",
  ts: "2024-01-01T00:00:00Z",
  temperature_c: 22.5,
  power_w: 150,
  voltage_v: 230,
  raw: {},
};

const mockCapacity = {
  import_limit_kw: 10.0,
  export_limit_kw: 5.0,
  import_subscription_kw: 8.0,
  import_reservation_kw: 2.0,
  import_limit_event_id: "evt-1",
  export_limit_event_id: null,
  last_updated: "2024-01-01T10:00:00Z",
};

const mockLedger = [
  { asset_id: "ev", energy_kwh: 1.234, cost_eur: 0.2468, co2_g: 123.4, updated_at: "2024-01-01T11:00:00Z", started_at: "2024-01-01T10:00:00Z" },
  { asset_id: "battery", energy_kwh: 0.5, cost_eur: 0.1, co2_g: 50.0, updated_at: "2024-01-01T11:00:00Z", started_at: "2024-01-01T10:00:00Z" },
];

vi.mock("../api/hooks", () => ({
  useSignals: () => ({ data: undefined }),
  useHealth: vi.fn(() => ({ data: "ok", isError: false })),
  usePrograms: vi.fn(() => ({ data: mockPrograms })),
  useEvents: vi.fn(() => ({ data: mockEvents })),
  useSensor: vi.fn(() => ({ data: mockSensor })),
  useReports: vi.fn(() => ({ data: [] })),
  useSim: vi.fn(() => ({ data: null, isError: false })),
  useCapacity: vi.fn(() => ({ data: mockCapacity })),
  useLedger: vi.fn(() => ({ data: mockLedger })),
}));

function renderDashboard() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <BrowserRouter>
        <DashboardPage />
      </BrowserRouter>
    </QueryClientProvider>,
  );
}

describe("DashboardPage", () => {
  it("renders health card with status", () => {
    renderDashboard();
    expect(screen.getByTestId("dash-health-card")).toBeVisible();
    expect(screen.getByTestId("dash-health-value")).toHaveTextContent("ok");
  });

  it("renders programs card with count", () => {
    renderDashboard();
    expect(screen.getByTestId("dash-programs-card")).toBeVisible();
    expect(screen.getByTestId("dash-programs-count")).toHaveTextContent("2");
  });

  it("renders events card with count", () => {
    renderDashboard();
    expect(screen.getByTestId("dash-events-card")).toBeVisible();
    expect(screen.getByTestId("dash-events-count")).toHaveTextContent("2");
  });

  it("renders sensor card with values", () => {
    renderDashboard();
    expect(screen.getByTestId("dash-sensor-card")).toBeVisible();
    expect(screen.getByTestId("dash-sensor-power")).toHaveTextContent("150");
    expect(screen.getByTestId("dash-sensor-temp")).toHaveTextContent("22.5");
    expect(screen.getByTestId("dash-sensor-voltage")).toHaveTextContent("230");
  });

  it("renders capacity card with import/export limits", () => {
    renderDashboard();
    expect(screen.getByTestId("dash-capacity-card")).toBeVisible();
    expect(screen.getByTestId("dash-capacity-card")).toHaveTextContent("10.0 kW");
    expect(screen.getByTestId("dash-capacity-card")).toHaveTextContent("5.0 kW");
  });

  it("renders ledger card with asset rows and running-since header", () => {
    renderDashboard();
    expect(screen.getByTestId("dash-ledger-card")).toBeVisible();
    expect(screen.getByTestId("dash-ledger-since")).toHaveTextContent("running since");
    expect(screen.getByTestId("dash-ledger-card")).toHaveTextContent("ev");
    expect(screen.getByTestId("dash-ledger-card")).toHaveTextContent("battery");
    expect(screen.getByTestId("dash-ledger-card")).toHaveTextContent("1.234");
  });
});
