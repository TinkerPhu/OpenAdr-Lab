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
        },
      },
    ]);
    mockForecasts.mockReturnValue([]);
    renderPanel(["heater"]);

    expect(screen.getByTestId("flexibility-row-heater")).toHaveTextContent("(fixed)");
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
        },
      },
    ]);
    mockForecasts.mockReturnValue([]);
    renderPanel(["heater"]);

    const row = screen.getByTestId("flexibility-row-heater");
    expect(row).toHaveTextContent("6.00 kW");
    expect(row).toHaveTextContent("3.00 kW");
  });
});
