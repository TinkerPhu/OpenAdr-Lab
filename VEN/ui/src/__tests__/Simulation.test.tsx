import { render, screen, act } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { BrowserRouter } from "react-router-dom";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { SimulationPage } from "../pages/Simulation";
import type { SimSnapshot, TraceEntry, UserOverrides } from "../api/types";

// ─── Mock data ───────────────────────────────────────────────────────────────

const baseSim: SimSnapshot = {
  ts: "2026-02-20T10:00:00Z",
  net_power_w: 500,
  import_w: 500,
  export_w: 0,
  voltage_v: 230,
  base_load_w: 500,
  import_kwh: 1.5,
  export_kwh: 0,
  assets: {},
  ev: {
    soc: 0.5,
    plugged: true,
    current_kw: 7,
    max_charge_kw: 11,
    soc_target: 0.9,
    battery_kwh: 60,
  },
  heater: {
    temp_c: 20,
    current_kw: 2,
    max_kw: 4,
    temp_min_c: 18,
    temp_max_c: 24,
  },
  pv: {
    irradiance: 0.8,
    export_limit_kw: null,
    current_kw: 5,
    rated_kw: 8,
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
const mockSimOverrideData = vi.fn(() => ({} as UserOverrides));
const mockSetSimOverride = vi.fn(() => ({ mutate: vi.fn() }));

vi.mock("../api/hooks", () => ({
  useSim: () => ({ data: mockSimData(), isError: false }),
  useTrace: (limit: number) => ({
    data: limit === 1 ? mockTrace1Data() : [],
  }),
  useEvents: () => ({ data: [] }),
  useSimOverride: () => ({ data: mockSimOverrideData() }),
  useSetSimOverride: () => mockSetSimOverride(),
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

describe("SimulationPage — EV override controls", () => {
  beforeEach(() => {
    mockSimData.mockReturnValue(baseSim);
    mockSimOverrideData.mockReturnValue({});
    mockSetSimOverride.mockReturnValue({ mutate: vi.fn() });
  });

  it("ev slider is enabled and shows idle caption when no event active", () => {
    mockTrace1Data.mockReturnValue([makeTrace("IDLE", 11, 2, 0)]);
    renderSim();
    // No override toggle should be shown (no event)
    expect(screen.queryByTestId("ev-charge-override-toggle")).not.toBeInTheDocument();
    // Slider is enabled — data-testid is on the Box wrapper; find input inside it
    const wrapper = screen.getByTestId("ev-charge-slider");
    const input = wrapper.querySelector("input[type='range']") as HTMLInputElement;
    expect(input).not.toBeDisabled();
    // Caption says idle default
    expect(screen.getByTestId("ev-charge-caption")).toHaveTextContent("No active event");
  });

  it("ev slider is disabled when CHARGE_STATE_SETPOINT event active and no force override", () => {
    mockTrace1Data.mockReturnValue([makeTrace("CHARGE_SETPOINT", 7, 2, 0)]);
    renderSim();
    // Toggle switch should be visible
    expect(screen.getByTestId("ev-charge-override-toggle")).toBeInTheDocument();
    // Slider input should be disabled — data-testid is on the Box wrapper
    const wrapper = screen.getByTestId("ev-charge-slider");
    const input = wrapper.querySelector("input[type='range']") as HTMLInputElement;
    expect(input).toBeDisabled();
    // Caption mentions VTN commands
    expect(screen.getByTestId("ev-charge-caption")).toHaveTextContent("VTN commands");
  });

  it("ev caption shows VTN value when event active", () => {
    mockTrace1Data.mockReturnValue([makeTrace("CHARGE_SETPOINT", 7, 2, 0)]);
    renderSim();
    expect(screen.getByTestId("ev-charge-caption")).toHaveTextContent("7.0 kW");
  });

  it("ev slider has negative range when ev max_charge_kw is 11", () => {
    mockTrace1Data.mockReturnValue([makeTrace("IDLE", 11, 2, 0)]);
    renderSim();
    const wrapper = screen.getByTestId("ev-charge-slider");
    const input = wrapper.querySelector("input[type='range']") as HTMLInputElement;
    expect(input).toHaveAttribute("aria-valuemin", "-11");
  });

  it("ev override toggle enables slider and shows override caption", async () => {
    mockTrace1Data.mockReturnValue([makeTrace("CHARGE_SETPOINT", 7, 2, 0)]);
    renderSim();

    const toggle = screen.getByTestId("ev-charge-override-toggle");
    await act(async () => {
      await userEvent.click(toggle);
    });

    // Slider should now be enabled
    const wrapper = screen.getByTestId("ev-charge-slider");
    const input = wrapper.querySelector("input[type='range']") as HTMLInputElement;
    expect(input).not.toBeDisabled();
    // Caption should mention overriding
    expect(screen.getByTestId("ev-charge-caption")).toHaveTextContent("Overriding VTN");
  });

  it("ev override caption shows VTN intent value when overriding", async () => {
    mockTrace1Data.mockReturnValue([makeTrace("CHARGE_SETPOINT", 7, 2, 0)]);
    renderSim();

    const toggle = screen.getByTestId("ev-charge-override-toggle");
    await act(async () => {
      await userEvent.click(toggle);
    });

    expect(screen.getByTestId("ev-charge-caption")).toHaveTextContent("VTN intent: 7.0 kW");
  });
});

describe("SimulationPage — Heater override controls", () => {
  beforeEach(() => {
    mockSimData.mockReturnValue(baseSim);
    mockSimOverrideData.mockReturnValue({});
    mockSetSimOverride.mockReturnValue({ mutate: vi.fn() });
  });

  it("heater force slider is disabled when IMPORT_CAP event active and no override", () => {
    mockTrace1Data.mockReturnValue([makeTrace("IMPORT_CAP", 0, 0, 0)]);
    renderSim();
    const wrapper = screen.getByTestId("heater-force-slider");
    const input = wrapper.querySelector("input[type='range']") as HTMLInputElement;
    expect(input).toBeDisabled();
  });

  it("heater override toggle enables slider", async () => {
    mockTrace1Data.mockReturnValue([makeTrace("IMPORT_CAP", 0, 0, 0)]);
    renderSim();

    const toggle = screen.getByTestId("heater-force-override-toggle");
    await act(async () => {
      await userEvent.click(toggle);
    });

    const wrapper = screen.getByTestId("heater-force-slider");
    const input = wrapper.querySelector("input[type='range']") as HTMLInputElement;
    expect(input).not.toBeDisabled();
  });

  it("heater force slider is enabled when no event", () => {
    mockTrace1Data.mockReturnValue([makeTrace("IDLE", 11, 2, 0)]);
    renderSim();
    // No toggle shown when no event
    expect(screen.queryByTestId("heater-force-override-toggle")).not.toBeInTheDocument();
    const wrapper = screen.getByTestId("heater-force-slider");
    const input = wrapper.querySelector("input[type='range']") as HTMLInputElement;
    expect(input).not.toBeDisabled();
  });
});

describe("SimulationPage — PV curtailment override controls", () => {
  beforeEach(() => {
    mockSimData.mockReturnValue(baseSim);
    mockSimOverrideData.mockReturnValue({});
    mockSetSimOverride.mockReturnValue({ mutate: vi.fn() });
  });

  it("pv force slider is disabled when EXPORT_CAP event active and no override", () => {
    mockTrace1Data.mockReturnValue([makeTrace("EXPORT_CAP", 11, 4, 5.0)]);
    renderSim();
    const wrapper = screen.getByTestId("pv-force-slider");
    const input = wrapper.querySelector("input[type='range']") as HTMLInputElement;
    expect(input).toBeDisabled();
    expect(screen.getByTestId("pv-force-caption")).toHaveTextContent("VTN commands");
  });

  it("pv curtailment override toggle enables slider", async () => {
    mockTrace1Data.mockReturnValue([makeTrace("EXPORT_CAP", 11, 4, 5.0)]);
    renderSim();

    const toggle = screen.getByTestId("pv-force-override-toggle");
    await act(async () => {
      await userEvent.click(toggle);
    });

    const wrapper = screen.getByTestId("pv-force-slider");
    const input = wrapper.querySelector("input[type='range']") as HTMLInputElement;
    expect(input).not.toBeDisabled();
  });

  it("pv force slider is enabled when no event", () => {
    mockTrace1Data.mockReturnValue([makeTrace("IDLE", 11, 2, 0)]);
    renderSim();
    expect(screen.queryByTestId("pv-force-override-toggle")).not.toBeInTheDocument();
    const wrapper = screen.getByTestId("pv-force-slider");
    const input = wrapper.querySelector("input[type='range']") as HTMLInputElement;
    expect(input).not.toBeDisabled();
  });
});
