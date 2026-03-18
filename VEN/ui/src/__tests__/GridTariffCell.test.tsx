/**
 * GridTariffCell — now line position test
 *
 * Asserts that the nowMs passed to TariffChart stays current as time passes
 * and timeline data refreshes, rather than being frozen at page-mount time.
 *
 * Also asserts that TariffChart receives hoursBack and hoursForward so the
 * chart can use a fixed domain [nowMs - hoursBack*h, nowMs + hoursForward*h]
 * instead of Recharts auto-domain, which may exclude nowMs when past data is absent.
 */
import { render, screen, act } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, it, expect, vi, afterEach } from "vitest";
import { ControllerV2Page } from "../pages/ControllerV2";
import type { SimSnapshot } from "../api/types";

// ─── Minimal sim fixture ─────────────────────────────────────────────────────

const baseSim: SimSnapshot = {
  ts: "2026-01-01T10:00:00Z",
  grid: { net_power_w: 0, voltage_v: 230, import_kwh: 0, export_kwh: 0 },
  assets: {},
};

// ─── Mock TariffChart — expose nowMs, hoursBack, hoursForward as DOM data attrs

vi.mock("../components/controller-v2/charts/TariffChart", () => ({
  TariffChart: ({
    nowMs,
    hoursBack,
    hoursForward,
  }: {
    nowMs: number;
    hoursBack?: number;
    hoursForward?: number;
  }) => (
    <div
      data-testid="tariff-chart"
      data-now-ms={String(nowMs)}
      data-hours-back={String(hoursBack ?? "")}
      data-hours-forward={String(hoursForward ?? "")}
    />
  ),
}));

// ─── Mutable: changed between renders to simulate React Query data refresh ───

let timelineData: unknown[] = [];

vi.mock("../api/hooks", () => ({
  useSim: () => ({ data: baseSim, isLoading: false, isError: false, refetch: vi.fn() }),
  useTariffs: () => ({ data: [], refetch: vi.fn() }),
  usePlan: () => ({ data: null, refetch: vi.fn() }),
  useRequests: () => ({ data: [], refetch: vi.fn() }),
  useSimOverride: () => ({ data: {} }),
  useSetSimOverride: () => ({ mutate: vi.fn() }),
  useTimeline: () => ({ data: timelineData }),
  useAllTimelines: () => ({ data: {}, refetch: vi.fn() }),
  useSimSchema: () => ({ data: {} }),
}));

// ─── Helpers ─────────────────────────────────────────────────────────────────

function makeQueryClient() {
  return new QueryClient({ defaultOptions: { queries: { retry: false } } });
}

// ─── Tests ───────────────────────────────────────────────────────────────────

describe("GridTariffCell — now line position", () => {
  afterEach(() => {
    vi.useRealTimers();
    timelineData = [];
  });

  it("nowMs passed to TariffChart advances when timeline data refreshes after time has passed", () => {
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
      screen.getByTestId("tariff-chart").getAttribute("data-now-ms")!,
      10
    );
    expect(initialNowMs).toBe(t0);

    // Simulate 5 minutes passing (user keeps the page open)
    act(() => void vi.advanceTimersByTime(5 * 60 * 1000));

    // Simulate useTimeline React Query refetch: swap in a new array reference.
    timelineData = [];
    act(() => {
      rerender(
        <QueryClientProvider client={qc}>
          <ControllerV2Page />
        </QueryClientProvider>
      );
    });

    const updatedNowMs = parseInt(
      screen.getByTestId("tariff-chart").getAttribute("data-now-ms")!,
      10
    );
    const t5 = new Date("2026-01-01T10:05:00.000Z").getTime();

    // nowMs must have advanced to T+5min, not remain frozen at the page-mount value T+0
    expect(updatedNowMs).toBeGreaterThanOrEqual(t5);
  });

  it("TariffChart receives hoursBack >= 1 and hoursForward >= 1 for fixed-domain coverage", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-01-01T10:00:00.000Z"));

    const qc = makeQueryClient();
    render(
      <QueryClientProvider client={qc}>
        <ControllerV2Page />
      </QueryClientProvider>
    );

    const chart = screen.getByTestId("tariff-chart");
    const hoursBack = parseFloat(chart.getAttribute("data-hours-back") ?? "0");
    const hoursForward = parseFloat(chart.getAttribute("data-hours-forward") ?? "0");

    // Both past and future must be covered so the now line is never outside the domain
    expect(hoursBack).toBeGreaterThanOrEqual(1.0);
    expect(hoursForward).toBeGreaterThanOrEqual(1.0);
  });
});
