import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect, vi } from "vitest";
import { ChartLegend } from "../components/charts/ChartLegend";

const entries = [
  { key: "power", label: "Power [kW]", color: "#2196F3" },
  { key: "cost", label: "Cost rate [€/h]", color: "#212121" },
];

describe("ChartLegend", () => {
  it("renders one row per entry", () => {
    render(<ChartLegend entries={entries} isHidden={() => false} toggle={() => {}} interactive={true} />);
    expect(screen.getByText("Power [kW]")).toBeInTheDocument();
    expect(screen.getByText("Cost rate [€/h]")).toBeInTheDocument();
  });

  it("checkbox reflects hidden state", () => {
    render(
      <ChartLegend
        entries={entries}
        isHidden={(key) => key === "cost"}
        toggle={() => {}}
        interactive={true}
      />
    );
    expect(screen.getByTestId("legend-toggle-power")).toBeChecked();
    expect(screen.getByTestId("legend-toggle-cost")).not.toBeChecked();
  });

  it("renders no checkbox elements when not interactive", () => {
    render(<ChartLegend entries={entries} isHidden={() => false} toggle={() => {}} interactive={false} />);
    expect(screen.queryByTestId("legend-toggle-power")).not.toBeInTheDocument();
    expect(screen.queryByTestId("legend-toggle-cost")).not.toBeInTheDocument();
    // Labels still render — same row layout, just no checkbox.
    expect(screen.getByText("Power [kW]")).toBeInTheDocument();
  });

  it("clicking a checkbox calls toggle with the entry's key", async () => {
    const toggle = vi.fn();
    render(<ChartLegend entries={entries} isHidden={() => false} toggle={toggle} interactive={true} />);
    await userEvent.click(screen.getByTestId("legend-toggle-power"));
    expect(toggle).toHaveBeenCalledWith("power");
  });

  it("clicking the label also calls toggle (label wraps the checkbox)", async () => {
    const toggle = vi.fn();
    render(<ChartLegend entries={entries} isHidden={() => false} toggle={toggle} interactive={true} />);
    await userEvent.click(screen.getByText("Cost rate [€/h]"));
    expect(toggle).toHaveBeenCalledWith("cost");
  });
});
