/**
 * GridHeadroomCell (BL-43) — left-section text, pin/expand controls, and chart prop threading.
 * Mocks SiteHeadroomChart (same pattern as GridTariffCell.test.tsx mocking TariffEnvelopeChart)
 * so this stays a unit test of the cell, not a recharts integration test. The mock records
 * its props so the "Move commitment start" tests can drive its cursor callbacks directly.
 */
import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { GridHeadroomCell } from "../components/controller/GridHeadroomCell";
import type {
  CapacityCurvesResponse,
  SiteFlexibilityEnvelope,
  SiteFlexibilitySample,
  SiteFlexibilityForecastSlot,
} from "../api/types";

const { chartProps, curvesAtCalls, curvesAtResponse } = vi.hoisted(() => ({
  chartProps: [] as Array<Record<string, unknown>>,
  curvesAtCalls: [] as Array<number | null>,
  curvesAtResponse: { current: null as ((startMs: number) => CapacityCurvesResponse) | null },
}));

vi.mock("../components/controller/charts/SiteHeadroomChart", () => ({
  SiteHeadroomChart: (props: {
    history: SiteFlexibilitySample[];
    forecast: SiteFlexibilityForecastSlot[];
    xAxisTickIntervalMinutes?: number;
  }) => {
    chartProps.push(props);
    return (
      <div
        data-testid="site-headroom-chart"
        data-history-len={String(props.history.length)}
        data-forecast-len={String(props.forecast.length)}
        data-tick-interval-minutes={String(props.xAxisTickIntervalMinutes)}
      />
    );
  },
}));

vi.mock("../api/hooks", () => ({
  useCapacityCurvesAt: (startMs: number | null) => {
    curvesAtCalls.push(startMs);
    const respond = curvesAtResponse.current;
    return { data: startMs === null || respond === null ? undefined : respond(startMs) };
  },
}));

const envelope: SiteFlexibilityEnvelope = {
  ts: "2026-01-01T10:00:00Z",
  up_kw: 5.5,
  down_kw: 2.25,
  up_duration_s: 3600,
  down_duration_s: null,
};

const history: SiteFlexibilitySample[] = [
  { ts: "2026-01-01T09:59:00Z", up_kw: 5.0, down_kw: 2.0 },
  { ts: "2026-01-01T10:00:00Z", up_kw: 5.5, down_kw: 2.25 },
];

const forecast: SiteFlexibilityForecastSlot[] = [
  { ts: "2026-01-01T10:05:00Z", up_kw: 4.8, down_kw: 3.1 },
];

