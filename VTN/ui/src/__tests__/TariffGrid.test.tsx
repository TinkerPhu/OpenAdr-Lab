import { useState } from "react";
import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect } from "vitest";
import { TariffGrid } from "../components/TariffGrid";
import { draftFromPattern, validateDraft, type SignalDraft } from "../utils/signalDraft";
import { buildSignalRegistry } from "../utils/signalRegistry";

const REGISTRY = buildSignalRegistry([
  { payloadType: "PRICE", units: "KWH", currency: "EUR" },
  { payloadType: "EXPORT_PRICE", units: "KWH", currency: "EUR" },
]);

const MIDNIGHT = Date.parse("2026-10-18T00:00:00Z");
const HOUR = 3_600_000;

/** 24 hourly rows, every PRICE cell empty. */
function emptyDay(): SignalDraft {
  return draftFromPattern(
    { programID: "p", eventName: "tou", duration: "P9999Y" },
    Array.from({ length: 24 }, (_, h) => ({
      start: new Date(MIDNIGHT + h * HOUR).toISOString(),
      end: new Date(MIDNIGHT + (h + 1) * HOUR).toISOString(),
      payloads: [{ type: "PRICE", value: null }],
    })),
    REGISTRY,
  );
}

function Harness({ initial, onDraft }: { initial: SignalDraft; onDraft?: (d: SignalDraft) => void }) {
  const [draft, setDraft] = useState(initial);
  return (
    <TariffGrid
      draft={draft}
      registry={REGISTRY}
      issues={validateDraft(draft, REGISTRY)}
      onChange={(d) => {
        setDraft(d);
        onDraft?.(d);
      }}
    />
  );
}

function valueInputs(): HTMLInputElement[] {
  return screen.getAllByTestId(/^cell-PRICE-/) as HTMLInputElement[];
}

describe("TariffGrid", () => {
  it("shows one row per interval with the registry's column header", () => {
    render(<Harness initial={emptyDay()} />);
    expect(valueInputs()).toHaveLength(24);
    expect(screen.getByRole("columnheader", { name: "Import tariff (EUR/kWh)" })).toBeInTheDocument();
  });

  it("fills 24 rows from 24 pasted lines", () => {
    render(<Harness initial={emptyDay()} />);
    const text = Array.from({ length: 24 }, (_, i) => String((i + 1) / 100)).join("\n");
    fireEvent.paste(valueInputs()[0], { clipboardData: { getData: () => text } });
    const filled = valueInputs().filter((i) => i.value !== "");
    expect(filled).toHaveLength(24);
    expect(valueInputs()[23].value).toBe("0.24");
  });

  it("edits one cell by typing", async () => {
    let last: SignalDraft | undefined;
    render(<Harness initial={emptyDay()} onDraft={(d) => (last = d)} />);
    await userEvent.type(valueInputs()[2], "0.5");
    expect(last?.intervals[2].values.PRICE).toBe(0.5);
  });

  it("marks an empty or out-of-range cell invalid", () => {
    render(<Harness initial={emptyDay()} />);
    expect(screen.getAllByTestId("cell-invalid").length).toBe(24);
    fireEvent.change(valueInputs()[0], { target: { value: "99" } });
    expect(valueInputs()[0]).toHaveAttribute("aria-invalid", "true");
    fireEvent.change(valueInputs()[0], { target: { value: "0.2" } });
    expect(valueInputs()[0]).not.toHaveAttribute("aria-invalid", "true");
  });

  it("sets the selected rows to a value", async () => {
    render(<Harness initial={emptyDay()} />);
    await userEvent.click(screen.getByTestId("row-select-2"));
    fireEvent.click(screen.getByTestId("row-select-4"), { shiftKey: true });
    fireEvent.change(screen.getByTestId("selection-value"), { target: { value: "0.1" } });
    await userEvent.click(screen.getByTestId("selection-set"));
    expect(valueInputs().slice(2, 5).map((i) => i.value)).toEqual(["0.1", "0.1", "0.1"]);
    expect(valueInputs()[5].value).toBe("");
  });

  it("fills the selection down from its first row", async () => {
    render(<Harness initial={emptyDay()} />);
    fireEvent.change(valueInputs()[1], { target: { value: "0.3" } });
    await userEvent.click(screen.getByTestId("row-select-1"));
    fireEvent.click(screen.getByTestId("row-select-3"), { shiftKey: true });
    await userEvent.click(screen.getByTestId("selection-fill-down"));
    expect(valueInputs().slice(1, 4).map((i) => i.value)).toEqual(["0.3", "0.3", "0.3"]);
    expect(within(document.body).getAllByTestId(/^cell-PRICE-/)[4].getAttribute("value")).toBe("");
  });
});
