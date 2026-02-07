import { render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { BrowserRouter } from "react-router-dom";
import { describe, it, expect, vi } from "vitest";
import { DashboardPage } from "../pages/Dashboard";

const mockHealth = {
  time: "2026-01-01T00:00:00Z",
  bff: { ok: true, version: "0.1.0" },
  vtn: { reachable: true, authOk: true },
};

const mockPrograms = [
  { id: "p1", programName: "Program Alpha", createdDateTime: "2026-01-01" },
  { id: "p2", programName: "Program Beta", createdDateTime: "2026-01-02" },
];

const mockEvents = [
  { id: "e1", programID: "p1", eventName: "Event 1", createdDateTime: "2026-01-01", intervals: [] },
  { id: "e2", programID: "p1", eventName: "Event 2", createdDateTime: "2026-01-02", intervals: [] },
];

const mockVens = [
  { id: "v1", venName: "ven-1", createdDateTime: "2026-01-01" },
  { id: "v2", venName: "ven-2", createdDateTime: "2026-01-02" },
];

const mockReports = [
  { id: "r1", programID: "p1", eventID: "e1", clientName: "ven-1", resources: [], createdDateTime: "2026-01-01" },
];

vi.mock("../api/hooks", () => ({
  useHealth: vi.fn(() => ({ data: mockHealth, isError: false })),
  usePrograms: vi.fn(() => ({ data: mockPrograms })),
  useEvents: vi.fn(() => ({ data: mockEvents })),
  useVens: vi.fn(() => ({ data: mockVens })),
  useReports: vi.fn(() => ({ data: mockReports })),
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

  it("renders VENs card with count", () => {
    renderDashboard();
    expect(screen.getByTestId("dash-vens-card")).toBeVisible();
    expect(screen.getByTestId("dash-vens-count")).toHaveTextContent("2");
  });
});
