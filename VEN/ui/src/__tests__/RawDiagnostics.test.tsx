import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { RawDiagnosticsPage } from "../pages/RawDiagnostics";
import type { SimSnapshot, PlannedRates } from "../api/types";

// ─── Mock data ────────────────────────────────────────────────────────────────

const baseSim: SimSnapshot = {
  ts: "2026-03-18T10:00:00Z",
  grid: { net_power_w: 1200, voltage_v: 230, import_kwh: 5, export_kwh: 0 },
  assets: {
    ev: { power_kw: 7 },
    battery: { power_kw: -2 },
  },
};

const baseTariffs: PlannedRates = [
  {
    interval_start: "2026-03-18T09:00:00Z",
    import_tariff_eur_kwh: 0.25,
    export_tariff_eur_kwh: 0.05,
    co2_g_kwh: 350,
  },
];

const baseTimeline = {
  grid: [{ ts: Date.now() - 3600_000, values: { power_kw: 1.2 } }],
  ev: [{ ts: Date.now() - 3600_000, values: { power_kw: 7.0 } }],
};

// ─── Mocks ────────────────────────────────────────────────────────────────────

const mockSim = vi.fn().mockResolvedValue(baseSim);
const mockRates = vi.fn().mockResolvedValue(baseTariffs);
const mockAllTimelines = vi.fn().mockResolvedValue(baseTimeline);

vi.mock("../App", () => ({
  useVenContext: () => ({
    api: {
      baseUrl: "http://localhost:8211",
      sim: mockSim,
      rates: mockRates,
      allTimelines: mockAllTimelines,
    },
    venUrl: "http://localhost:8211",
    venName: "ven-1",
    setVenUrl: vi.fn(),
  }),
}));

// ─── Helpers ──────────────────────────────────────────────────────────────────

function renderPage() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <RawDiagnosticsPage />
    </QueryClientProvider>
  );
}

// ─── Tests ────────────────────────────────────────────────────────────────────

describe("RawDiagnosticsPage", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  // ── US1: Simulator State ────────────────────────────────────────────────────

  it("renders the Simulator State cell", () => {
    renderPage();
    expect(screen.getByTestId("diagnostic-cell-simulator-state")).toBeInTheDocument();
  });

  it("Simulator State cell starts in unloaded state", () => {
    renderPage();
    expect(screen.getByText("Click refresh to load simulator state.")).toBeInTheDocument();
  });

  it("Simulator State cell refresh button triggers api.sim()", async () => {
    renderPage();
    await userEvent.click(screen.getByTestId("refresh-btn-simulator-state"));
    expect(mockSim).toHaveBeenCalledOnce();
  });

  it("Simulator State chart renders after refresh", async () => {
    renderPage();
    await userEvent.click(screen.getByTestId("refresh-btn-simulator-state"));
    await waitFor(() =>
      expect(screen.getByTestId("sim-profile-chart")).toBeInTheDocument()
    );
  });

  // ── US2: Tariffs ────────────────────────────────────────────────────────────

  it("renders the Tariffs cell", () => {
    renderPage();
    expect(screen.getByTestId("diagnostic-cell-tariffs")).toBeInTheDocument();
  });

  it("Tariffs cell starts in unloaded state", () => {
    renderPage();
    expect(screen.getByText("Click refresh to load tariff data.")).toBeInTheDocument();
  });

  it("Tariffs cell refresh button triggers api.rates()", async () => {
    renderPage();
    await userEvent.click(screen.getByTestId("refresh-btn-tariffs"));
    expect(mockRates).toHaveBeenCalledOnce();
  });

  it("Tariffs chart renders after refresh", async () => {
    renderPage();
    await userEvent.click(screen.getByTestId("refresh-btn-tariffs"));
    await waitFor(() =>
      expect(screen.getByTestId("tariffs-line-chart")).toBeInTheDocument()
    );
  });

  it("refreshing Tariffs does not trigger Simulator State fetch", async () => {
    renderPage();
    await userEvent.click(screen.getByTestId("refresh-btn-tariffs"));
    await waitFor(() => expect(mockRates).toHaveBeenCalled());
    expect(mockSim).not.toHaveBeenCalled();
  });

  // ── US3: Timeline ───────────────────────────────────────────────────────────

  it("renders the Timeline cell", () => {
    renderPage();
    expect(screen.getByTestId("diagnostic-cell-timeline")).toBeInTheDocument();
  });

  it("Timeline cell starts with empty series state (chart always rendered, no series available)", () => {
    renderPage();
    expect(screen.getByTestId("timeline-series-chart")).toBeInTheDocument();
    expect(screen.getByText("No data for selected series")).toBeInTheDocument();
  });

  it("Timeline cell refresh button triggers api.allTimelines()", async () => {
    renderPage();
    await userEvent.click(screen.getByTestId("refresh-btn-timeline"));
    expect(mockAllTimelines).toHaveBeenCalledWith({ hoursBack: 1.0, hoursForward: 1.0 });
  });

  it("Timeline series dropdown appears after refresh and lists available series", async () => {
    renderPage();
    await userEvent.click(screen.getByTestId("refresh-btn-timeline"));
    await waitFor(() =>
      expect(screen.getByTestId("timeline-series-select")).toBeInTheDocument()
    );
  });

  it("Timeline chart renders after refresh", async () => {
    renderPage();
    await userEvent.click(screen.getByTestId("refresh-btn-timeline"));
    await waitFor(() =>
      expect(screen.getByTestId("timeline-series-chart")).toBeInTheDocument()
    );
  });

  it("refreshing Timeline does not trigger Simulator State or Tariffs fetch", async () => {
    renderPage();
    await userEvent.click(screen.getByTestId("refresh-btn-timeline"));
    await waitFor(() => expect(mockAllTimelines).toHaveBeenCalled());
    expect(mockSim).not.toHaveBeenCalled();
    expect(mockRates).not.toHaveBeenCalled();
  });
});
