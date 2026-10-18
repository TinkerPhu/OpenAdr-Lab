import { fireEvent, render, screen } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { RecurrenceControl } from "../components/RecurrenceControl";
import type { Recurrence } from "../utils/recurrence";

const START = new Date("2026-10-18T00:00:00Z");
const DAY_END = new Date("2026-10-19T00:00:00Z");

function show(recurrence: Recurrence, over: { dstNote?: boolean; onChange?: () => void } = {}) {
  const onChange = over.onChange ?? vi.fn();
  render(
    <RecurrenceControl
      recurrence={recurrence}
      patternStart={START}
      patternEnd={DAY_END}
      dstNote={over.dstNote ?? false}
      onChange={onChange}
    />,
  );
  return onChange;
}

describe("RecurrenceControl", () => {
  it("once: no period, no duration field", () => {
    show({ kind: "once" });
    expect(screen.getByTestId("recurrence-kind")).toHaveValue("once");
    expect(screen.queryByTestId("recurrence-period")).toBeNull();
    expect(screen.queryByTestId("recurrence-duration")).toBeNull();
  });

  it("forever: says it repeats every 24 h, forever", () => {
    show({ kind: "forever" });
    expect(screen.getByTestId("recurrence-kind")).toHaveValue("forever");
    expect(screen.getByTestId("recurrence-period")).toHaveTextContent("repeats every 24 h");
    expect(screen.getByTestId("recurrence-period")).toHaveTextContent("forever");
  });

  it("span: shows the duration it repeats for, and classifies an edit", () => {
    const onChange = show({ kind: "span", duration: "P7D" });
    expect(screen.getByTestId("recurrence-kind")).toHaveValue("span");
    expect(screen.getByTestId("recurrence-duration")).toHaveValue("P7D");
    fireEvent.change(screen.getByTestId("recurrence-duration"), { target: { value: "PT12H" } });
    expect(onChange).toHaveBeenCalledWith({ kind: "truncated", duration: "PT12H" });
  });

  it("truncated: says the surplus intervals are ignored", () => {
    show({ kind: "truncated", duration: "PT12H" });
    expect(screen.getByTestId("recurrence-kind")).toHaveValue("span");
    expect(screen.getByTestId("recurrence-truncated")).toHaveTextContent(
      "Truncated to PT12H",
    );
    expect(screen.queryByTestId("recurrence-period")).toBeNull();
  });

  it("invalid: names what was written", () => {
    show({ kind: "invalid", duration: "soon" });
    expect(screen.getByTestId("recurrence-duration")).toHaveAttribute("aria-invalid", "true");
  });

  it("choosing a kind reports the matching recurrence", () => {
    const onChange = show({ kind: "once" });
    fireEvent.change(screen.getByTestId("recurrence-kind"), { target: { value: "forever" } });
    expect(onChange).toHaveBeenLastCalledWith({ kind: "forever" });
    fireEvent.change(screen.getByTestId("recurrence-kind"), { target: { value: "span" } });
    expect(onChange).toHaveBeenLastCalledWith({ kind: "span", duration: "P7D" });
    fireEvent.change(screen.getByTestId("recurrence-kind"), { target: { value: "once" } });
    expect(onChange).toHaveBeenLastCalledWith({ kind: "once" });
  });

  it("shows the clock-change note only when asked, and only for a repeating series", () => {
    show({ kind: "forever" }, { dstNote: true });
    expect(screen.getByTestId("recurrence-dst-note")).toHaveTextContent(
      "does not follow local clock time",
    );
  });

  it("no clock-change note for a series that does not repeat", () => {
    show({ kind: "once" }, { dstNote: true });
    expect(screen.queryByTestId("recurrence-dst-note")).toBeNull();
  });
});
