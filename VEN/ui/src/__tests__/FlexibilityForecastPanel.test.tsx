import { render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, it, expect, vi } from "vitest";
import { FlexibilityForecastPanel } from "../components/controller/FlexibilityForecastPanel";
import type { AssetCapability, AssetForecast } from "../api/types";

// WP-T6 (docs/history/project_journal.md, search "WP-T"): wires GET /capability/:asset_id
// and GET /forecast.

const mockCapabilities = vi.fn((): Array<{ data?: AssetCapability }> => []);
const mockForecasts = vi.fn((): AssetForecast[] => []);

vi.mock("../api/hooks", () => ({
  useAssetCapabilities: () => mockCapabilities(),
  useAssetForecasts: () => ({ data: mockForecasts() }),
}));

function renderPanel(assetIds: string[]) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <FlexibilityForecastPanel assetIds={assetIds} />
    </QueryClientProvider>,
  );
}

describe("FlexibilityForecastPanel", () => {
  it("renders nothing when there are no assets", () => {
    const { container } = renderPanel([]);
    expect(container).toBeEmptyDOMElement();
  });

  it("renders a dash for an asset with no capability/forecast data yet", () => {
    mockCapabilities.mockReturnValue([{ data: undefined }]);
    mockForecasts.mockReturnValue([]);
    renderPanel(["ev"]);

    const row = screen.getByTestId("flexibility-row-ev");
    expect(row).toHaveTextContent("—");
  });

  it("renders capability min/max and forecast source when present", () => {
    mockCapabilities.mockReturnValue([
      {
        data: {
          max_import_kw: 7.4,
          min_import_kw: 1.4,
          max_export_kw: 0,
          min_export_kw: 0,
          is_fixed: false,
          adjustability: "STEPLESS",
          power_steps_kw: [],
          key_features: [],
        },
      },
    ]);
    mockForecasts.mockReturnValue([
      {
        asset_id: "ev",
        updated_at: "2026-07-18T10:00:00Z",
        source: "OPTIMIZATION",
        confidence: 0.9,
        power_kw: [3.2, 3.2, 0],
        soc: null,
        availability_windows: null,
      },
    ]);
    renderPanel(["ev"]);

    const row = screen.getByTestId("flexibility-row-ev");
    expect(row).toHaveTextContent("7.40 kW");
    expect(row).toHaveTextContent("1.40 kW");
    expect(row).not.toHaveTextContent("3.20 kW"); // forecast number dropped
    expect(row).not.toHaveTextContent("confidence");
    expect(screen.getByTestId("forecast-source-ev")).toHaveTextContent("Optimization");
  });

  it("marks a fixed-capability asset", () => {
    mockCapabilities.mockReturnValue([
      {
        data: {
          max_import_kw: 2.0,
          min_import_kw: 2.0,
          max_export_kw: 0,
          min_export_kw: 0,
          is_fixed: true,
          adjustability: "STEPPED",
          power_steps_kw: [0, 1.25, 2.5],
          key_features: [],
        },
      },
    ]);
    mockForecasts.mockReturnValue([]);
    renderPanel(["heater"]);

    expect(screen.getByTestId("flexibility-row-heater")).toHaveTextContent("(fixed)");
  });

  it("shows how the asset says it answers a setpoint", () => {
    // R-81: the arbiter projects an asset through its declared response, so
    // an operator has to be able to see what that declaration says.
    mockCapabilities.mockReturnValue([
      {
        data: {
          max_import_kw: 7.4,
          min_import_kw: 1.4,
          max_export_kw: 0,
          min_export_kw: 0,
          is_fixed: false,
          adjustability: "STEPLESS",
          power_steps_kw: [],
          key_features: [],
          step_rule: "CONTINUOUS",
          snap_to_zero_below_kw: 1.4,
          power_next_tick_kw: 7.4,
        },
      },
    ]);
    mockForecasts.mockReturnValue([]);
    renderPanel(["ev"]);

    const cell = screen.getByTestId("adjustability-ev");
    expect(cell).toHaveTextContent("off below 1.4 kW");
    expect(cell).toHaveTextContent("drawing 7.4 kW until the command lands");
  });

  it("rounds the power the asset draws until the command lands to one decimal", () => {
    mockCapabilities.mockReturnValue([
      {
        data: {
          max_import_kw: 7.4,
          min_import_kw: 1.4,
          max_export_kw: 0,
          min_export_kw: 0,
          is_fixed: false,
          adjustability: "STEPLESS",
          power_steps_kw: [],
          key_features: [],
          snap_to_zero_below_kw: 1.4,
          power_next_tick_kw: 4.472824277831694,
        },
      },
    ]);
    mockForecasts.mockReturnValue([]);
    renderPanel(["ev"]);

    expect(screen.getByTestId("adjustability-ev")).toHaveTextContent(
      "off below 1.4 kW, drawing 4.5 kW until the command lands",
    );
  });

  it("shows the heater's Min import distinct from Max import (tiered asset)", () => {
    mockCapabilities.mockReturnValue([
      {
        data: {
          max_import_kw: 6.0,
          min_import_kw: 3.0,
          max_export_kw: 0,
          min_export_kw: 0,
          is_fixed: false,
          adjustability: "STEPPED",
          power_steps_kw: [0, 3, 6],
          key_features: [],
        },
      },
    ]);
    mockForecasts.mockReturnValue([]);
    renderPanel(["heater"]);

    const row = screen.getByTestId("flexibility-row-heater");
    expect(row).toHaveTextContent("6.00 kW");
    expect(row).toHaveTextContent("3.00 kW");
  });

  // ── BL-27: Adjustability column ───────────────────────────────────────────

  it("shows the discrete levels for a Stepped asset", () => {
    mockCapabilities.mockReturnValue([
      {
        data: {
          max_import_kw: 2.5,
          min_import_kw: 1.25,
          max_export_kw: 0,
          min_export_kw: 0,
          is_fixed: false,
          adjustability: "STEPPED",
          power_steps_kw: [0, 1.25, 2.5],
          key_features: [],
        },
      },
    ]);
    mockForecasts.mockReturnValue([]);
    renderPanel(["heater"]);

    expect(screen.getByTestId("adjustability-heater")).toHaveTextContent(
      "Stepped (0/1.25/2.5 kW)",
    );
  });

  it("shows a plain classification label with no levels for a Stepless asset", () => {
    mockCapabilities.mockReturnValue([
      {
        data: {
          max_import_kw: 7.4,
          min_import_kw: 1.4,
          max_export_kw: 0,
          min_export_kw: 0,
          is_fixed: false,
          adjustability: "STEPLESS",
          power_steps_kw: [],
          key_features: [],
        },
      },
    ]);
    mockForecasts.mockReturnValue([]);
    renderPanel(["ev"]);

    const cell = screen.getByTestId("adjustability-ev");
    expect(cell).toHaveTextContent("Stepless");
    expect(cell).not.toHaveTextContent("kW)");
  });
  // ── Asset key features: generated by each asset, rendered without a per-kind branch ──

  it("shows each asset's own key features in small print under its name", () => {
    mockCapabilities.mockReturnValue([
      {
        data: {
          max_import_kw: 0,
          min_import_kw: 0,
          max_export_kw: -5,
          min_export_kw: 0,
          is_fixed: false,
          adjustability: "CROPPABLE",
          power_steps_kw: [],
          key_features: [{ label: "peak power", value: "5.00 kW" }],
        },
      },
      {
        data: {
          max_import_kw: 2,
          min_import_kw: 0,
          max_export_kw: 0,
          min_export_kw: 0,
          is_fixed: false,
          adjustability: "NONE",
          power_steps_kw: [],
          key_features: [
            { label: "avg", value: "-" },
            { label: "max", value: "-" },
          ],
        },
      },
    ]);
    mockForecasts.mockReturnValue([]);
    renderPanel(["pv", "base_load"]);

    expect(screen.getByTestId("key-feature-pv-peak power")).toHaveTextContent("peak power: 5.00 kW");
    expect(screen.getByTestId("key-feature-base_load-avg")).toHaveTextContent("avg: -");
    expect(screen.getByTestId("key-feature-base_load-max")).toHaveTextContent("max: -");
  });

  it("renders no key-feature lines for an asset that declares none", () => {
    mockCapabilities.mockReturnValue([
      {
        data: {
          max_import_kw: 6.0,
          min_import_kw: 3.0,
          max_export_kw: 0,
          min_export_kw: 0,
          is_fixed: false,
          adjustability: "STEPPED",
          power_steps_kw: [0, 3, 6],
          key_features: [],
        },
      },
    ]);
    mockForecasts.mockReturnValue([]);
    renderPanel(["heater"]);

    expect(screen.queryByTestId(/^key-feature-heater-/)).toBeNull();
  });
});
