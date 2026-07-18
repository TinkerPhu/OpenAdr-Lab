import { render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { BrowserRouter } from "react-router-dom";
import { describe, it, expect, vi } from "vitest";
import { DashboardPage } from "../pages/Dashboard";
import { useHealth } from "../api/hooks";

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

const mockActiveRequest = {
  id: "req-ev-01",
  asset_id: "ev",
  target_energy_kwh: 10,
  target_soc: 0.9,
  desired_power_kw: 7.4,
  completion_policy: "STOP",
  deadlines: [{ latest_end: new Date(Date.now() + 3_600_000).toISOString(), max_total_cost_eur: null, max_marginal_rate_eur_kwh: null, min_completion: 1.0 }],
  mode: "BY_DEADLINE",
  max_total_cost_eur: null,
  tier_count: 1,
  session_id: "sess-ev-01",
  session_type: "ev",
  status: "ACTIVE",
  estimated_cost_eur: 1.8,
  estimated_co2_g: 300,
  interruptible: true,
  tolerance_min: null,
  budget_eur: null,
  created_at: "2024-01-01T08:00:00Z",
  updated_at: "2024-01-01T10:00:00Z",
  session: {
    type: "ev", id: "sess-ev-01", target_soc: 0.9,
    departure_time: new Date(Date.now() + 3_600_000).toISOString(),
    soft_deadline: false, mode: "BY_DEADLINE", budget_eur: null,
    created_at: "2024-01-01T08:00:00Z", updated_at: "2024-01-01T10:00:00Z",
  },
};

vi.mock("../api/hooks", () => ({
  useSignals: () => ({ data: undefined }),
  useHealth: vi.fn(() => ({
    data: {
      status: "ok",
      components: {
        ven_process: { status: "ok" },
        vtn_connection: { status: "ok" },
        storage: { status: "ok" },
        planner: { status: "ok" },
      },
    },
    isError: false,
  })),
  usePrograms: vi.fn(() => ({ data: mockPrograms })),
  useEvents: vi.fn(() => ({ data: mockEvents })),
  useSensor: vi.fn(() => ({ data: mockSensor })),
  useReports: vi.fn(() => ({ data: [] })),
  useSim: vi.fn(() => ({ data: null, isError: false })),
  useCapacity: vi.fn(() => ({ data: mockCapacity })),
  useLedger: vi.fn(() => ({ data: mockLedger })),
  usePlan: vi.fn(() => ({ data: { objective: "min_cost" } })),
  useRequests: vi.fn(() => ({ data: [mockActiveRequest] })),
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

  // Regression test: the Dashboard's health card previously did a truthy
  // check on the response body instead of reading `.status`, so it always
  // showed "ok" once /health returned any successful (always-truthy) object.
  it("renders degraded status when a health component is degraded", () => {
    vi.mocked(useHealth).mockReturnValueOnce({
      data: {
        status: "degraded",
        components: {
          ven_process: { status: "ok" },
          vtn_connection: { status: "degraded", detail: "backoff 30.0s" },
          storage: { status: "ok" },
          planner: { status: "ok" },
        },
      },
      isError: false,
    } as ReturnType<typeof useHealth>);
    renderDashboard();
    expect(screen.getByTestId("dash-health-value")).toHaveTextContent("degraded");
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

  it("renders session strip with objective chip and one chip per active session (BL-36)", () => {
    renderDashboard();
    expect(screen.getByTestId("dash-session-strip")).toBeVisible();
    expect(screen.getByTestId("dash-objective-chip")).toHaveTextContent("Objective: Cost");
    expect(screen.getByTestId("session-chip-req-ev-01")).toBeVisible();
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
