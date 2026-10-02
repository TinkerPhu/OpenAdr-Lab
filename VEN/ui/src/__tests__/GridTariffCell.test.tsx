/**
 * GridTariffCell — now line position test
 *
 * Asserts that the nowMs passed to TariffEnvelopeChart stays current as time passes
 * and allTimelines data refreshes, rather than being frozen at page-mount time.
 *
 * Also asserts that TariffEnvelopeChart receives hoursBack and hoursForward so the
 * chart can use a fixed domain [nowMs - hoursBack*h, nowMs + hoursForward*h]
 * instead of Recharts auto-domain, which may exclude nowMs when past data is absent.
 */
import { render, screen, act } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, it, expect, vi, afterEach } from "vitest";
import { ControllerPage } from "../pages/Controller";
import type { SimSnapshot } from "../api/types";

// ─── Minimal sim fixture ─────────────────────────────────────────────────────

const baseSim: SimSnapshot = {
  ts: "2026-01-01T10:00:00Z",
  grid: { net_power_w: 0, voltage_v: 230, import_kwh: 0, export_kwh: 0 },
  assets: {},
};

// ─── Mock TariffEnvelopeChart — expose nowMs, hoursBack, hoursForward as DOM data attrs

vi.mock("../components/controller/charts/TariffEnvelopeChart", () => ({
  TariffEnvelopeChart: ({
    nowMs,
    hoursBack,
    hoursForward,
  }: {
    nowMs: number;
    hoursBack?: number;
    hoursForward?: number;
  }) => (
    <div
      data-testid="tariff-envelope-chart"
      data-now-ms={String(nowMs)}
      data-hours-back={String(hoursBack ?? "")}
      data-hours-forward={String(hoursForward ?? "")}
    />
  ),
}));

// GridRatesCell also renders on the Controller page — stub its chart too so this
// file's assertions stay scoped to GridTariffCell/TariffEnvelopeChart.
vi.mock("../components/controller/charts/GridRatesChart", () => ({
  GridRatesChart: () => <div data-testid="grid-rates-chart" />,
}));

// ─── Mutable: changed between renders to simulate React Query data refresh ───

let allTimelinesData: { zones: unknown[]; timelines: Record<string, unknown[]> } = { zones: [], timelines: {} };
let tariffsData: unknown[] = [];

vi.mock("../api/hooks", () => ({
  useSignals: () => ({ data: undefined }),
  useSim: () => ({ data: baseSim, isLoading: false, isError: false, refetch: vi.fn() }),
  useTariffs: () => ({ data: tariffsData, refetch: vi.fn() }),
  useCapacitySchedule: () => ({ data: [], refetch: vi.fn() }),
  useRequests: () => ({ data: [], refetch: vi.fn() }),
  useSimInject: () => ({ data: {} }),
  useSetSimInject: () => ({ mutate: vi.fn() }),
  useResetAssetSoc: () => ({ mutate: vi.fn() }),
  useAllTimelines: () => ({ data: allTimelinesData, refetch: vi.fn() }),
  useSimSchema: () => ({ data: {} }),
  useFlexibility: () => ({ data: undefined }),
  useFlexibilityHistory: () => ({ data: [] }),
  useFlexibilityForecast: () => ({ data: [] }),
  useCapacityCurves: () => ({ data: null }),
  useCapacityCurvesAt: () => ({ data: undefined }),
  // WP-T6 (docs/history/project_journal.md, search "WP-T"): wires GET /capability/:asset_id, GET /forecast.
  useAssetCapabilities: () => [],
  useAssetForecasts: () => ({ data: [] }),
}));

// ─── Helpers ─────────────────────────────────────────────────────────────────

function makeQueryClient() {
  return new QueryClient({ defaultOptions: { queries: { retry: false } } });
}

// ─── Tests ───────────────────────────────────────────────────────────────────