describe("GridHeadroomCell", () => {
  it("shows placeholders when no envelope is available yet", () => {
    render(
      <GridHeadroomCell
        envelope={undefined}
        history={[]}
        forecast={[]}
        gridTimeline={[]}
        nowMs={Date.now()}
        extended={false}
        pinned={false}
        onTogglePin={vi.fn()}
      />
    );
    expect(screen.getByTestId("grid-headroom-cell")).toBeInTheDocument();
    expect(screen.getByTestId("headroom-up-kw")).toHaveTextContent("—");
    expect(screen.getByTestId("headroom-down-kw")).toHaveTextContent("—");
  });

  it("renders current up_kw/down_kw and duration from the live envelope", () => {
    render(
      <GridHeadroomCell
        envelope={envelope}
        history={history}
        forecast={[]}
        gridTimeline={[]}
        nowMs={Date.now()}
        extended={false}
        pinned={false}
        onTogglePin={vi.fn()}
      />
    );
    expect(screen.getByTestId("headroom-up-kw")).toHaveTextContent("5.50 kW");
    expect(screen.getByTestId("headroom-up-kw")).toHaveTextContent("1.0 h");
    expect(screen.getByTestId("headroom-down-kw")).toHaveTextContent("2.25 kW");
    expect(screen.getByTestId("headroom-down-kw")).toHaveTextContent("—");
  });

  it("threads the history array through to SiteHeadroomChart", () => {
    render(
      <GridHeadroomCell
        envelope={envelope}
        history={history}
        forecast={[]}
        gridTimeline={[]}
        nowMs={Date.now()}
        extended={false}
        pinned={false}
        onTogglePin={vi.fn()}
      />
    );
    expect(screen.getByTestId("site-headroom-chart")).toHaveAttribute("data-history-len", "2");
  });

  it("threads the forecast array through to SiteHeadroomChart", () => {
    render(
      <GridHeadroomCell
        envelope={envelope}
        history={history}
        forecast={forecast}
        gridTimeline={[]}
        nowMs={Date.now()}
        extended={false}
        pinned={false}
        onTogglePin={vi.fn()}
      />
    );
    expect(screen.getByTestId("site-headroom-chart")).toHaveAttribute("data-forecast-len", "1");
  });

  it("passes the default tick interval to SiteHeadroomChart when not extended", () => {
    render(
      <GridHeadroomCell
        envelope={envelope}
        history={history}
        forecast={[]}
        gridTimeline={[]}
        nowMs={Date.now()}
        extended={false}
        pinned={false}
        onTogglePin={vi.fn()}
      />
    );
    expect(screen.getByTestId("site-headroom-chart")).toHaveAttribute("data-tick-interval-minutes", "10");
  });

  it("passes the extended tick interval to SiteHeadroomChart when extended", () => {
    render(
      <GridHeadroomCell
        envelope={envelope}
        history={history}
        forecast={[]}
        gridTimeline={[]}
        nowMs={Date.now()}
        extended={true}
        pinned={false}
        onTogglePin={vi.fn()}
      />
    );
    expect(screen.getByTestId("site-headroom-chart")).toHaveAttribute("data-tick-interval-minutes", "30");
  });

  it("calls onTogglePin when the pin button is clicked", async () => {
    const user = userEvent.setup();
    const onTogglePin = vi.fn();
    render(
      <GridHeadroomCell
        envelope={envelope}
        history={history}
        forecast={[]}
        gridTimeline={[]}
        nowMs={Date.now()}
        extended={false}
        pinned={false}
        onTogglePin={onTogglePin}
      />
    );
    await user.click(screen.getByTestId("grid-headroom-cell-pin-btn"));
    expect(onTogglePin).toHaveBeenCalledOnce();
  });
});

