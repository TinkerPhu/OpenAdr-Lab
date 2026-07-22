import { render, screen, within, fireEvent } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, it, expect, vi, beforeEach } from "vitest";
import App from "../App";

vi.mock("../api/client", () => ({
  // vitest 4: a mock invoked with `new` must use `function`, not an arrow
  VenApi: vi.fn().mockImplementation(function () {
    return {
    baseUrl: "http://localhost:8081",
    health: vi.fn().mockResolvedValue({
      status: "ok",
      components: {
        ven_process: { status: "ok" },
        vtn_connection: { status: "ok" },
        storage: { status: "ok" },
        planner: { status: "ok" },
      },
    }),
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

  it("renders primary navigation links directly", () => {
    renderApp();
    expect(screen.getByTestId("nav-dashboard")).toBeVisible();
    expect(screen.getByTestId("nav-devices")).toBeVisible();
    expect(screen.getByTestId("nav-controller")).toBeVisible();
    expect(screen.getByTestId("nav-history")).toBeVisible();
    expect(screen.getByTestId("nav-planner")).toBeVisible();
    expect(screen.getByTestId("nav-weather")).toBeVisible();
    expect(screen.getByTestId("nav-notifications")).toBeVisible();
  });

  // WP-T8 (docs/history/project_journal.md, search "WP-T" §3.2): Reports/Programs/Events
  // are grouped behind a "VTN Feed" menu, not shown in the flat top bar.
  it("groups Reports/Programs/Events behind the VTN Feed menu", () => {
    renderApp();
    expect(screen.getByTestId("nav-vtn-feed-menu")).toBeVisible();
    expect(screen.queryByTestId("nav-reports")).not.toBeInTheDocument();
    expect(screen.queryByTestId("nav-programs")).not.toBeInTheDocument();
    expect(screen.queryByTestId("nav-events")).not.toBeInTheDocument();

    fireEvent.click(screen.getByTestId("nav-vtn-feed-menu"));
    expect(screen.getByTestId("nav-reports")).toBeVisible();
    expect(screen.getByTestId("nav-programs")).toBeVisible();
    expect(screen.getByTestId("nav-events")).toBeVisible();
  });

  // The Diagnostics group is grouped for the same usage-frequency reason but
  // must never be gated behind a settings flag (design principle 2, §2) —
  // this test only checks it's reachable, not that it starts open.
  it("groups Diagnostics pages behind an always-visible Diagnostics menu", () => {
    renderApp();
    expect(screen.getByTestId("nav-diagnostics-menu")).toBeVisible();
    expect(screen.queryByTestId("nav-tasks")).not.toBeInTheDocument();

    fireEvent.click(screen.getByTestId("nav-diagnostics-menu"));
    expect(screen.getByTestId("nav-metrics")).toBeVisible();
    expect(screen.getByTestId("nav-raw-diagnostics")).toBeVisible();
    expect(screen.getByTestId("nav-tasks")).toBeVisible();
    expect(screen.getByTestId("nav-event-log")).toBeVisible();
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
