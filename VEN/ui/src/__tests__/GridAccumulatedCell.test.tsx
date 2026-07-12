/**
 * GridAccumulatedCell tests:
 * 1. nowMs advancement (page-level integration)
 * 2. buildStackedFromAllTimelines positional-zip logic (unit)
 */
import { render, screen, act } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, it, expect, vi, afterEach } from "vitest";
import { ControllerPage } from "../pages/Controller";
import { buildStackedFromAllTimelines } from "../components/controller/GridAccumulatedCell";
import type { SimSnapshot } from "../api/types";
import type { AssetTimelinePoint } from "../components/controller/types";

// ─── Minimal sim fixture ─────────────────────────────────────────────────────

const baseSim: SimSnapshot = {
  ts: "2026-01-01T10:00:00Z",
  grid: { net_power_w: 0, voltage_v: 230, import_kwh: 0, export_kwh: 0 },
  assets: {},
};

// ─── Mock StackedAreaChart — expose nowMs as a DOM data attribute ─────────────

vi.mock("../components/controller/charts/StackedAreaChart", () => ({
  StackedAreaChart: ({ nowMs }: { nowMs: number }) => (
    <div data-testid="stacked-area-chart" data-now-ms={String(nowMs)} />
  ),
}));

// ─── Mutable: changed between renders to simulate React Query data refresh ───

let allTimelinesData: { zones: unknown[]; timelines: Record<string, unknown> } = { zones: [], timelines: {} };

vi.mock("../api/hooks", () => ({
  useSignals: () => ({ data: undefined }),
  useSim: () => ({ data: baseSim, isLoading: false, isError: false, refetch: vi.fn() }),
  useTariffs: () => ({ data: [], refetch: vi.fn() }),
  useRequests: () => ({ data: [], refetch: vi.fn() }),
  useSimInject: () => ({ data: {} }),
  useSetSimInject: () => ({ mutate: vi.fn() }),
  useResetAssetSoc: () => ({ mutate: vi.fn() }),
  useAllTimelines: () => ({ data: allTimelinesData, refetch: vi.fn() }),
  useSimSchema: () => ({ data: {} }),
}));

// ─── Helpers ─────────────────────────────────────────────────────────────────

function makeQueryClient() {
  return new QueryClient({ defaultOptions: { queries: { retry: false } } });
}

function pt(ts: number, power_kw: number): AssetTimelinePoint {
  return { ts, values: { power_kw } };
}

function nullPt(ts: number): AssetTimelinePoint {
  return { ts, values: null };
}

// ─── Tests: nowMs advancement ────────────────────────────────────────────────

describe("GridAccumulatedCell — now line position", () => {
  afterEach(() => {
    vi.useRealTimers();
    allTimelinesData = { zones: [], timelines: {} };
  });

  it("nowMs passed to StackedAreaChart advances when allTimelines data refreshes after time has passed", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-01-01T10:00:00.000Z"));

    const qc = makeQueryClient();
    const { rerender } = render(
      <QueryClientProvider client={qc}>
        <ControllerPage />
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
    allTimelinesData = { zones: [], timelines: {} };
    act(() => {
      rerender(
        <QueryClientProvider client={qc}>
          <ControllerPage />
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

// ─── Tests: buildStackedFromAllTimelines positional zip ──────────────────────

describe("buildStackedFromAllTimelines — positional zip", () => {
  it("zips asset values by position across aligned arrays", () => {
    const timelines: Record<string, AssetTimelinePoint[]> = {
      ev: [pt(1000, 3.0), pt(2000, 4.0)],
      battery: [pt(1000, -1.0), pt(2000, -2.0)],
      base_load: [pt(1000, 1.5), pt(2000, 1.5)],
      grid: [pt(1000, 3.5), pt(2000, 3.5)],
    };
    const result = buildStackedFromAllTimelines(timelines);

    expect(result).toHaveLength(2);
    expect(result[0].ts).toBe(1000);
    expect(result[0].ev_pos).toBe(3.0);
    expect(result[0].ev_neg).toBe(0);
    expect(result[0].battery_pos).toBe(0);
    expect(result[0].battery_neg).toBe(-1.0);
    expect(result[0].base_load_pos).toBe(1.5);
    expect(result[0].gridPowerKw).toBe(3.5);

    expect(result[1].ts).toBe(2000);
    expect(result[1].ev_pos).toBe(4.0);
    expect(result[1].battery_neg).toBe(-2.0);
  });

  it("returns empty array when no known assets have data", () => {
    const result = buildStackedFromAllTimelines({});
    expect(result).toHaveLength(0);
  });

  it("handles values: null entries as zero contribution", () => {
    const timelines: Record<string, AssetTimelinePoint[]> = {
      ev: [pt(1000, 5.0), nullPt(2000)],
      base_load: [pt(1000, 1.0), nullPt(2000)],
      grid: [pt(1000, 6.0), nullPt(2000)],
    };
    const result = buildStackedFromAllTimelines(timelines);

    expect(result).toHaveLength(2);
    // First point: normal values
    expect(result[0].ev_pos).toBe(5.0);
    expect(result[0].gridPowerKw).toBe(6.0);
    // Second point: null values → zero/null
    expect(result[1].ev_pos).toBe(0);
    expect(result[1].ev_neg).toBe(0);
    expect(result[1].base_load_pos).toBe(0);
    expect(result[1].gridPowerKw).toBeNull();
  });

  it("treats missing assets as zero at every position", () => {
    const timelines: Record<string, AssetTimelinePoint[]> = {
      ev: [pt(1000, 2.0)],
      // no heater, pv, battery, base_load, grid
    };
    const result = buildStackedFromAllTimelines(timelines);

    expect(result).toHaveLength(1);
    expect(result[0].ev_pos).toBe(2.0);
    expect(result[0].heater_pos).toBe(0);
    expect(result[0].pv_pos).toBe(0);
    expect(result[0].battery_pos).toBe(0);
    expect(result[0].base_load_pos).toBe(0);
    expect(result[0].gridPowerKw).toBeNull();
  });
});
