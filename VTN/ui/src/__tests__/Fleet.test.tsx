import { render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { BrowserRouter } from "react-router-dom";
import { describe, it, expect, vi } from "vitest";
import { FleetPage } from "../pages/Fleet";

const mockLive = {
  source: "live" as const,
  vens: [
    {
      venName: "ven-1",
      netPowerW: 4000,
      state: "online",
      receivedAt: new Date().toISOString(),
    },
    {
      venName: "ven-2",
      netPowerW: -1500,
      state: "online",
      receivedAt: new Date().toISOString(),
    },
    {
      venName: "ven-3",
      netPowerW: null,
      state: "offline",
      receivedAt: new Date().toISOString(),
    },
  ],
  fleet: { netPowerW: 2500, contributingVens: 2, knownVens: 3 },
};

const mockHistory = {
  source: "raw" as const,
  from: "2026-09-22T09:00:00Z",
  to: "2026-09-22T10:00:00Z",
  stepSeconds: 60,
  vens: [],
  fleet: [
    { ts: "2026-09-22T09:00:00Z", netPowerW: 1000, contributingVens: 2 },
    { ts: "2026-09-22T09:01:00Z", netPowerW: 2000, contributingVens: 2 },
  ],
};

// The chart has its own test file; here it only has to not be recharts.
vi.mock("../components/FleetPowerChart", () => ({
  FleetPowerChart: ({ windowMinutes }: { windowMinutes: number }) => (
    <div data-testid="fleet-chart-stub">{windowMinutes}</div>
  ),
}));

vi.mock("../api/hooks", () => ({
  useFleetPower: vi.fn(() => ({ data: mockLive, isError: false })),
  useFleetHistory: vi.fn(() => ({ data: mockHistory, isError: false })),
  // The page also hosts the reactions card; it has its own test file, so here
  // it only has to be idle rather than absent.
  useFleetReactions: vi.fn(() => ({ data: undefined, isError: false })),
}));

function renderFleet() {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={queryClient}>
      <BrowserRouter>
        <FleetPage />
      </BrowserRouter>
    </QueryClientProvider>,
  );
}

describe("FleetPage", () => {
  it("shows the fleet total in kW", () => {
    renderFleet();
    expect(screen.getByTestId("fleet-total")).toHaveTextContent("2.50 kW");
  });

  /* The total is only as complete as the VENs behind it. Showing how many
   * contributed is what lets a reader judge it rather than trust it. */
  it("says how many VENs the total is built from", () => {
    renderFleet();
    expect(screen.getByTestId("fleet-contributors")).toHaveTextContent("2 of 3");
  });

  /* A VEN that has not reported must not appear as 0.00 kW: that is a claim
   * about what it is drawing, and we do not have one. */
  it("shows a silent VEN as no reading rather than zero", () => {
    renderFleet();
    const row = screen.getByTestId("fleet-ven-ven-3");
    expect(row).toHaveTextContent("—");
    expect(row).not.toHaveTextContent("0.00 kW");
  });

  it("keeps an exporting VEN negative", () => {
    renderFleet();
    expect(screen.getByTestId("fleet-ven-ven-2")).toHaveTextContent("-1.50 kW");
  });

  /* A last-will `offline` is what distinguishes a dead VEN from a quiet one,
   * so it has to reach the page. */
  it("shows the last-will state of a VEN that died", () => {
    renderFleet();
    expect(screen.getByTestId("fleet-ven-ven-3")).toHaveTextContent("offline");
  });

  it("draws the per-VEN chart over the selected window", () => {
    renderFleet();
    expect(screen.getByTestId("fleet-chart-stub")).toHaveTextContent("60");
  });

  /* The window picker is what makes "did it react" and "what did today look
   * like" the same page rather than two. */
  it("offers the operator a window to choose", async () => {
    renderFleet();
    expect(screen.getByTestId("fleet-window-select")).toBeInTheDocument();
  });

  /* A store that is not connected is not an empty hour. Saying so beats an
   * empty chart that looks like a quiet fleet. */
  it("explains a missing history instead of drawing an empty chart", async () => {
    const { useFleetHistory } = await import("../api/hooks");
    vi.mocked(useFleetHistory).mockReturnValueOnce({
      data: undefined,
      isError: true,
    } as ReturnType<typeof useFleetHistory>);
    renderFleet();
    expect(screen.getByTestId("fleet-history-error")).toBeVisible();
    expect(screen.queryByTestId("sparkline")).not.toBeInTheDocument();
  });
});
