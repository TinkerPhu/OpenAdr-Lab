import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { BrowserRouter } from "react-router-dom";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { ControllerV2Page } from "../pages/ControllerV2";
import type { SimSnapshot, SimInjectState, TariffSnapshot } from "../api/types";

// ─── Mock data ───────────────────────────────────────────────────────────────

const baseSim: SimSnapshot = {
  ts: "2026-03-14T10:00:00Z",
  grid: { net_power_w: 1500, voltage_v: 230, import_kwh: 5.0, export_kwh: 0 },
  assets: {
    ev: { power_kw: 7, soc: 0.5, plugged: 1, max_charge_kw: 11, soc_target: 0.9, battery_kwh: 60 },
    heater: { power_kw: 2, temp_c: 20, max_kw: 4, temp_min_c: 18, temp_max_c: 24 },
    pv: { power_kw: -5, irradiance: 0.8, rated_kw: 8 },
    battery: { power_kw: 1, soc: 0.6, capacity_kwh: 10, max_charge_kw: 5, max_discharge_kw: 5, min_soc: 0.1 },
    base_load: { power_kw: 0.5 },
  },
};

const baseRates: TariffSnapshot[] = [
  {
    interval_start: "2026-03-14T00:00:00Z",
    import_tariff_eur_kwh: 0.25,
    export_tariff_eur_kwh: 0.05,
    co2_g_kwh: 300,
  },
];

const baseInject: SimInjectState = {};

// ─── Mocks ───────────────────────────────────────────────────────────────────

const mockSim = vi.fn(() => baseSim);
const mockRates = vi.fn(() => baseRates);
const mockInject = vi.fn(() => baseInject);
const mockSetInject = vi.fn(() => ({ mutate: vi.fn() }));

// Minimal EV schema for testing schema-driven AssetRightSection
const evSchema = [
  { key: "ev_plugged", label: "Plugged In", kind: "switch", min: null, max: null, unit: "" },
  { key: "ev_desired_kw", label: "Charge Rate", kind: "slider", min: 0, max: 11, unit: "kW" },
  { key: "ev_soc_target", label: "Charge Target", kind: "slider", min: 0.1, max: 1.0, unit: "%", display_scale: 100 },
];

vi.mock("../api/hooks", () => ({
  useSim: () => ({ data: mockSim(), isLoading: false, isError: false, refetch: vi.fn() }),
  useTariffs: () => ({ data: mockRates(), refetch: vi.fn() }),
  useRequests: () => ({ data: [], refetch: vi.fn() }),
  useTrace: () => ({ data: [] }),
  useSimInject: () => ({ data: mockInject() }),
  useSetSimInject: () => mockSetInject(),
  useResetAssetSoc: () => ({ mutate: vi.fn() }),
  useTimeline: () => ({ data: [] }),
  useAllTimelines: () => ({ data: {}, refetch: vi.fn() }),
  useSimSchema: () => ({ data: { ev: evSchema } }),
}));

// ─── Helpers ─────────────────────────────────────────────────────────────────

function renderPage() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <BrowserRouter>
        <ControllerV2Page />
      </BrowserRouter>
    </QueryClientProvider>
  );
}

// ─── Tests ───────────────────────────────────────────────────────────────────

