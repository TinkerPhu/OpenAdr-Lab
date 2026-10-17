import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect, vi, beforeEach } from "vitest";
import type { ComfortCurveCost, ComfortCurveResponse } from "../api/types";
import { ComfortCurveCard } from "../components/devices/ComfortCurveCard";
import { makeSim } from "./fixtures/sim";

/** A default roster: the curve assets come first in profile order. */
const roster = makeSim({ ev: {}, heater: {}, battery: {}, pv: {} });

const mockCurveData = vi.fn(
  (): ComfortCurveResponse => ({ source: "default", rates: [] }),
);
const NO_COST: ComfortCurveCost = {
  max_cost_eur: null,
  energy_kwh: null,
  planned_cost_eur: 0,
  spend_points: null,
  spend_from_fill: null,
};
const mockSetSpend = vi.fn(async () => ({}));
let mockSetSpendError: Error | null = null;
const mockCost = vi.fn(
  (): ComfortCurveCost => ({ ...NO_COST }),
);
const mockSetCurve = vi.fn(async () => ({}));
let mockSetError: Error | null = null;
const mockDeleteCurve = vi.fn(async () => ({}));

vi.mock("../api/hooks", () => ({
  useSignals: () => ({ data: undefined }),
  useComfortCurve: () => ({ data: mockCurveData() }),
  useComfortCurveCost: (assetId: string) => ({ data: assetId === "ev" ? mockCost() : undefined }),
  useSetComfortCurveSpend: () => ({
    mutateAsync: mockSetSpend,
    isPending: false,
    error: mockSetSpendError,
  }),
  useSetComfortCurve: () => ({
    mutateAsync: mockSetCurve,
    isPending: false,
    error: mockSetError,
  }),
  useDeleteComfortCurve: () => ({ mutateAsync: mockDeleteCurve, isPending: false }),
}));

