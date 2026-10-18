import { fireEvent, render, screen } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { OperatorToolbar } from "../components/OperatorToolbar";
import { SIGNAL_OPERATORS } from "../utils/signalOperators";
import { draftFromPattern } from "../utils/signalDraft";
import { buildSignalRegistry } from "../utils/signalRegistry";

const REGISTRY = buildSignalRegistry([
  { payloadType: "PRICE", units: "KWH", currency: "EUR" },
  { payloadType: "EXPORT_PRICE", units: "KWH", currency: "EUR" },
]);
const MIDNIGHT = Date.parse("2026-10-18T00:00:00Z");
const HOUR = 3_600_000;

const draft = draftFromPattern(
  { programID: "p", eventName: "tou", duration: "P9999Y" },
  Array.from({ length: 24 }, (_, h) => ({
    start: new Date(MIDNIGHT + h * HOUR).toISOString(),
    end: new Date(MIDNIGHT + (h + 1) * HOUR).toISOString(),
    payloads: [
      { type: "PRICE", value: 0.1 },
      { type: "EXPORT_PRICE", value: 0.05 },
    ],
  })),
  REGISTRY,
);

function show(onChange = vi.fn()) {
  render(<OperatorToolbar draft={draft} registry={REGISTRY} onChange={onChange} />);
  return onChange;
}

describe("OperatorToolbar", () => {
  it("lists every operator in the table, by its label", () => {
    show();
    const labels = Array.from(
      screen.getByTestId("operator-select").querySelectorAll("option"),
    ).map((o) => o.textContent);
    expect(labels).toEqual(Object.values(SIGNAL_OPERATORS).map((o) => o.label));
  });

  it("shows the chosen operator's parameters with their defaults", () => {
    show();
    fireEvent.change(screen.getByTestId("operator-select"), { target: { value: "twoTier" } });
    for (const p of SIGNAL_OPERATORS.twoTier.params) {
      expect(screen.getByTestId(`op-param-${p.key}`)).toHaveValue(String(p.default));
    }
  });

  it("applies the operator to the chosen signal only", () => {
    const onChange = show();
    fireEvent.change(screen.getByTestId("operator-select"), { target: { value: "scale" } });
    fireEvent.change(screen.getByTestId("op-param-factor"), { target: { value: "2" } });
    fireEvent.click(screen.getByTestId("operator-apply"));
    const out = onChange.mock.calls[0][0];
    expect(out.intervals[0].values.PRICE).toBe(0.2);
    expect(out.intervals[0].values.EXPORT_PRICE).toBe(0.05);
  });

  it("applies to the second signal when it is chosen", () => {
    const onChange = show();
    fireEvent.change(screen.getByTestId("operator-signal"), { target: { value: "EXPORT_PRICE" } });
    fireEvent.change(screen.getByTestId("op-param-factor"), { target: { value: "2" } });
    fireEvent.click(screen.getByTestId("operator-apply"));
    const out = onChange.mock.calls[0][0];
    expect(out.intervals[0].values.EXPORT_PRICE).toBe(0.1);
    expect(out.intervals[0].values.PRICE).toBe(0.1);
  });

  it("will not apply a parameter that is not a number", () => {
    const onChange = show();
    fireEvent.change(screen.getByTestId("op-param-factor"), { target: { value: "abc" } });
    expect(screen.getByTestId("operator-apply")).toBeDisabled();
    fireEvent.click(screen.getByTestId("operator-apply"));
    expect(onChange).not.toHaveBeenCalled();
  });
});
