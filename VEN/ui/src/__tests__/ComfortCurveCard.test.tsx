import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect, vi, beforeEach } from "vitest";
import type { ComfortCurveResponse } from "../api/types";
import { ComfortCurveCard } from "../components/devices/ComfortCurveCard";

const mockCurveData = vi.fn(
  (): ComfortCurveResponse => ({ source: "default", rates: [] }),
);
const mockSetCurve = vi.fn(async () => ({}));
const mockDeleteCurve = vi.fn(async () => ({}));

vi.mock("../api/hooks", () => ({
  useSignals: () => ({ data: undefined }),
  useComfortCurve: () => ({ data: mockCurveData() }),
  useSetComfortCurve: () => ({ mutateAsync: mockSetCurve, isPending: false }),
  useDeleteComfortCurve: () => ({ mutateAsync: mockDeleteCurve, isPending: false }),
}));

describe("ComfortCurveCard", () => {
  beforeEach(() => {
    mockSetCurve.mockClear();
    mockDeleteCurve.mockClear();
    mockCurveData.mockReturnValue({
      source: "default",
      rates: [
        { fill: 0.8, max_marginal_price: 0.3, max_marginal_co2: 0 },
        { fill: 1.0, max_marginal_price: 0.1, max_marginal_co2: 0 },
      ],
    });
  });

  it("renders the effective curve rows and the source chip", () => {
    render(<ComfortCurveCard />);
    expect(screen.getByTestId("comfort-source-chip")).toHaveTextContent("default");
    const rows = screen.getAllByTestId(/^comfort-row-/);
    expect(rows).toHaveLength(2);
    expect(screen.getByTestId("comfort-fill-0")).toHaveValue(80);
    expect(screen.getByTestId("comfort-bid-0")).toHaveValue(0.3);
  });

  it("renders a curve chart preview alongside the editable rows", () => {
    render(<ComfortCurveCard />);
    expect(screen.getByTestId("comfort-curve-chart")).toBeInTheDocument();
  });

  it("shows an empty-state placeholder instead of a chart when there are no points", () => {
    mockCurveData.mockReturnValue({ source: "default", rates: [] });
    render(<ComfortCurveCard />);
    expect(screen.getByTestId("comfort-curve-chart-empty")).toBeInTheDocument();
    expect(screen.queryByTestId("comfort-curve-chart")).not.toBeInTheDocument();
  });

  it("saves an edited curve as an override", async () => {
    const user = userEvent.setup();
    render(<ComfortCurveCard />);
    const bid = screen.getByTestId("comfort-bid-0");
    await user.clear(bid);
    await user.type(bid, "0.5");
    await user.click(screen.getByTestId("comfort-save-btn"));
    expect(mockSetCurve).toHaveBeenCalledWith({
      assetId: "ev",
      rates: [
        { fill: 0.8, max_marginal_price: 0.5, max_marginal_co2: 0 },
        { fill: 1.0, max_marginal_price: 0.1, max_marginal_co2: 0 },
      ],
    });
  });

  it("reset restores the default via DELETE and is enabled only on overrides", async () => {
    mockCurveData.mockReturnValue({
      source: "override",
      rates: [{ fill: 0.9, max_marginal_price: 0.5, max_marginal_co2: 0 }],
    });
    const user = userEvent.setup();
    render(<ComfortCurveCard />);
    expect(screen.getByTestId("comfort-source-chip")).toHaveTextContent("override");
    await user.click(screen.getByTestId("comfort-reset-btn"));
    expect(mockDeleteCurve).toHaveBeenCalledWith("ev");
  });
});
