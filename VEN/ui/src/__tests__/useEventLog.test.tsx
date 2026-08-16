/**
 * useEventLog — GB-13: initial GET + live SSE stream
 *
 * Verifies the hook seeds its list from the initial GET, appends entries
 * arriving over the /events/log/events SSE stream, de-dups an entry that
 * arrives via both the GET and a racing SSE message, and caps the
 * client-side list at EVENT_LOG_CLIENT_CAP so a long-lived connection
 * doesn't grow the list unbounded.
 */

import React from "react";
import { renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import type { EventLogEntry } from "../api/types";

// ── Minimal EventSource mock ────────────────────────────────────────────────
// No EventSource polyfill exists in this codebase's test setup; each
// instance is tracked so tests can push messages through it manually.

class MockEventSource {
  static instances: MockEventSource[] = [];
  onmessage: ((e: { data: string }) => void) | null = null;
  url: string;

  constructor(url: string) {
    this.url = url;
    MockEventSource.instances.push(this);
  }

  emit(entry: EventLogEntry) {
    this.onmessage?.({ data: JSON.stringify(entry) });
  }

  close = vi.fn();
}

function entry(id: string, message = id): EventLogEntry {
  return { id, created_at: "2026-08-16T00:00:00Z", category: "storage", message };
}

const mockEventLog = vi.fn<() => Promise<EventLogEntry[]>>();

vi.mock("../App", () => ({
  useVenContext: () => ({
    api: { baseUrl: "http://ven-1", eventLog: mockEventLog },
  }),
}));

import { useEventLog } from "../api/hooks";

function makeWrapper(queryClient: QueryClient) {
  return function Wrapper({ children }: { children: React.ReactNode }) {
    return <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>;
  };
}

describe("useEventLog", () => {
  let queryClient: QueryClient;

  beforeEach(() => {
    MockEventSource.instances = [];
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    (globalThis as any).EventSource = MockEventSource;
    queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    mockEventLog.mockReset();
  });

  afterEach(() => {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    delete (globalThis as any).EventSource;
  });

  it("seeds the list from the initial GET", async () => {
    mockEventLog.mockResolvedValue([entry("evt-1")]);
    const { result } = renderHook(() => useEventLog(), { wrapper: makeWrapper(queryClient) });

    await waitFor(() => expect(result.current.data).toEqual([entry("evt-1")]));
  });

  it("opens an EventSource against /events/log/events", async () => {
    mockEventLog.mockResolvedValue([]);
    renderHook(() => useEventLog(), { wrapper: makeWrapper(queryClient) });

    await waitFor(() => expect(MockEventSource.instances).toHaveLength(1));
    expect(MockEventSource.instances[0].url).toBe("http://ven-1/events/log/events");
  });

  it("appends an entry received over SSE and bumps dataUpdatedAt", async () => {
    mockEventLog.mockResolvedValue([entry("evt-1")]);
    const { result } = renderHook(() => useEventLog(), { wrapper: makeWrapper(queryClient) });
    await waitFor(() => expect(result.current.data).toEqual([entry("evt-1")]));
    const updatedAtBefore = result.current.dataUpdatedAt;

    MockEventSource.instances[0].emit(entry("evt-2"));

    await waitFor(() => expect(result.current.data).toEqual([entry("evt-1"), entry("evt-2")]));
    expect(result.current.dataUpdatedAt).toBeGreaterThanOrEqual(updatedAtBefore);
  });

  it("de-dups an SSE entry that also arrived via the initial GET", async () => {
    mockEventLog.mockResolvedValue([entry("evt-1")]);
    const { result } = renderHook(() => useEventLog(), { wrapper: makeWrapper(queryClient) });
    await waitFor(() => expect(result.current.data).toEqual([entry("evt-1")]));

    MockEventSource.instances[0].emit(entry("evt-1"));

    // Give any (incorrect) duplicate-append a tick to land, then assert none did.
    await new Promise((r) => setTimeout(r, 10));
    expect(result.current.data).toEqual([entry("evt-1")]);
  });

  it("caps the client-side list at 200 entries", async () => {
    mockEventLog.mockResolvedValue(
      Array.from({ length: 200 }, (_, i) => entry(`evt-${i}`)),
    );
    const { result } = renderHook(() => useEventLog(), { wrapper: makeWrapper(queryClient) });
    await waitFor(() => expect(result.current.data).toHaveLength(200));

    MockEventSource.instances[0].emit(entry("evt-200"));

    await waitFor(() =>
      expect(result.current.data?.[result.current.data!.length - 1]).toEqual(
        entry("evt-200"),
      ),
    );
    expect(result.current.data).toHaveLength(200);
    expect(result.current.data?.[0]).toEqual(entry("evt-1"));
  });

  it("closes the EventSource on unmount", async () => {
    mockEventLog.mockResolvedValue([]);
    const { unmount } = renderHook(() => useEventLog(), { wrapper: makeWrapper(queryClient) });
    await waitFor(() => expect(MockEventSource.instances).toHaveLength(1));

    unmount();

    expect(MockEventSource.instances[0].close).toHaveBeenCalledOnce();
  });
});
