import { fireEvent, render, screen } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { DraftEditLayer } from "../components/DraftEditLayer";
import { draftFromPattern, type SignalDraft } from "../utils/signalDraft";
import { buildSignalRegistry } from "../utils/signalRegistry";
import type { SignalBand } from "../api/types";

const REGISTRY = buildSignalRegistry([
  { payloadType: "PRICE", units: "KWH", currency: "EUR" },
  { payloadType: "EXPORT_PRICE", units: "KWH", currency: "EUR" },
]);
const MIDNIGHT = Date.parse("2026-10-18T00:00:00Z");
const HOUR = 3_600_000;
const DAY = 24 * HOUR;

/** A d3-like linear scale: callable, with `invert`, which is all the layer asks of it. */
function linear(d0: number, d1: number, r0: number, r1: number) {
  const f = (v: number) => r0 + ((v - d0) / (d1 - d0)) * (r1 - r0);
  f.invert = (p: number) => d0 + ((p - r0) / (r1 - r0)) * (d1 - d0);
  return f;
}
// 1 px per minute across 48 h; 0..0.6 EUR/kWh over 300..0 px (500 px per EUR).
const X = linear(MIDNIGHT, MIDNIGHT + 2 * DAY, 0, 2880);
const Y = linear(0, 0.6, 300, 0);
const chartProps = {
  xAxisMap: { 0: { scale: X } },
  yAxisMap: { tariff: { scale: Y } },
  offset: { left: 0, top: 0, width: 2880, height: 300 },
};

function day(): SignalDraft {
  return draftFromPattern(
    { programID: "p", eventName: "tou", duration: "P9999Y" },
    Array.from({ length: 24 }, (_, h) => ({
      start: new Date(MIDNIGHT + h * HOUR).toISOString(),
      end: new Date(MIDNIGHT + (h + 1) * HOUR).toISOString(),
      payloads: [{ type: "PRICE", value: 0.3 }],
    })),
    REGISTRY,
  );
}

/** What the BFF preview returns for the day repeated twice. */
function bands(): SignalBand[] {
  return Array.from({ length: 48 }, (_, i) => ({
    from: new Date(MIDNIGHT + i * HOUR).toISOString(),
    to: new Date(MIDNIGHT + (i + 1) * HOUR).toISOString(),
    eventID: "draft",
    eventName: "d",
    payloadType: "PRICE",
    value: 0.3,
  }));
}

function show(over: { draft?: SignalDraft; bands?: SignalBand[] } = {}) {
  const onChange = vi.fn();
  render(
    <svg>
      <DraftEditLayer
        {...chartProps}
        axisId="tariff"
        draft={over.draft ?? day()}
        registry={REGISTRY}
        bands={over.bands ?? bands()}
        onChange={onChange}
      />
    </svg>,
  );
  return onChange;
}

/** The draft the layer most recently reported. */
const lastCall = (mock: ReturnType<typeof vi.fn>): SignalDraft => {
  const calls = mock.mock.calls;
  return calls[calls.length - 1][0];
};

const drag = (testId: string, to: { clientX?: number; clientY?: number }) => {
  const el = screen.getByTestId(testId);
  fireEvent.pointerDown(el, { pointerId: 1, clientX: 0, clientY: 0 });
  fireEvent.pointerMove(el, { pointerId: 1, ...to });
};

