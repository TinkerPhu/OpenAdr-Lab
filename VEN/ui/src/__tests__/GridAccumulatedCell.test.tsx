/**
 * GridAccumulatedCell — now line position test
 *
 * Asserts that the nowMs passed to StackedAreaChart stays current as time passes
 * and allTimelines data refreshes, rather than being frozen at page-mount time.
 */
import { render, screen, act } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, it, expect, vi, afterEach } from "vitest";
import { ControllerV2Page } from "../pages/ControllerV2";
import type { SimSnapshot } from "../api/types";

// ─── Minimal sim fixture ─────────────────────────────────────────────────────

const baseSim: SimSnapshot = {
  ts: "2026-01-01T10:00:00Z",
  grid: { net_power_w: 0, import_w: 0, export_w: 0, voltage_v: 230, import_kwh: 0, export_kwh: 0 },
  assets: {},
};

// ─── Mock StackedAreaChart — expose nowMs as a DOM data attribute ─────────────

vi.mock("../components/controller-v2/charts/StackedAreaChart", () => ({
  StackedAreaChart: ({ nowMs }: { nowMs: number }) => (
    <div data-testid="stacked-area-chart" data-now-ms={String(nowMs)} />
  ),
}));

// ─── Mutable: changed between renders to simulate React Query data refresh ───

let allTimelinesData: Record<string, unknown> = {};

vi.mock("../api/hooks", () => ({
  useSim: () => ({ data: baseSim, isLoading: false, isError: false, refetch: vi.fn() }),
  useTariffs: () => ({ data: [], refetch: vi.fn() }),
  useRequests: () => ({ data: [], refetch: vi.fn() }),
  useSimOverride: () => ({ data: {} }),
  useSetSimOverride: () => ({ mutate: vi.fn() }),
  useAllTimelines: () => ({ data: allTimelinesData, refetch: vi.fn() }),
  useSimSchema: () => ({ data: {} }),
}));

// ─── Helpers ─────────────────────────────────────────────────────────────────

function makeQueryClient() {
  return new QueryClient({ defaultOptions: { queries: { retry: false } } });
}

// ─── Tests ───────────────────────────────────────────────────────────────────

describe("GridAccumulatedCell — now line position", () => {
  afterEach(() => {
    vi.useRealTimers();
    allTimelinesData = {};
  });

  it("nowMs passed to StackedAreaChart advances when allTimelines data refreshes after time has passed", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-01-01T10:00:00.000Z"));

    const qc = makeQueryClient();
    const { rerender } = render(
      <QueryClientProvider client={qc}>
        <ControllerV2Page />
      </QueryClientProvider>
    );

    const t0 = new Date("2026-01-01T10:00:00.000Z").getTime();
    const initialNowMs = parseInt(
      screen.getByTestId("stacked-area-chart").getAttribute("data-now-ms")!,
      10
    );
    expect(initialNowMs).toBe(t0);

    // Simulate 5 minutes passing (user keeps the page open)
    act(() => void vi.advanceTimersByTime(5 * 60 * 1000));

    // Simulate allTimelines React Query refetch: swap in a new object reference.
    // An empty record is valid input to buildStackedFromAllTimelines (yields no rows).
    allTimelinesData = {};
    act(() => {
      rerender(
        <QueryClientProvider client={qc}>
          <ControllerV2Page />
        </QueryClientProvider>
      );
    });

    const updatedNowMs = parseInt(
      screen.getByTestId("stacked-area-chart").getAttribute("data-now-ms")!,
      10
    );
    const t5 = new Date("2026-01-01T10:05:00.000Z").getTime();

    // nowMs must have advanced to T+5min, not remain frozen at the page-mount value T+0
    expect(updatedNowMs).toBeGreaterThanOrEqual(t5);
  });
});
