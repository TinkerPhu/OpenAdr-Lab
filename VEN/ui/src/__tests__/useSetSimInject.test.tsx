/**
 * useSetSimInject — onSuccess side-effects
 *
 * Verifies that after a successful sim-inject POST the hook refetches both
 * ["sim"] (live snapshot) and ["timeline/all"] (forecast chart).
 * The ["timeline/all"] refetch was the missing piece that caused the
 * blend-back speed slider to not reflect in the PV forecast.
 */

import React from "react";
import { renderHook, act, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, it, expect, vi, beforeEach } from "vitest";

// Mock VenContext so the hook can be rendered without the full app tree.
vi.mock("../App", () => ({
  useVenContext: () => ({
    api: {
      postSimInject: vi.fn().mockResolvedValue({}),
    },
  }),
}));

import { useSetSimInject } from "../api/hooks";

// ── Helpers ─────────────────────────────────────────────────────────────────

function makeWrapper(queryClient: QueryClient) {
  return function Wrapper({ children }: { children: React.ReactNode }) {
    return (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
  };
}

// ── Tests ────────────────────────────────────────────────────────────────────

describe("useSetSimInject — onSuccess refetches forecast", () => {
  let queryClient: QueryClient;
  let refetchSpy: ReturnType<typeof vi.spyOn>;
  let invalidateSpy: ReturnType<typeof vi.spyOn>;

  beforeEach(() => {
    queryClient = new QueryClient({
      defaultOptions: {
        queries: { retry: false },
        mutations: { retry: false },
      },
    });
    // Spy on query-client methods to avoid actual network calls.
    refetchSpy = vi
      .spyOn(queryClient, "refetchQueries")
      .mockImplementation(async () => {});
    invalidateSpy = vi
      .spyOn(queryClient, "invalidateQueries")
      .mockImplementation(async () => {});
  });

  it("refetches ['timeline/all'] after a successful inject POST", async () => {
    const wrapper = makeWrapper(queryClient);
    const { result } = renderHook(() => useSetSimInject(), { wrapper });

    act(() => {
      result.current.mutate({ pv_irradiance_alpha: 0.5 } as never);
    });

    await waitFor(() => expect(result.current.isSuccess).toBe(true));

    const timelineCalls = (refetchSpy.mock.calls as Array<[{ queryKey: unknown[] }]>)
      .filter(([opts]) => opts?.queryKey?.[0] === "timeline/all");

    expect(
      timelineCalls.length,
      "refetchQueries should have been called with { queryKey: ['timeline/all'] }"
    ).toBeGreaterThan(0);
  });

  it("also refetches ['sim'] after a successful inject POST", async () => {
    const wrapper = makeWrapper(queryClient);
    const { result } = renderHook(() => useSetSimInject(), { wrapper });

    act(() => {
      result.current.mutate({ pv_irradiance_alpha: 0.5 } as never);
    });

    await waitFor(() => expect(result.current.isSuccess).toBe(true));

    const simCalls = (refetchSpy.mock.calls as Array<[{ queryKey: unknown[] }]>)
      .filter(([opts]) => opts?.queryKey?.[0] === "sim");

    expect(
      simCalls.length,
      "refetchQueries should have been called with { queryKey: ['sim'] }"
    ).toBeGreaterThan(0);
  });

  it("invalidates ['simInject'] after a successful inject POST", async () => {
    const wrapper = makeWrapper(queryClient);
    const { result } = renderHook(() => useSetSimInject(), { wrapper });

    act(() => {
      result.current.mutate({ pv_irradiance_alpha: 0.5 } as never);
    });

    await waitFor(() => expect(result.current.isSuccess).toBe(true));

    const injectCalls = (invalidateSpy.mock.calls as Array<[{ queryKey: unknown[] }]>)
      .filter(([opts]) => opts?.queryKey?.[0] === "simInject");

    expect(
      injectCalls.length,
      "invalidateQueries should have been called with { queryKey: ['simInject'] }"
    ).toBeGreaterThan(0);
  });
});
