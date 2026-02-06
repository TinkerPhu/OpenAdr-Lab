import { render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, it, expect, vi, beforeEach } from "vitest";
import App from "../App";

vi.mock("../api/client", () => ({
  VenApi: vi.fn().mockImplementation(() => ({
    baseUrl: "http://localhost:8081",
    health: vi.fn().mockResolvedValue("ok"),
    programs: vi.fn().mockResolvedValue([]),
    events: vi.fn().mockResolvedValue([]),
    sensors: vi.fn().mockResolvedValue({
      id: "s1", ts: "2024-01-01T00:00:00Z",
      temperature_c: 22, power_w: 100, voltage_v: 230, raw: {},
    }),
    postSensors: vi.fn().mockResolvedValue({}),
  })),
}));

function renderApp() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false, refetchInterval: false },
    },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <App />
    </QueryClientProvider>,
  );
}

describe("App shell", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("renders the VEN selector", () => {
    renderApp();
    expect(screen.getByTestId("ven-selector")).toBeInTheDocument();
    expect(screen.getByTestId("ven-selector")).toBeVisible();
  });

  it("renders navigation links", () => {
    renderApp();
    expect(screen.getByTestId("nav-dashboard")).toBeVisible();
    expect(screen.getByTestId("nav-programs")).toBeVisible();
    expect(screen.getByTestId("nav-events")).toBeVisible();
    expect(screen.getByTestId("nav-sensors")).toBeVisible();
  });

  it("renders auto-refresh toggle", () => {
    renderApp();
    expect(screen.getByTestId("auto-refresh-toggle")).toBeVisible();
  });

  it("renders refresh all button", () => {
    renderApp();
    expect(screen.getByTestId("refresh-all-btn")).toBeVisible();
  });

  it("renders health status chip", async () => {
    renderApp();
    const chip = await screen.findByTestId("health-status");
    expect(chip).toBeVisible();
  });
});
