import { render, screen, within, fireEvent } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, it, expect, vi, beforeEach } from "vitest";
import App from "../App";

vi.mock("../api/client", () => ({
  // vitest 4: a mock invoked with `new` must use `function`, not an arrow
  VenApi: vi.fn().mockImplementation(function () {
    return {
    baseUrl: "http://localhost:8081",
    health: vi.fn().mockResolvedValue("ok"),
    programs: vi.fn().mockResolvedValue([]),
    events: vi.fn().mockResolvedValue([]),
    reports: vi.fn().mockResolvedValue([]),
    submitReport: vi.fn().mockResolvedValue({}),
    sensors: vi.fn().mockResolvedValue({
      id: "s1", ts: "2024-01-01T00:00:00Z",
      temperature_c: 22, power_w: 100, voltage_v: 230, raw: {},
    }),
    postSensors: vi.fn().mockResolvedValue({}),
    };
  }),
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
    expect(screen.getByTestId("nav-reports")).toBeVisible();
    expect(screen.getByTestId("nav-planner")).toBeVisible();
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

describe("dynamic VEN dropdown", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("extends the dropdown with a registered, healthy fleet VEN", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async (input: RequestInfo | URL) => {
        const url = String(input);
        if (url === "/api/vens-registry") {
          return {
            ok: true,
            json: async () => [
              { venName: "ven-1" },
              { venName: "fleet-ven-000" },
            ],
          } as Response;
        }
        if (url === "/api/dyn/fleet-ven-000/health") {
          return { ok: true } as Response;
        }
        return { ok: false, status: 404 } as Response;
      }),
    );
    try {
      renderApp();
      const selector = screen.getByTestId("ven-selector");
      const combobox = within(selector).getByRole("combobox");
      fireEvent.mouseDown(combobox);
      const fleetItem = await screen.findByText(/fleet-ven-000/);
      expect(fleetItem).toBeVisible();
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("keeps only the default trio when the registry is unreachable", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => {
        throw new Error("registry down");
      }),
    );
    try {
      renderApp();
      const selector = screen.getByTestId("ven-selector");
      const combobox = within(selector).getByRole("combobox");
      fireEvent.mouseDown(combobox);
      const items = await screen.findAllByRole("option");
      expect(items).toHaveLength(3);
    } finally {
      vi.unstubAllGlobals();
    }
  });
});