describe("GridHeadroomCell — Move commitment start", () => {
  const MIN = 60_000;
  const nowMs = Date.parse("2026-01-01T10:00:00Z");
  // Remaining plan slots every 15 min from 10:15.
  const slotStartsMs = [1, 2, 3, 4].map((i) => nowMs + i * 15 * MIN);
  const slots: SiteFlexibilityForecastSlot[] = slotStartsMs.map((ms) => ({
    ts: new Date(ms).toISOString(),
    up_kw: -3,
    down_kw: 4,
  }));
  const curvesAnchoredAt = (startMs: number): CapacityCurvesResponse => {
    const start = new Date(startMs).toISOString();
    return {
      start,
      import: { direction: "import", start, steps: [{ elapsed_s: 0, power_kw: 4 }] },
      export: { direction: "export", start, steps: [{ elapsed_s: 0, power_kw: -3 }] },
    };
  };
  const perTick = curvesAnchoredAt(nowMs);
  // Stands in for the server: it, not the cell, snaps a requested time down to its plan slot.
  const serverAnswer = (requestedMs: number): CapacityCurvesResponse =>
    curvesAnchoredAt(slotStartsMs.filter((s) => s <= requestedMs).pop() ?? nowMs);

  const lastChart = () => chartProps[chartProps.length - 1] as {
    capacity: CapacityCurvesResponse | null;
    commitmentStartMs: number | null;
    onCursorMove?: (tsMs: number | null) => void;
    onCursorDoubleClick?: (tsMs: number) => void;
  };
  const hover = (tsMs: number | null) => {
    act(() => lastChart().onCursorMove?.(tsMs));
    act(() => vi.advanceTimersByTime(200)); // past the hover debounce
  };
  const doubleClick = (tsMs: number) => act(() => lastChart().onCursorDoubleClick?.(tsMs));
  const selectMode = (name: RegExp) => act(() => screen.getByRole("button", { name }).click());

  const renderCell = () =>
    render(
      <GridHeadroomCell
        envelope={envelope}
        history={[]}
        forecast={slots}
        capacity={perTick}
        gridTimeline={[]}
        nowMs={nowMs}
        extended={false}
        pinned={false}
        onTogglePin={vi.fn()}
      />
    );

  beforeEach(() => {
    vi.useFakeTimers();
    chartProps.length = 0;
    curvesAtCalls.length = 0;
    curvesAtResponse.current = serverAnswer;
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("defaults to Values: the cursor never moves the curves", () => {
    renderCell();
    hover(slotStartsMs[2] + 5 * MIN);
    expect(lastChart().capacity).toBe(perTick);
    expect(lastChart().commitmentStartMs).toBeNull();
    expect(curvesAtCalls.every((c) => c === null)).toBe(true);
  });

  it("hovering a future time anchors the curves at the plan slot it falls in", () => {
    renderCell();
    selectMode(/move commitment start/i);
    hover(slotStartsMs[1] + 7 * MIN);
    expect(curvesAtCalls).toContain(slotStartsMs[1] + 7 * MIN);
    expect(lastChart().capacity?.start).toBe(new Date(slotStartsMs[1]).toISOString());
    expect(lastChart().commitmentStartMs).toBe(slotStartsMs[1]);
    expect(screen.getByTestId("commitment-start-caption")).toHaveTextContent(/Commitment start: /);
  });

  it("hovering at or before now keeps the curves at now", () => {
    renderCell();
    selectMode(/move commitment start/i);
    hover(nowMs - 10 * MIN);
    expect(lastChart().capacity).toBe(perTick);
    expect(lastChart().commitmentStartMs).toBeNull();
  });

  it("sends the cursor time itself, only once the cursor rests", () => {
    renderCell();
    selectMode(/move commitment start/i);
    act(() => lastChart().onCursorMove?.(slotStartsMs[2] + MIN));
    act(() => vi.advanceTimersByTime(50));
    act(() => lastChart().onCursorMove?.(slotStartsMs[2] + 6 * MIN));
    act(() => vi.advanceTimersByTime(50));
    expect(curvesAtCalls.filter((c) => c !== null)).toEqual([]);
    act(() => vi.advanceTimersByTime(200));
    expect(new Set(curvesAtCalls.filter((c) => c !== null))).toEqual(new Set([slotStartsMs[2] + 6 * MIN]));
  });

  it("a double-click holds the start until the next double-click", () => {
    renderCell();
    selectMode(/move commitment start/i);
    hover(slotStartsMs[1] + MIN);
    doubleClick(slotStartsMs[1] + MIN);
    hover(slotStartsMs[3] + MIN);
    expect(lastChart().commitmentStartMs).toBe(slotStartsMs[1]);
    expect(screen.getByTestId("commitment-start-caption")).toHaveTextContent(/held/i);
    doubleClick(slotStartsMs[3] + MIN);
    hover(slotStartsMs[3] + 2 * MIN);
    expect(lastChart().commitmentStartMs).toBe(slotStartsMs[3]);
  });

  it("switching back to Values releases a held start", () => {
    renderCell();
    selectMode(/move commitment start/i);
    doubleClick(slotStartsMs[2] + MIN);
    selectMode(/values/i);
    expect(lastChart().capacity).toBe(perTick);
    expect(lastChart().commitmentStartMs).toBeNull();
    selectMode(/move commitment start/i);
    hover(null);
    expect(lastChart().commitmentStartMs).toBeNull();
  });

  it("says so when the server anchored the curves at now instead (no active plan)", () => {
    curvesAtResponse.current = () => perTick;
    renderCell();
    selectMode(/move commitment start/i);
    hover(slotStartsMs[1] + MIN);
    expect(lastChart().commitmentStartMs).toBeNull();
    expect(screen.getByTestId("commitment-start-caption")).toHaveTextContent(/no active plan/i);
  });
});