describe("DraftEditLayer", () => {
  it("draws one value handle per previewed band", () => {
    show();
    expect(screen.getAllByTestId(/^handle-value-/)).toHaveLength(48);
  });

  it("dragging a handle sets that row's value, snapped to the registry step", () => {
    const onChange = show();
    drag("handle-value-17", { clientY: 300 - 0.3047 * 500 });
    const out: SignalDraft = lastCall(onChange);
    expect(out.intervals[17].values.PRICE).toBe(0.3);
    drag("handle-value-17", { clientY: 300 - 0.4 * 500 });
    expect((lastCall(onChange) as SignalDraft).intervals[17].values.PRICE).toBe(0.4);
  });

  it("a drag on the second repetition edits the pattern row, not a copy", () => {
    const onChange = show();
    drag("handle-value-41", { clientY: 300 - 0.45 * 500 });
    const out: SignalDraft = lastCall(onChange);
    expect(out.intervals).toHaveLength(24);
    expect(out.intervals[17].values.PRICE).toBe(0.45);
    expect(out.intervals[16].values.PRICE).toBe(0.3);
  });

  it("clamps to the signal's range", () => {
    const onChange = show();
    drag("handle-value-3", { clientY: -5000 });
    expect((lastCall(onChange) as SignalDraft).intervals[3].values.PRICE).toBe(
      REGISTRY.PRICE.max,
    );
  });

  it("shows the value being set while dragging, and not afterwards", () => {
    show();
    expect(screen.queryByTestId("drag-readout")).toBeNull();
    drag("handle-value-5", { clientY: 300 - 0.42 * 500 });
    expect(screen.getByTestId("drag-readout")).toHaveTextContent("0.42");
    expect(screen.getByTestId("drag-readout")).toHaveTextContent("EUR/kWh");
    fireEvent.pointerUp(screen.getByTestId("handle-value-5"), { pointerId: 1 });
    expect(screen.queryByTestId("drag-readout")).toBeNull();
  });

  it("refuses an edge drag that would leave a row with no length", () => {
    const onChange = show();
    // The 05:00 edge dragged to 06:40 snaps to 07:00, past the end of the 05:00-06:00 row.
    drag("handle-edge-5", { clientX: 6 * 60 + 40 });
    expect(onChange).not.toHaveBeenCalled();
  });

  describe("with a two-hour row (05:00-07:00) the edge has room to move", () => {
    function twoHourRowDraft(): SignalDraft {
      const d = day();
      d.intervals.splice(6, 1);
      d.intervals[5] = { ...d.intervals[5], end: new Date(MIDNIGHT + 7 * HOUR).toISOString() };
      return d;
    }
    /** The bands of that 23-row pattern, drawn twice. */
    function twoHourRowBands(): SignalBand[] {
      const d = twoHourRowDraft();
      const period = Date.parse(d.intervals[22].end) - MIDNIGHT;
      return [0, period].flatMap((shift) =>
        d.intervals.map((r) => ({
          from: new Date(Date.parse(r.start) + shift).toISOString(),
          to: new Date(Date.parse(r.end) + shift).toISOString(),
          eventID: "draft",
          eventName: "d",
          payloadType: "PRICE",
          value: 0.3,
        })),
      );
    }

    it("moves the shared edge to the snapped time", () => {
      const onChange = show({ draft: twoHourRowDraft(), bands: twoHourRowBands() });
      // Row 5 starts at 05:00 (x = 300 px); drag its start edge to 06:00 (x = 360 px).
      drag("handle-edge-5", { clientX: 360 });
      const out: SignalDraft = lastCall(onChange);
      expect(out.intervals[4].end).toBe(new Date(MIDNIGHT + 6 * HOUR).toISOString());
      expect(out.intervals[5].start).toBe(new Date(MIDNIGHT + 6 * HOUR).toISOString());
      expect(out.intervals).toHaveLength(23);
    });

    it("an edge drag on the second repetition moves the pattern's edge", () => {
      const d = twoHourRowDraft();
      const onChange = show({ draft: d, bands: twoHourRowBands() });
      const period = Date.parse(d.intervals[22].end) - MIDNIGHT; // 23 h
      const x = (period + 6 * HOUR) / 60_000; // 06:00 within the second repetition
      drag("handle-edge-28", { clientX: x });
      const out: SignalDraft = lastCall(onChange);
      expect(out.intervals[4].end).toBe(new Date(MIDNIGHT + 6 * HOUR).toISOString());
    });
  });

  it("has no edge handle on the first row, where there is no edge to move", () => {
    show();
    expect(screen.queryByTestId("handle-edge-0")).toBeNull();
    expect(screen.getByTestId("handle-edge-1")).toBeInTheDocument();
  });

  it("has no handle for a band whose value was not readable", () => {
    const b = bands();
    b[2] = { ...b[2], value: null };
    show({ bands: b });
    expect(screen.queryByTestId("handle-value-2")).toBeNull();
    expect(screen.getAllByTestId(/^handle-value-/)).toHaveLength(47);
  });

  it("has no handle for a band of a signal the draft does not carry", () => {
    const b = bands().map((x) => ({ ...x, payloadType: "EXPORT_PRICE" }));
    show({ bands: b });
    expect(screen.queryAllByTestId(/^handle-value-/)).toHaveLength(0);
  });
});
