import { render, screen, fireEvent, act } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { AssetRightSection } from "../components/controller/AssetRightSection";
import type { SimSnapshot } from "../api/types";

// Minimal MUI Slider mock:
// - onChange  fires on fireEvent.change   → live drag, updates local state
// - onChangeCommitted fires on fireEvent.mouseUp → commit, triggers POST
// The span wrapper preserves the existing helper pattern (root.querySelector('input[type="range"]')).
vi.mock("@mui/material", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@mui/material")>();
  return {
    ...actual,
    Slider: (props: any) => {
      const { onChange, onChangeCommitted, value, min, max, step, "data-testid": testId } = props;
      return (
        <span data-testid={testId}>
          <input
            type="range"
            value={value ?? 0}
            min={min}
            max={max}
            step={step}
            onChange={(e) => onChange?.(e, Number(e.target.value))}
            onMouseUp={(e) => onChangeCommitted?.(e, Number((e.target as HTMLInputElement).value))}
            readOnly={!onChange}
          />
        </span>
      );
    },
  };
});

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
  { key: "pv_irradiance", label: "Irradiance Override", kind: "slider" as const, min: 0, max: 1, unit: "%", display_scale: 100 },
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

// ─── SoC slider — commit on mouse-up, no snap-back ───────────────────────────

describe("AssetRightSection — SoC slider commit on mouse-up", () => {
  beforeEach(() => {
    mockSchemaData.mockReturnValue({});
  });

  it("battery: drag updates label immediately; POST fires on mouse-up; no snap-back before onDone", () => {
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

    const input = getSocSliderInput("battery");

    // Drag slider to 80% — label updates immediately, no POST yet
    act(() => { fireEvent.change(input, { target: { value: "80" } }); });
    expect(screen.getByText(/SoC: 80%/)).toBeInTheDocument();
    expect(mockOnResetSoc).not.toHaveBeenCalled();

    // Mouse-up commits — POST fires immediately
    act(() => { fireEvent.mouseUp(input); });
    expect(mockOnResetSoc).toHaveBeenCalledWith("battery", 0.8, expect.any(Function));
    expect(capturedOnDone).toBeDefined();

    // KEY: slider must NOT snap back to 60% before onDone is called
    expect(screen.getByText(/SoC: 80%/)).toBeInTheDocument();

    // onDone fires (mutation success in production) — reverts to live value
    act(() => { capturedOnDone!(); });
    expect(screen.getByText("SoC: 60%")).toBeInTheDocument();
  });

  it("ev: drag updates label immediately; POST fires on mouse-up; no snap-back before onDone", () => {
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

    expect(screen.getByText("SoC: 50%")).toBeInTheDocument();

    const input = getSocSliderInput("ev");

    act(() => { fireEvent.change(input, { target: { value: "90" } }); });
    expect(screen.getByText(/SoC: 90%/)).toBeInTheDocument();
    expect(mockOnResetSoc).not.toHaveBeenCalled();

    act(() => { fireEvent.mouseUp(input); });
    expect(mockOnResetSoc).toHaveBeenCalledWith("ev", 0.9, expect.any(Function));
    expect(capturedOnDone).toBeDefined();

    expect(screen.getByText(/SoC: 90%/)).toBeInTheDocument();

    act(() => { capturedOnDone!(); });
    expect(screen.getByText("SoC: 50%")).toBeInTheDocument();
  });

  it("battery: multiple drags in one gesture POST the final value only", () => {
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

    // Simulate drag through multiple values — no POST during drag
    act(() => { fireEvent.change(input, { target: { value: "70" } }); });
    act(() => { fireEvent.change(input, { target: { value: "80" } }); });
    act(() => { fireEvent.change(input, { target: { value: "90" } }); });
    expect(mockOnResetSoc).not.toHaveBeenCalled();

    // Mouse-up commits — exactly one POST with the final value
    act(() => { fireEvent.mouseUp(input); });
    expect(mockOnResetSoc).toHaveBeenCalledTimes(1);
    expect(mockOnResetSoc).toHaveBeenCalledWith("battery", 0.9, expect.any(Function));
  });
});

// ─── Schema-driven sliders — instant local responsiveness ─────────────────────

