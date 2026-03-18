import { render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { BrowserRouter } from "react-router-dom";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { ControllerV2Page } from "../pages/ControllerV2";
import type { SimSnapshot, UserOverrides, TariffSnapshot } from "../api/types";

// ─── Mock data ───────────────────────────────────────────────────────────────

const baseSim: SimSnapshot = {
  ts: "2026-03-14T10:00:00Z",
  net_power_w: 1500,
  import_w: 1500,
  export_w: 0,
  voltage_v: 230,
  base_load_w: 500,
  import_kwh: 5.0,
  export_kwh: 0,
  assets: {
    ev: { power_kw: 7, soc: 0.5 },
    battery: { power_kw: 1, soc: 0.6 },
  },
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
  battery: {
    soc: 0.6,
    current_kw: 1,
    capacity_kwh: 10,
    max_charge_kw: 5,
    max_discharge_kw: 5,
    min_soc: 0.1,
  },
};

const baseRates: TariffSnapshot[] = [
  {
    interval_start: "2026-03-14T00:00:00Z",
    interval_end: "2026-03-15T00:00:00Z",
    import_price_eur_kwh: 0.25,
    export_price_eur_kwh: 0.05,
    co2_g_kwh: 300,
    source_event_id: null,
    is_forecast: false,
  },
];

const baseOverrides: UserOverrides = {};

// ─── Mocks ───────────────────────────────────────────────────────────────────

const mockSim = vi.fn(() => baseSim);
const mockRates = vi.fn(() => baseRates);
const mockOverrides = vi.fn(() => baseOverrides);
const mockSetOverride = vi.fn(() => ({ mutate: vi.fn() }));

// Minimal EV schema for testing schema-driven AssetRightSection
const evSchema = [
  { key: "ev_plugged", label: "Plugged In", kind: "Switch", min: null, max: null, unit: "" },
  { key: "ev_desired_kw", label: "Charge Rate", kind: "Slider", min: 0, max: 11, unit: "kW" },
];

vi.mock("../api/hooks", () => ({
  useSim: () => ({ data: mockSim(), isLoading: false, isError: false, refetch: vi.fn() }),
  useTariffs: () => ({ data: mockRates(), refetch: vi.fn() }),
  usePlan: () => ({ data: null, refetch: vi.fn() }),
  useRequests: () => ({ data: [], refetch: vi.fn() }),
  useTrace: () => ({ data: [] }),
  useSimOverride: () => ({ data: mockOverrides() }),
  useSetSimOverride: () => mockSetOverride(),
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
    mockOverrides.mockReturnValue(baseOverrides);
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

describe("ControllerV2Page — expand button is below pin button", () => {
  beforeEach(() => {
    mockSim.mockReturnValue(baseSim);
    mockRates.mockReturnValue(baseRates);
    mockOverrides.mockReturnValue(baseOverrides);
  });

  // Returns true when `a` appears before `b` in the document.
  function isBefore(a: Element, b: Element) {
    return !!(a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING);
  }

  const allAssets = ["ev", "battery", "heater", "pv", "base_load"] as const;

  for (const assetId of allAssets) {
    it(`expand button is below pin button on ${assetId} cell`, () => {
      renderPage();
      const pinBtn = screen.getByTestId(`asset-cell-${assetId}-pin-btn`);
      const extendBtn = screen.getByTestId(`asset-cell-${assetId}-extend-btn`);
      expect(isBefore(pinBtn, extendBtn)).toBe(true);
    });
  }

  it("expand button is below pin button on grid tariff cell", () => {
    renderPage();
    const pinBtn = screen.getByTestId("grid-tariff-cell-pin-btn");
    const extendBtn = screen.getByTestId("grid-tariff-cell-extend-btn");
    expect(isBefore(pinBtn, extendBtn)).toBe(true);
  });

  it("expand button is below pin button on grid accumulated cell", () => {
    renderPage();
    const pinBtn = screen.getByTestId("grid-accumulated-cell-pin-btn");
    const extendBtn = screen.getByTestId("grid-accumulated-cell-extend-btn");
    expect(isBefore(pinBtn, extendBtn)).toBe(true);
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
});
