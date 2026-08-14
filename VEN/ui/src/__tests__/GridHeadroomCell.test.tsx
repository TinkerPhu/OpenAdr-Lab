/**
 * GridHeadroomCell (BL-43) — left-section text, pin/expand controls, and chart prop threading.
 * Mocks SiteHeadroomChart (same pattern as GridTariffCell.test.tsx mocking TariffEnvelopeChart)
 * so this stays a unit test of the cell, not a recharts integration test.
 */
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect, vi } from "vitest";
import { GridHeadroomCell } from "../components/controller/GridHeadroomCell";
import type { SiteFlexibilityEnvelope, SiteFlexibilitySample } from "../api/types";

vi.mock("../components/controller/charts/SiteHeadroomChart", () => ({
  SiteHeadroomChart: ({ history }: { history: SiteFlexibilitySample[] }) => (
    <div data-testid="site-headroom-chart" data-history-len={String(history.length)} />
  ),
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

describe("GridHeadroomCell", () => {
  it("shows placeholders when no envelope is available yet", () => {
    render(
      <GridHeadroomCell
        envelope={undefined}
        history={[]}
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
        gridTimeline={[]}
        nowMs={Date.now()}
        extended={false}
        pinned={false}
        onTogglePin={vi.fn()}
      />
    );
    expect(screen.getByTestId("site-headroom-chart")).toHaveAttribute("data-history-len", "2");
  });

  it("calls onTogglePin when the pin button is clicked", async () => {
    const user = userEvent.setup();
    const onTogglePin = vi.fn();
    render(
      <GridHeadroomCell
        envelope={envelope}
        history={history}
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
