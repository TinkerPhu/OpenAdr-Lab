import { render, screen, fireEvent, act } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { AssetRightSection } from "../components/controller-v2/AssetRightSection";
import type { SimSnapshot } from "../api/types";

// Make schema configurable per-describe via a vi.fn() so individual suites can
// inject the PV control descriptors without affecting the SoC suite (empty schema).
const mockSchemaData = vi.fn(() => ({} as Record<string, unknown>));

vi.mock("../api/hooks", () => ({
  useSimSchema: () => ({ data: mockSchemaData() }),
}));

// ─── Fixtures ─────────────────────────────────────────────────────────────────

const simWithBattery: SimSnapshot = {
  ts: "2026-03-14T10:00:00Z",
  grid: { net_power_w: 1500, voltage_v: 230, import_kwh: 5.0, export_kwh: 0 },
  assets: {
    battery: { power_kw: 1, soc: 0.6, capacity_kwh: 10, max_charge_kw: 5, max_discharge_kw: 5, min_soc: 0.1 },
  },
};

const simWithEv: SimSnapshot = {
  ts: "2026-03-14T10:00:00Z",
  grid: { net_power_w: 1500, voltage_v: 230, import_kwh: 5.0, export_kwh: 0 },
  assets: {
    ev: { power_kw: 7, soc: 0.5, plugged: 1, max_charge_kw: 11, soc_target: 0.9, battery_kwh: 60 },
  },
};

const simWithPv: SimSnapshot = {
  ts: "2026-03-14T10:00:00Z",
  grid: { net_power_w: 1500, voltage_v: 230, import_kwh: 5.0, export_kwh: 0 },
  assets: {
    pv: { power_kw: -5, irradiance: 0.8, rated_kw: 8 },
  },
};

const pvSchema = [
  { key: "pv_irradiance", label: "Irradiance Override", kind: "slider" as const, min: 0, max: 1, unit: "", display_scale: undefined },
  { key: "pv_irradiance_alpha", label: "Blend-back Speed", kind: "slider" as const, min: 0.01, max: 1, unit: "", display_scale: undefined },
];

// ─── Helpers ─────────────────────────────────────────────────────────────────

function getSocSliderInput(assetId: string): HTMLInputElement {
  const root = screen.getByTestId(`ctrl-${assetId}-soc`);
  const input = root.querySelector('input[type="range"]') as HTMLInputElement;
  if (!input) throw new Error(`No range input found inside ctrl-${assetId}-soc`);
  return input;
}

function getSchemaSliderInput(key: string): HTMLInputElement {
  const testId = `ctrl-${key.replace(/_/g, "-")}`;
  const root = screen.getByTestId(testId);
  const input = root.querySelector('input[type="range"]') as HTMLInputElement;
  if (!input) throw new Error(`No range input inside ${testId}`);
  return input;
}

// ─── SoC slider — no snap-back ───────────────────────────────────────────────