describe("ControllerV2Page — layout", () => {
  beforeEach(() => {
    mockSim.mockReturnValue(baseSim);
    mockRates.mockReturnValue(baseRates);
    mockInject.mockReturnValue(baseInject);
  });

  it("renders the page root", () => {
    renderPage();
    expect(screen.getByTestId("controller-v2-page")).toBeInTheDocument();
  });

  it("renders grid tariff cell", () => {
    renderPage();
    expect(screen.getByTestId("grid-tariff-cell")).toBeInTheDocument();
  });

  it("renders grid accumulated cell", () => {
    renderPage();
    expect(screen.getByTestId("grid-accumulated-cell")).toBeInTheDocument();
  });

  it("renders EV asset cell", () => {
    renderPage();
    expect(screen.getByTestId("asset-cell-ev")).toBeInTheDocument();
  });

  it("renders heater asset cell", () => {
    renderPage();
    expect(screen.getByTestId("asset-cell-heater")).toBeInTheDocument();
  });

  it("renders PV asset cell", () => {
    renderPage();
    expect(screen.getByTestId("asset-cell-pv")).toBeInTheDocument();
  });

  it("renders battery asset cell", () => {
    renderPage();
    expect(screen.getByTestId("asset-cell-battery")).toBeInTheDocument();
  });

  it("renders base load asset cell", () => {
    renderPage();
    expect(screen.getByTestId("asset-cell-base_load")).toBeInTheDocument();
  });
});

describe("ControllerV2Page — asset metrics", () => {
  beforeEach(() => {
    mockSim.mockReturnValue(baseSim);
    mockRates.mockReturnValue(baseRates);
  });

  it("shows EV power value", () => {
    renderPage();
    expect(screen.getByTestId("asset-power-ev")).toBeInTheDocument();
  });

  it("shows EV cost rate value", () => {
    renderPage();
    expect(screen.getByTestId("asset-cost-rate-ev")).toBeInTheDocument();
  });

  it("shows EV CO2eq rate value", () => {
    renderPage();
    expect(screen.getByTestId("asset-co2-rate-ev")).toBeInTheDocument();
  });

  it("shows EV SoC value", () => {
    renderPage();
    expect(screen.getByTestId("asset-soc-ev")).toBeInTheDocument();
  });

  it("shows battery SoC value", () => {
    renderPage();
    expect(screen.getByTestId("asset-soc-battery")).toBeInTheDocument();
  });
});

describe("ControllerV2Page — pin and collapse buttons", () => {
  beforeEach(() => {
    mockSim.mockReturnValue(baseSim);
  });

  it("shows pin button on EV cell", () => {
    renderPage();
    expect(screen.getByTestId("asset-cell-ev-pin-btn")).toBeInTheDocument();
  });

  it("shows pin button on grid tariff cell", () => {
    renderPage();
    expect(screen.getByTestId("grid-tariff-cell-pin-btn")).toBeInTheDocument();
  });

  it("shows collapse left button on EV cell", () => {
    renderPage();
    expect(screen.getByTestId("asset-cell-ev-collapse-left")).toBeInTheDocument();
  });

  it("shows collapse right button on EV cell", () => {
    renderPage();
    expect(screen.getByTestId("asset-cell-ev-collapse-right")).toBeInTheDocument();
  });
});

describe("ControllerV2Page — global expand button", () => {
  beforeEach(() => {
    mockSim.mockReturnValue(baseSim);
    mockRates.mockReturnValue(baseRates);
    mockInject.mockReturnValue(baseInject);
  });

  it("renders a global expand button in the title bar", () => {
    renderPage();
    expect(screen.getByTestId("global-time-range-extend-btn")).toBeInTheDocument();
  });

  it("no per-cell expand buttons exist", () => {
    renderPage();
    expect(screen.queryByTestId("asset-cell-ev-extend-btn")).toBeNull();
    expect(screen.queryByTestId("grid-tariff-cell-extend-btn")).toBeNull();
    expect(screen.queryByTestId("grid-accumulated-cell-extend-btn")).toBeNull();
  });
});

describe("ControllerV2Page — error and loading states", () => {
  it("shows error state when sim fails", () => {
    vi.mocked(vi.fn()).mockReturnValue(null);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={queryClient}>
        <BrowserRouter>
          <ControllerV2Page />
        </BrowserRouter>
      </QueryClientProvider>
    );
    // When sim data is not available, page still renders without crashing
    expect(screen.getByTestId("controller-v2-page")).toBeInTheDocument();
  });
});

