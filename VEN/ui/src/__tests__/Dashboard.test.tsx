import { render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { BrowserRouter } from "react-router-dom";
import { describe, it, expect, vi } from "vitest";
import { DashboardPage } from "../pages/Dashboard";

const mockPrograms = [
  { id: "p1", name: "Program Alpha" },
  { id: "p2", name: "Program Beta" },
];

const mockEvents = [
  { id: "e1", program_id: "p1", status: "active", created_at: "2024-01-01", raw: {} },
  { id: "e2", program_id: "p1", status: "completed", created_at: "2024-01-02", raw: {} },
];

const mockSensor = {
  id: "s1",
  ts: "2024-01-01T00:00:00Z",
  temperature_c: 22.5,
  power_w: 150,
  voltage_v: 230,
  raw: {},
};

vi.mock("../api/hooks", () => ({
  useHealth: vi.fn(() => ({ data: "ok", isError: false })),
  usePrograms: vi.fn(() => ({ data: mockPrograms })),
  useEvents: vi.fn(() => ({ data: mockEvents })),
  useSensor: vi.fn(() => ({ data: mockSensor })),
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
});