describe("AssetRightSection — schema-driven sliders instant response", () => {
  beforeEach(() => {
    mockSchemaData.mockReturnValue({ pv: pvSchema });
  });

  afterEach(() => {
    mockSchemaData.mockReturnValue({});
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

    // Should show live irradiance as % (80 %), not the slider min (0 %)
    expect(screen.getByText(/Irradiance Override: 80 %/)).toBeInTheDocument();
  });

  it("irradiance: label updates immediately on drag; no POST until mouse-up", () => {
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

    // Drag irradiance to 60 % (display units; raw = 60/100 = 0.6)
    act(() => {
      fireEvent.change(getSchemaSliderInput("pv_irradiance"), { target: { value: "60" } });
    });

    // Label updates immediately — no network roundtrip needed
    expect(screen.getByText(/Irradiance Override: 60 %/)).toBeInTheDocument();

    // onOverrideChange must NOT have fired yet (no mouse-up)
    expect(mockOnOverrideChange).not.toHaveBeenCalled();
  });

  it("irradiance: stale simInject cache value does not block live irradiance display", () => {
    // Regression: getValue placed pv_irradiance AFTER the overrides check.
    // The server refetches simInject before the backend tick clears the one-shot,
    // so overrides.pv_irradiance = 0.7 was returned instead of the live value.
    render(
      <AssetRightSection
        assetId="pv"
        simSnapshot={simWithPv}          // irradiance = 0.8 (live)
        overrides={{ pv_irradiance: 0.3 } as never}  // stale cached one-shot
        onOverrideChange={vi.fn()}
        onResetSoc={vi.fn()}
      />
    );

    // Must show live irradiance (80 %), NOT the stale override (30 %)
    expect(screen.getByText(/Irradiance Override: 80 %/)).toBeInTheDocument();
  });

  it("irradiance: reverts to live sim irradiance after mouse-up commit", () => {
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

    const input = getSchemaSliderInput("pv_irradiance");

    // Drag to 60 % (display) = 0.6 (raw)
    act(() => { fireEvent.change(input, { target: { value: "60" } }); });
    expect(screen.getByText(/Irradiance Override: 60 %/)).toBeInTheDocument();

    // Mouse-up: POST sent, local state released
    act(() => { fireEvent.mouseUp(input); });

    // DynamicControl divides by scale: 60/100 = 0.6
    expect(mockOnOverrideChange).toHaveBeenCalledWith({ pv_irradiance: 0.6 });
    // Slider now follows live irradiance again (80 % from fixture)
    expect(screen.getByText(/Irradiance Override: 80 %/)).toBeInTheDocument();
  });

  it("irradiance: POST fires once on mouse-up with final drag value", () => {
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

    // Simulate drag through multiple values (30 %, 50 %, 70 %)
    act(() => { fireEvent.change(input, { target: { value: "30" } }); });
    act(() => { fireEvent.change(input, { target: { value: "50" } }); });
    act(() => { fireEvent.change(input, { target: { value: "70" } }); });
    expect(mockOnOverrideChange).not.toHaveBeenCalled();

    // One mouse-up → one POST with the final value
    act(() => { fireEvent.mouseUp(input); });
    expect(mockOnOverrideChange).toHaveBeenCalledTimes(1);
    expect(mockOnOverrideChange).toHaveBeenCalledWith({ pv_irradiance: 0.7 });
  });

  it("blend-back speed: label updates immediately on drag; no POST until mouse-up", () => {
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

    // Not sent to server yet
    expect(mockOnOverrideChange).not.toHaveBeenCalled();
  });
});

// ─── Blend-back speed — ignores server updates once locally set ───────────────

describe("AssetRightSection — blend-back speed holds local value across prop updates", () => {
  beforeEach(() => {
    mockSchemaData.mockReturnValue({ pv: pvSchema });
  });

  afterEach(() => {
    mockSchemaData.mockReturnValue({});
  });

  it("blend-back: local drag value persists after commit and across server prop updates", () => {
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

    const input = getSchemaSliderInput("pv_irradiance_alpha");

    // User drags blend-back speed to 0.5 and commits
    act(() => { fireEvent.change(input, { target: { value: "0.5" } }); });
    expect(screen.getByText(/Blend-back Speed: 0\.50/)).toBeInTheDocument();

    act(() => { fireEvent.mouseUp(input); });
    expect(mockOnOverrideChange).toHaveBeenCalledWith({ pv_irradiance_alpha: 0.5 });

    // Blend-back speed retains local value after commit (unlike pv_irradiance)
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

  it("irradiance: reverts to live sim value after commit (contrast with blend-back)", () => {
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

    const input = getSchemaSliderInput("pv_irradiance");

    act(() => { fireEvent.change(input, { target: { value: "30" } }); });
    expect(screen.getByText(/Irradiance Override: 30 %/)).toBeInTheDocument();

    act(() => { fireEvent.mouseUp(input); });

    // Unlike blend-back, irradiance releases local hold and follows live sim
    expect(screen.getByText(/Irradiance Override: 80 %/)).toBeInTheDocument();
  });

  it("alpha and irradiance commits are independent — no shared cancellation", () => {
    // Regression test for the shared-timer bug: adjusting irradiance used to
    // cancel a pending alpha POST within the 300ms debounce window.
    // With per-event commits (onChangeCommitted), each control fires its own
    // POST independently on mouse-up.
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

    // Set alpha and commit
    act(() => {
      fireEvent.change(getSchemaSliderInput("pv_irradiance_alpha"), { target: { value: "0.99" } });
    });
    act(() => { fireEvent.mouseUp(getSchemaSliderInput("pv_irradiance_alpha")); });

    // Set irradiance and commit
    act(() => {
      fireEvent.change(getSchemaSliderInput("pv_irradiance"), { target: { value: "70" } });
    });
    act(() => { fireEvent.mouseUp(getSchemaSliderInput("pv_irradiance")); });

    // Both POSTs must have fired independently
    expect(mockOnOverrideChange).toHaveBeenCalledTimes(2);
    expect(mockOnOverrideChange).toHaveBeenCalledWith({ pv_irradiance_alpha: 0.99 });
    expect(mockOnOverrideChange).toHaveBeenCalledWith({ pv_irradiance: 0.7 });
  });
});
