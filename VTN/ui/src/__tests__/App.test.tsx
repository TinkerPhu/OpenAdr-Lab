import { render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, it, expect, vi, beforeEach } from "vitest";
import App from "../App";

vi.mock("../api/client", () => ({
  BffApi: vi.fn().mockImplementation(() => ({
    baseUrl: "",
    health: vi.fn().mockResolvedValue({
      time: "2026-01-01T00:00:00Z",
      bff: { ok: true, version: "0.1.0" },
      vtn: { reachable: true, authOk: true },
    }),
    programs: vi.fn().mockResolvedValue([]),
    events: vi.fn().mockResolvedValue([]),
    vens: vi.fn().mockResolvedValue([]),
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

  it("renders navigation links", () => {
    renderApp();
    expect(screen.getByTestId("nav-dashboard")).toBeVisible();
    expect(screen.getByTestId("nav-programs")).toBeVisible();
    expect(screen.getByTestId("nav-events")).toBeVisible();
    expect(screen.getByTestId("nav-vens")).toBeVisible();
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