describe("GridTariffCell — now line position", () => {
  afterEach(() => {
    vi.useRealTimers();
    allTimelinesData = { zones: [], timelines: {} };
    tariffsData = [];
  });

  it("nowMs passed to TariffEnvelopeChart advances when timeline data refreshes after time has passed", () => {
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
      screen.getByTestId("tariff-envelope-chart").getAttribute("data-now-ms")!,
      10
    );
    expect(initialNowMs).toBe(t0);

    // Simulate 5 minutes passing (user keeps the page open)
    act(() => void vi.advanceTimersByTime(5 * 60 * 1000));

    // Simulate useAllTimelines React Query refetch: swap in a new object reference.
    allTimelinesData = { zones: [], timelines: {} };
    act(() => {
      rerender(
        <QueryClientProvider client={qc}>
          <ControllerPage />
        </QueryClientProvider>
      );
    });

    const updatedNowMs = parseInt(
      screen.getByTestId("tariff-envelope-chart").getAttribute("data-now-ms")!,
      10
    );
    const t5 = new Date("2026-01-01T10:05:00.000Z").getTime();

    // nowMs must have advanced to T+5min, not remain frozen at the page-mount value T+0
    expect(updatedNowMs).toBeGreaterThanOrEqual(t5);
  });

  it("nowMs does NOT advance when only tariffsData changes (nowMs comes from page-level allTimelines)", () => {
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
      screen.getByTestId("tariff-envelope-chart").getAttribute("data-now-ms")!,
      10
    );
    expect(initialNowMs).toBe(t0);

    // Advance time 5 minutes then refresh ONLY tariffsData — allTimelinesData stays the same object
    act(() => void vi.advanceTimersByTime(5 * 60 * 1000));
    tariffsData = []; // new array reference, but allTimelinesData unchanged
    act(() => {
      rerender(
        <QueryClientProvider client={qc}>
          <ControllerPage />
        </QueryClientProvider>
      );
    });

    const nowMsAfter = parseInt(
      screen.getByTestId("tariff-envelope-chart").getAttribute("data-now-ms")!,
      10
    );

    // nowMs must remain at T0, not advance — tariffs no longer drive nowMs
    expect(nowMsAfter).toBe(t0);
  });

  it("TariffEnvelopeChart receives hoursBack >= 1 and hoursForward >= 1 for fixed-domain coverage", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-01-01T10:00:00.000Z"));

    const qc = makeQueryClient();
    render(
      <QueryClientProvider client={qc}>
        <ControllerPage />
      </QueryClientProvider>
    );

    const chart = screen.getByTestId("tariff-envelope-chart");
    const hoursBack = parseFloat(chart.getAttribute("data-hours-back") ?? "0");
    const hoursForward = parseFloat(chart.getAttribute("data-hours-forward") ?? "0");

    // Both past and future must be covered so the now line is never outside the domain
    expect(hoursBack).toBeGreaterThanOrEqual(1.0);
    expect(hoursForward).toBeGreaterThanOrEqual(1.0);
  });
});

describe("GridTariffCell — expanded state via global button", () => {
  afterEach(() => {
    vi.useRealTimers();
    allTimelinesData = { zones: [], timelines: {} };
    tariffsData = [];
  });

  // The page opens with the time range already extended (`Controller.tsx`:
  // `useState(true)`), so the global button's first click *collapses* to the
  // default window rather than expanding to 48 h. These two tests used to hard-code
  // "click once = 48 h", which silently became wrong when that default flipped on
  // 2026-09-30, and stayed red because the UI suite was not run with that change.
  // They now assert the toggle's *behaviour* — that it moves between the two
  // documented windows and back — so a future change to which state comes first
  // cannot make them wrong again, only a change to what the windows mean.
  it("global expand button toggles the tariff chart between the two documented windows", async () => {
    const user = userEvent.setup();
    const qc = makeQueryClient();
    render(
      <QueryClientProvider client={qc}>
        <ControllerPage />
      </QueryClientProvider>
    );

    const forward = () =>
      parseFloat(
        screen.getByTestId("tariff-envelope-chart").getAttribute("data-hours-forward") ?? "-1"
      );
    const back = () =>
      parseFloat(
        screen.getByTestId("tariff-envelope-chart").getAttribute("data-hours-back") ?? "-1"
      );

    // DEFAULT_WINDOW and EXTENDED_WINDOW (chartLayout.ts) differ only in the
    // forward reach: 1 h vs the full 48 h plan horizon. Back is 1 h in both.
    const initial = forward();
    expect([1, 48]).toContain(initial);
    expect(back()).toBe(1);

    await user.click(screen.getByTestId("global-time-range-extend-btn"));
    const toggled = forward();
    expect(toggled).toBe(initial === 48 ? 1 : 48);
    expect(back()).toBe(1);
  });

  it("the tariff chart returns to its starting window when the button is clicked twice", async () => {
    const user = userEvent.setup();
    const qc = makeQueryClient();
    render(
      <QueryClientProvider client={qc}>
        <ControllerPage />
      </QueryClientProvider>
    );

    const forward = () =>
      parseFloat(
        screen.getByTestId("tariff-envelope-chart").getAttribute("data-hours-forward") ?? "-1"
      );

    // The previous version asserted only ">= 1" after two clicks, which both windows
    // satisfy — so it passed whatever the toggle did, including nothing. Asserting a
    // return to the *starting* value is what actually pins idempotency.
    const initial = forward();
    const btn = screen.getByTestId("global-time-range-extend-btn");
    await user.click(btn);
    expect(forward()).not.toBe(initial);
    await user.click(btn);
    expect(forward()).toBe(initial);
  });
});