describe("AssetRightSection — SoC slider no snap-back", () => {
  beforeEach(() => {
    mockSchemaData.mockReturnValue({});
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("battery: slider holds pending value after debounce fires, releases only when onDone called", () => {
    let capturedOnDone: (() => void) | undefined;
    const mockOnResetSoc = vi.fn((_assetId: string, _soc: number, onDone: () => void) => {
      capturedOnDone = onDone;
    });

    render(
      <AssetRightSection
        assetId="battery"
        simSnapshot={simWithBattery}
        overrides={undefined}
        onOverrideChange={vi.fn()}
        onResetSoc={mockOnResetSoc}
      />
    );

    // Initial: shows live SoC (60%)
    expect(screen.getByText("SoC: 60%")).toBeInTheDocument();

    // Drag slider to 80%
    act(() => {
      fireEvent.change(getSocSliderInput("battery"), { target: { value: "80" } });
    });

    // pendingSocPct set immediately — label updates
    expect(screen.getByText(/SoC: 80%/)).toBeInTheDocument();

    // Fire the 500ms debounce
    act(() => {
      vi.advanceTimersByTime(500);
    });

    // POST was issued with the right args
    expect(mockOnResetSoc).toHaveBeenCalledWith("battery", 0.8, expect.any(Function));
    expect(capturedOnDone).toBeDefined();

    // KEY: slider must NOT snap back to 60% before onDone is called
    expect(screen.getByText(/SoC: 80%/)).toBeInTheDocument();

    // onDone fires (mutation success + refetch complete in production)
    act(() => {
      capturedOnDone!();
    });

    // pendingSocPct cleared → reverts to live value
    expect(screen.getByText("SoC: 60%")).toBeInTheDocument();
  });

  it("ev: slider holds pending value after debounce fires, releases only when onDone called", () => {
    let capturedOnDone: (() => void) | undefined;
    const mockOnResetSoc = vi.fn((_assetId: string, _soc: number, onDone: () => void) => {
      capturedOnDone = onDone;
    });

    render(
      <AssetRightSection
        assetId="ev"
        simSnapshot={simWithEv}
        overrides={undefined}
        onOverrideChange={vi.fn()}
        onResetSoc={mockOnResetSoc}
      />
    );

    // Initial: shows live SoC (50%)
    expect(screen.getByText("SoC: 50%")).toBeInTheDocument();

    // Drag slider to 90%
    act(() => {
      fireEvent.change(getSocSliderInput("ev"), { target: { value: "90" } });
    });

    expect(screen.getByText(/SoC: 90%/)).toBeInTheDocument();

    act(() => {
      vi.advanceTimersByTime(500);
    });

    expect(mockOnResetSoc).toHaveBeenCalledWith("ev", 0.9, expect.any(Function));
    expect(capturedOnDone).toBeDefined();

    // KEY: no snap-back before onDone
    expect(screen.getByText(/SoC: 90%/)).toBeInTheDocument();

    act(() => {
      capturedOnDone!();
    });

    expect(screen.getByText("SoC: 50%")).toBeInTheDocument();
  });

  it("battery: multiple rapid drags only POST the final value", () => {
    const mockOnResetSoc = vi.fn();

    render(
      <AssetRightSection
        assetId="battery"
        simSnapshot={simWithBattery}
        overrides={undefined}
        onOverrideChange={vi.fn()}
        onResetSoc={mockOnResetSoc}
      />
    );

    const input = getSocSliderInput("battery");

    act(() => {
      fireEvent.change(input, { target: { value: "70" } });
    });
    act(() => {
      vi.advanceTimersByTime(200);
    });
    act(() => {
      fireEvent.change(input, { target: { value: "80" } });
    });
    act(() => {
      vi.advanceTimersByTime(200);
    });
    act(() => {
      fireEvent.change(input, { target: { value: "90" } });
    });

    // No POST yet — all within the debounce window
    expect(mockOnResetSoc).not.toHaveBeenCalled();

    act(() => {
      vi.advanceTimersByTime(500);
    });

    // Only the final value fires
    expect(mockOnResetSoc).toHaveBeenCalledTimes(1);
    expect(mockOnResetSoc).toHaveBeenCalledWith("battery", 0.9, expect.any(Function));
  });
});

// ─── Schema-driven sliders — instant local responsiveness ─────────────────────

describe("AssetRightSection — schema-driven sliders instant response", () => {
  beforeEach(() => {
    mockSchemaData.mockReturnValue({ pv: pvSchema });
    vi.useFakeTimers();
  });

  afterEach(() => {
    mockSchemaData.mockReturnValue({});
    vi.useRealTimers();
  });

  it("irradiance: initially shows live sim irradiance when no override is active", () => {
    render(
      <AssetRightSection
        assetId="pv"
        simSnapshot={simWithPv}   // irradiance = 0.8
        overrides={undefined}
        onOverrideChange={vi.fn()}
        onResetSoc={vi.fn()}
      />
    );

    // Should show live irradiance (0.80), not the slider min (0.00)
    expect(screen.getByText(/Irradiance Override: 0\.80/)).toBeInTheDocument();
  });

  it("irradiance: label updates immediately on drag before debounce fires", () => {
    const mockOnOverrideChange = vi.fn();

    render(
      <AssetRightSection
        assetId="pv"
        simSnapshot={simWithPv}
        overrides={undefined}
        onOverrideChange={mockOnOverrideChange}
        onResetSoc={vi.fn()}
      />
    );

    // Drag irradiance to 0.6
    act(() => {
      fireEvent.change(getSchemaSliderInput("pv_irradiance"), { target: { value: "0.6" } });
    });

    // Label updates immediately — no network roundtrip needed
    expect(screen.getByText(/Irradiance Override: 0\.60/)).toBeInTheDocument();

    // onOverrideChange must NOT have fired yet (still in debounce window)
    expect(mockOnOverrideChange).not.toHaveBeenCalled();
  });

  it("irradiance: reverts to live sim irradiance after debounce fires", () => {
    const mockOnOverrideChange = vi.fn();

    render(
      <AssetRightSection
        assetId="pv"
        simSnapshot={simWithPv}   // irradiance = 0.8
        overrides={undefined}
        onOverrideChange={mockOnOverrideChange}
        onResetSoc={vi.fn()}
      />
    );

    // Drag to 0.6
    act(() => {
      fireEvent.change(getSchemaSliderInput("pv_irradiance"), { target: { value: "0.6" } });
    });
    expect(screen.getByText(/Irradiance Override: 0\.60/)).toBeInTheDocument();

    // Debounce fires: POST sent, local state released
    act(() => { vi.advanceTimersByTime(300); });

    expect(mockOnOverrideChange).toHaveBeenCalledWith({ pv_irradiance: 0.6 });
    // Slider now follows live irradiance again (0.8 in test fixture;
    // in production the sim freezes at 0.6 once the override is applied)
    expect(screen.getByText(/Irradiance Override: 0\.80/)).toBeInTheDocument();
  });

  it("irradiance: onOverrideChange debounced — fires once with final value after 300ms", () => {
    const mockOnOverrideChange = vi.fn();

    render(
      <AssetRightSection
        assetId="pv"
        simSnapshot={simWithPv}
        overrides={undefined}
        onOverrideChange={mockOnOverrideChange}
        onResetSoc={vi.fn()}
      />
    );

    const input = getSchemaSliderInput("pv_irradiance");

    // Three rapid drags
    act(() => { fireEvent.change(input, { target: { value: "0.3" } }); });
    act(() => { vi.advanceTimersByTime(100); });
    act(() => { fireEvent.change(input, { target: { value: "0.5" } }); });
    act(() => { vi.advanceTimersByTime(100); });
    act(() => { fireEvent.change(input, { target: { value: "0.7" } }); });

    // Not called yet
    expect(mockOnOverrideChange).not.toHaveBeenCalled();

    // Fire debounce
    act(() => { vi.advanceTimersByTime(300); });

    // Called exactly once with the final value
    expect(mockOnOverrideChange).toHaveBeenCalledTimes(1);
    expect(mockOnOverrideChange).toHaveBeenCalledWith({ pv_irradiance: 0.7 });
  });

  it("blend-back speed: label updates immediately on drag before debounce fires", () => {
    const mockOnOverrideChange = vi.fn();

    render(
      <AssetRightSection
        assetId="pv"
        simSnapshot={simWithPv}
        overrides={undefined}
        onOverrideChange={mockOnOverrideChange}
        onResetSoc={vi.fn()}
      />
    );

    // Drag blend-back speed to 0.5
    act(() => {
      fireEvent.change(getSchemaSliderInput("pv_irradiance_alpha"), { target: { value: "0.5" } });
    });

    // Label updates immediately
    expect(screen.getByText(/Blend-back Speed: 0\.50/)).toBeInTheDocument();

    // Not yet sent to server
    expect(mockOnOverrideChange).not.toHaveBeenCalled();
  });
});

// ─── Blend-back speed — ignores server updates once locally set ───────────────

describe("AssetRightSection — blend-back speed holds local value across prop updates", () => {
  beforeEach(() => {
    mockSchemaData.mockReturnValue({ pv: pvSchema });
    vi.useFakeTimers();
  });

  afterEach(() => {
    mockSchemaData.mockReturnValue({});
    vi.useRealTimers();
  });

  it("blend-back: local drag value persists after debounce and across prop updates", () => {
    const mockOnOverrideChange = vi.fn();

    const { rerender } = render(
      <AssetRightSection
        assetId="pv"
        simSnapshot={simWithPv}
        overrides={undefined}
        onOverrideChange={mockOnOverrideChange}
        onResetSoc={vi.fn()}
      />
    );

    // User drags blend-back speed to 0.5
    act(() => {
      fireEvent.change(getSchemaSliderInput("pv_irradiance_alpha"), { target: { value: "0.5" } });
    });

    expect(screen.getByText(/Blend-back Speed: 0\.50/)).toBeInTheDocument();

    // Debounce fires
    act(() => { vi.advanceTimersByTime(300); });

    // Blend-back speed retains local value after debounce (unlike pv_irradiance)
    expect(screen.getByText(/Blend-back Speed: 0\.50/)).toBeInTheDocument();

    // Server pushes back its default (0.1) via overrides prop update
    rerender(
      <AssetRightSection
        assetId="pv"
        simSnapshot={simWithPv}
        overrides={{ pv_irradiance_alpha: 0.1 }}
        onOverrideChange={mockOnOverrideChange}
        onResetSoc={vi.fn()}
      />
    );

    // Local value must win — no revert to 0.10
    expect(screen.getByText(/Blend-back Speed: 0\.50/)).toBeInTheDocument();
  });

  it("irradiance: reverts to live sim value after debounce (contrast with blend-back)", () => {
    const mockOnOverrideChange = vi.fn();

    render(
      <AssetRightSection
        assetId="pv"
        simSnapshot={simWithPv}   // irradiance = 0.8
        overrides={undefined}
        onOverrideChange={mockOnOverrideChange}
        onResetSoc={vi.fn()}
      />
    );

    act(() => {
      fireEvent.change(getSchemaSliderInput("pv_irradiance"), { target: { value: "0.3" } });
    });
    expect(screen.getByText(/Irradiance Override: 0\.30/)).toBeInTheDocument();

    act(() => { vi.advanceTimersByTime(300); });

    // Unlike blend-back, irradiance releases local hold and follows live sim
    expect(screen.getByText(/Irradiance Override: 0\.80/)).toBeInTheDocument();
  });
});