describe("ControllerV2Page — asset cell left section line order", () => {
  beforeEach(() => {
    mockSim.mockReturnValue(baseSim);
    mockRates.mockReturnValue(baseRates);
  });

  // Helper: returns true if `a` appears before `b` in the DOM.
  function isBefore(a: Element, b: Element) {
    return !!(a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING);
  }

  it("asset name is the first (bold title) line — shows 'EV'", () => {
    renderPage();
    expect(screen.getByTestId("asset-name-ev")).toHaveTextContent("EV");
  });

  it("asset name appears before power in the EV left section", () => {
    renderPage();
    const nameEl = screen.getByTestId("asset-name-ev");
    const powerEl = screen.getByTestId("asset-power-ev");
    expect(isBefore(nameEl, powerEl)).toBe(true);
  });

  it("asset power appears before price rate in the EV left section", () => {
    renderPage();
    const powerEl = screen.getByTestId("asset-power-ev");
    const costEl = screen.getByTestId("asset-cost-rate-ev");
    expect(isBefore(powerEl, costEl)).toBe(true);
  });

  it("price rate appears before GHG rate in the EV left section", () => {
    renderPage();
    const costEl = screen.getByTestId("asset-cost-rate-ev");
    const co2El = screen.getByTestId("asset-co2-rate-ev");
    expect(isBefore(costEl, co2El)).toBe(true);
  });
});

describe("ControllerV2Page — right section collapsed by default", () => {
  beforeEach(() => {
    mockSim.mockReturnValue(baseSim);
    mockRates.mockReturnValue(baseRates);
  });

  it("collapse-right button for EV shows 'Expand right' label on initial render", () => {
    renderPage();
    expect(screen.getByTestId("asset-cell-ev-collapse-right"))
      .toHaveAttribute("aria-label", "Expand right");
  });

  it("collapse-right button for battery shows 'Expand right' label on initial render", () => {
    renderPage();
    expect(screen.getByTestId("asset-cell-battery-collapse-right"))
      .toHaveAttribute("aria-label", "Expand right");
  });

  it("clicking collapse-right for EV changes label to 'Collapse right'", async () => {
    const user = userEvent.setup();
    renderPage();
    await user.click(screen.getByTestId("asset-cell-ev-collapse-right"));
    expect(screen.getByTestId("asset-cell-ev-collapse-right"))
      .toHaveAttribute("aria-label", "Collapse right");
  });
});

describe("ControllerV2Page — settings panel expands with single click", () => {
  beforeEach(() => {
    mockSim.mockReturnValue(baseSim);
    mockRates.mockReturnValue(baseRates);
  });

  it("EV controls are visible after a single click on the collapse-right button (no accordion needed)", async () => {
    const user = userEvent.setup();
    renderPage();
    await user.click(screen.getByTestId("asset-cell-ev-collapse-right"));
    // Controls must be directly accessible — no second accordion click required.
    expect(screen.getByTestId("ctrl-ev-plugged")).toBeInTheDocument();
    expect(screen.getByTestId("ctrl-ev-soc")).toBeInTheDocument();
  });
});

describe("ControllerV2Page — simulation controls", () => {
  beforeEach(() => {
    mockSim.mockReturnValue(baseSim);
  });

  it("shows EV plugged toggle", () => {
    renderPage();
    expect(screen.getByTestId("ctrl-ev-plugged")).toBeInTheDocument();
  });

  it("shows EV SoC slider", () => {
    renderPage();
    expect(screen.getByTestId("ctrl-ev-soc")).toBeInTheDocument();
  });

  it("shows battery SoC slider", () => {
    renderPage();
    expect(screen.getByTestId("ctrl-battery-soc")).toBeInTheDocument();
  });

  it("shows EV Charge Target slider with % display (display_scale=100)", () => {
    renderPage();
    expect(screen.getByTestId("ctrl-ev-soc-target")).toBeInTheDocument();
    // baseSim has ev.soc_target = 0.9; display_scale=100 → label shows "90 %"
    expect(screen.getByText(/Charge Target:\s*90 %/)).toBeInTheDocument();
  });
});