describe("ComfortCurveCard", () => {
  beforeEach(() => {
    mockCost.mockReturnValue({ ...NO_COST });
    mockSetCurve.mockClear();
    mockSetSpend.mockClear();
    mockSetSpendError = null;
    mockSetError = null;
    mockDeleteCurve.mockClear();
    mockCurveData.mockReturnValue({
      source: "default",
      rates: [
        { fill: 0.8, max_marginal_price: 0.3, max_marginal_co2: 0 },
        { fill: 1.0, max_marginal_price: 0.1, max_marginal_co2: 0 },
      ],
    });
  });

  // W6: every asset a curve prices, whatever its id, in roster order; nothing else.
  it("offers every curve-priced asset of the roster, by its own id", () => {
    const twoBatteries = makeSim({
      "bat-a": { asset_type: "battery" },
      pv: {},
      "bat-b": { asset_type: "battery" },
      ev: {},
    });
    render(<ComfortCurveCard sim={twoBatteries} />);
    const options = Array.from(
      screen.getByTestId("comfort-asset-select").querySelectorAll("option"),
    ).map((o) => o.value);
    expect(options).toEqual(["bat-a", "bat-b", "ev"]);
  });

  it("renders the effective curve rows and the source chip", () => {
    render(<ComfortCurveCard sim={roster} />);
    expect(screen.getByTestId("comfort-source-chip")).toHaveTextContent("default");
    const rows = screen.getAllByTestId(/^comfort-row-/);
    expect(rows).toHaveLength(2);
    expect(screen.getByTestId("comfort-fill-0")).toHaveValue(80);
    expect(screen.getByTestId("comfort-bid-0")).toHaveValue(0.3);
    expect(screen.getByTestId("comfort-co2-0")).toHaveValue(0);
  });

  it("edits the CO2 bid field and saves it as part of the override", async () => {
    const user = userEvent.setup();
    render(<ComfortCurveCard sim={roster} />);
    const co2 = screen.getByTestId("comfort-co2-0");
    await user.clear(co2);
    await user.type(co2, "250");
    await user.click(screen.getByTestId("comfort-save-btn"));
    expect(mockSetCurve).toHaveBeenCalledWith({
      assetId: "ev",
      rates: [
        { fill: 0.8, max_marginal_price: 0.3, max_marginal_co2: 250 },
        { fill: 1.0, max_marginal_price: 0.1, max_marginal_co2: 0 },
      ],
    });
  });

  it("renders a curve chart preview alongside the editable rows", () => {
    render(<ComfortCurveCard sim={roster} />);
    expect(screen.getByTestId("comfort-curve-chart")).toBeInTheDocument();
  });

  it("shows an empty-state placeholder instead of a chart when there are no points", () => {
    mockCurveData.mockReturnValue({ source: "default", rates: [] });
    render(<ComfortCurveCard sim={roster} />);
    expect(screen.getByTestId("comfort-curve-chart-empty")).toBeInTheDocument();
    expect(screen.queryByTestId("comfort-curve-chart")).not.toBeInTheDocument();
  });

  it("saves an edited curve as an override", async () => {
    const user = userEvent.setup();
    render(<ComfortCurveCard sim={roster} />);
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

  it("labels the EV's fill axis as state of charge and explains the marginal bid", () => {
    render(<ComfortCurveCard sim={roster} />);
    // `ev-comfort-piecewise-core`: for an EV the curve is a property of the
    // battery's fullness, and each point is a bid for the *next* kWh.
    expect(screen.getAllByLabelText("State of charge (%)")).toHaveLength(2);
    expect(screen.getByText(/next/)).toBeInTheDocument();
    expect(screen.getByText(/must not rise as fill increases/)).toBeInTheDocument();
  });

  it("surfaces the API's rejection message when a curve is refused", () => {
    mockSetError = new Error(
      "point 1: max_marginal_price 0.4 rises above the previous 0.1 — bids must not increase with fill",
    );
    render(<ComfortCurveCard sim={roster} />);
    expect(screen.getByTestId("comfort-save-error")).toHaveTextContent(
      "bids must not increase with fill",
    );
  });

  it("shows no rejection message when the last save succeeded", () => {
    render(<ComfortCurveCard sim={roster} />);
    expect(screen.queryByTestId("comfort-save-error")).not.toBeInTheDocument();
  });

  it("reset restores the default via DELETE and is enabled only on overrides", async () => {
    mockCurveData.mockReturnValue({
      source: "override",
      rates: [{ fill: 0.9, max_marginal_price: 0.5, max_marginal_co2: 0 }],
    });
    const user = userEvent.setup();
    render(<ComfortCurveCard sim={roster} />);
    expect(screen.getByTestId("comfort-source-chip")).toHaveTextContent("override");
    await user.click(screen.getByTestId("comfort-reset-btn"));
    expect(mockDeleteCurve).toHaveBeenCalledWith("ev");
  });

  // comfort-curve-cost-and-spend-input, Part A: what the curve can cost beside what the plan spends.
  it("shows the most the curve can spend beside the plan's spend for the EV", () => {
    mockCost.mockReturnValue({ ...NO_COST, max_cost_eur: 4, energy_kwh: 20, planned_cost_eur: 1.4 });
    render(<ComfortCurveCard sim={roster} />);
    const cost = screen.getByTestId("comfort-cost");
    expect(cost).toHaveTextContent("Most this curve can spend");
    expect(cost).toHaveTextContent("€4.00");
    expect(cost).toHaveTextContent("20.0 kWh");
    expect(cost).toHaveTextContent("Planned (current plan)");
    expect(cost).toHaveTextContent("€1.40");
    expect(screen.queryByTestId("comfort-cost-forced")).toBeNull();
  });

  it("shows no cost for an asset that does not price energy by a curve", async () => {
    const user = userEvent.setup();
    render(<ComfortCurveCard sim={roster} />);
    const select = screen.getByTestId("comfort-asset-select").querySelector("select");
    await user.selectOptions(select as HTMLSelectElement, "heater");
    expect(screen.queryByTestId("comfort-cost")).toBeNull();
  });

  it("says charging was forced when the plan spends more than the curve can", () => {
    mockCost.mockReturnValue({ ...NO_COST, max_cost_eur: 4, energy_kwh: 20, planned_cost_eur: 5.2 });
    render(<ComfortCurveCard sim={roster} />);
    expect(screen.getByTestId("comfort-cost-forced")).toHaveTextContent("deadline");
  });

  // Part B: the same curve as the total you will pay, from the current charge.
  const SPEND_COST: ComfortCurveCost = {
    max_cost_eur: 2.5,
    energy_kwh: 30,
    planned_cost_eur: 0,
    spend_from_fill: 0.2,
    spend_points: [
      { fill: 0.6, cumulative_max_cost_eur: 2.0, max_marginal_co2: 0 },
      { fill: 0.8, cumulative_max_cost_eur: 2.5, max_marginal_co2: 0 },
    ],
  };

  it("offers no spend view for an asset without spend points", () => {
    render(<ComfortCurveCard sim={roster} />);
    expect(screen.queryByTestId("comfort-mode-spend")).toBeNull();
  });

  it("shows the curve as the total you will pay, from the current charge", async () => {
    const user = userEvent.setup();
    mockCost.mockReturnValue(SPEND_COST);
    render(<ComfortCurveCard sim={roster} />);
    expect(screen.getByTestId("comfort-mode-spend")).toBeInTheDocument();
    await user.click(screen.getByTestId("comfort-mode-spend"));
    expect(screen.getByTestId("comfort-spend-fill-0")).toHaveValue(60);
    expect(screen.getByTestId("comfort-spend-eur-0")).toHaveValue(2);
    expect(screen.getByTestId("comfort-spend-eur-1")).toHaveValue(2.5);
    expect(screen.getByTestId("comfort-spend-basis")).toHaveTextContent("from 20 % (current charge)");
  });

  it("saves edited spend rows through the spend route", async () => {
    const user = userEvent.setup();
    mockCost.mockReturnValue(SPEND_COST);
    render(<ComfortCurveCard sim={roster} />);
    await user.click(screen.getByTestId("comfort-mode-spend"));
    const eur = screen.getByTestId("comfort-spend-eur-1");
    await user.clear(eur);
    await user.type(eur, "2.4");
    await user.click(screen.getByTestId("comfort-save-btn"));
    expect(mockSetSpend).toHaveBeenCalledWith({
      assetId: "ev",
      points: [
        { fill: 0.6, cumulative_max_cost_eur: 2, max_marginal_co2: 0 },
        { fill: 0.8, cumulative_max_cost_eur: 2.4, max_marginal_co2: 0 },
      ],
    });
    expect(mockSetCurve).not.toHaveBeenCalled();
  });

  it("shows the reason the spend route gave", async () => {
    const user = userEvent.setup();
    mockCost.mockReturnValue(SPEND_COST);
    mockSetSpendError = new Error("point 1: adds 0.200 per kWh");
    render(<ComfortCurveCard sim={roster} />);
    await user.click(screen.getByTestId("comfort-mode-spend"));
    expect(screen.getByTestId("comfort-save-error")).toHaveTextContent("point 1: adds 0.200 per kWh");
  });
});
