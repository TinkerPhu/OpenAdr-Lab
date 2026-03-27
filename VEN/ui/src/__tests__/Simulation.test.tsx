import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { BrowserRouter } from "react-router-dom";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { SimulationPage } from "../pages/Simulation";
import type { SimSnapshot, TraceEntry, SimInjectState } from "../api/types";

// ─── Mock data ───────────────────────────────────────────────────────────────

const baseSim: SimSnapshot = {
  ts: "2026-02-20T10:00:00Z",
  grid: { net_power_w: 500, voltage_v: 230, import_kwh: 1.5, export_kwh: 0 },
  assets: {
    ev: { power_kw: 7, soc: 0.5, plugged: 1, max_charge_kw: 11, soc_target: 0.9, battery_kwh: 60 },
    heater: { power_kw: 2, temp_c: 20, max_kw: 4, temp_min_c: 18, temp_max_c: 24 },
    pv: { power_kw: -5, irradiance: 0.8, rated_kw: 8 },
    base_load: { power_kw: 0.5 },
  },
};

function makeTrace(mode: string, ev_charge_kw = 7, heater_kw = 2, pv_export_limit_kw: number | null = null): TraceEntry {
  return {
    ts: "2026-02-20T10:00:00Z",
    mode,
    fsm_state: "Holding",
    active_events: ["test-event"],
    winning_intent: `${mode}=7.0 (event: test-event, priority: 0)`,
    setpoints: { ev_charge_kw, heater_kw, pv_export_limit_kw, mode },
    constraints: ["EV max 11.0kW"],
    reason: "Holding setpoints for test-event",
  };
}

// ─── Mocks ───────────────────────────────────────────────────────────────────

const mockSimData = vi.fn(() => baseSim);
const mockTrace1Data = vi.fn(() => [makeTrace("IDLE", 11, 2, 0)]);
const mockSimInjectData = vi.fn(() => ({} as SimInjectState));
const mockSetSimInject = vi.fn(() => ({ mutate: vi.fn() }));

vi.mock("../api/hooks", () => ({
  useSim: () => ({ data: mockSimData(), isError: false }),
  useTrace: (limit: number) => ({
    data: limit === 1 ? mockTrace1Data() : [],
  }),
  useEvents: () => ({ data: [] }),
  useSimInject: () => ({ data: mockSimInjectData() }),
  useSetSimInject: () => mockSetSimInject(),
}));

// ─── Helpers ──────────────────────────────────────────────────────────────────

function renderSim() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <BrowserRouter>
        <SimulationPage />
      </BrowserRouter>
    </QueryClientProvider>,
  );
}

// ─── Tests ───────────────────────────────────────────────────────────────────

describe("SimulationPage — EV controls", () => {
  beforeEach(() => {
    mockSimData.mockReturnValue(baseSim);
    mockSimInjectData.mockReturnValue({});
    mockSetSimInject.mockReturnValue({ mutate: vi.fn() });
  });

  it("shows EV plugged switch", () => {
    renderSim();
    expect(screen.getByRole("checkbox", { name: /plugged in/i })).toBeInTheDocument();
  });

  it("shows EV SOC target slider with default from sim", () => {
    renderSim();
    expect(screen.getByText(/SOC target: 90%/i)).toBeInTheDocument();
  });

  it("ev plugged switch reflects inject state when override active", () => {
    mockSimInjectData.mockReturnValue({ ev_plugged: false });
    renderSim();
    const checkbox = screen.getByRole("checkbox", { name: /plugged in/i }) as HTMLInputElement;
    expect(checkbox.checked).toBe(false);
  });
});

describe("SimulationPage — PV irradiance controls", () => {
  beforeEach(() => {
    mockSimData.mockReturnValue(baseSim);
    mockSimInjectData.mockReturnValue({});
    mockSetSimInject.mockReturnValue({ mutate: vi.fn() });
  });

  it("irradiance toggle starts in auto mode", () => {
    renderSim();
    // Toggle label reads "Auto (time-based)" in initial state
    expect(screen.getByText(/Irradiance — Auto \(time-based\)/i)).toBeInTheDocument();
  });

  it("irradiance toggle label changes to Manual after click", async () => {
    renderSim();
    const toggle = screen.getByLabelText(/irradiance — auto/i);
    await userEvent.click(toggle);
    expect(screen.getByText(/Irradiance — Manual/i)).toBeInTheDocument();
  });
});

describe("SimulationPage — Heater controls", () => {
  beforeEach(() => {
    mockSimData.mockReturnValue(baseSim);
    mockSimInjectData.mockReturnValue({});
    mockSetSimInject.mockReturnValue({ mutate: vi.fn() });
  });

  it("shows ambient temperature text", () => {
    renderSim();
    expect(screen.getByText(/Ambient temperature/i)).toBeInTheDocument();
  });

  it("shows thermostat range from sim defaults", () => {
    renderSim();
    expect(screen.getByText(/Thermostat range: 18°C – 24°C/i)).toBeInTheDocument();
  });
});

describe("SimulationPage — Base load controls", () => {
  beforeEach(() => {
    mockSimData.mockReturnValue(baseSim);
    mockSimInjectData.mockReturnValue({});
    mockSetSimInject.mockReturnValue({ mutate: vi.fn() });
  });

  it("shows base load in kW", () => {
    renderSim();
    // base_load.power_kw = 0.5, displayed in kW
    expect(screen.getByText(/Base load \(profile override\): 0.50 kW/i)).toBeInTheDocument();
  });
});
