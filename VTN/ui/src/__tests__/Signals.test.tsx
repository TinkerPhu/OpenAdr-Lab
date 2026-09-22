import { render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, it, expect, vi } from "vitest";
import { SignalsPage, bandGeometry } from "../pages/Signals";
import type { FleetSignals } from "../api/types";

const mockSignals: FleetSignals = {
  from: "2026-09-22T09:00:00Z",
  to: "2026-09-22T10:00:00Z",
  vens: [
    {
      venName: "ven-1",
      bands: [
        {
          from: "2026-09-22T09:15:00Z",
          to: "2026-09-22T09:45:00Z",
          eventID: "ev-9f3",
          eventName: "peak",
          payloadType: "IMPORT_CAPACITY_LIMIT",
          value: 3,
        },
      ],
    },
    { venName: "ven-2", bands: [] },
  ],
  rejectedEvents: 0,
};

vi.mock("../api/hooks", () => ({
  useFleetSignals: vi.fn(() => ({ data: mockSignals, isError: false })),
}));

function renderSignals() {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={queryClient}>
      <SignalsPage />
    </QueryClientProvider>,
  );
}

describe("bandGeometry", () => {
  const from = Date.parse("2026-09-22T09:00:00Z");
  const to = Date.parse("2026-09-22T10:00:00Z");

  it("places a band where it actually falls in the window", () => {
    const g = bandGeometry(mockSignals.vens[0].bands[0], from, to);
    expect(g.leftPct).toBeCloseTo(25);
    expect(g.widthPct).toBeCloseTo(50);
  });

  /* A five-second dispatch inside a 24-hour window still has to be visible
   * and hoverable; a zero-width band is a signal that was sent and cannot be
   * seen. */
  it("never renders a band too narrow to see", () => {
    const g = bandGeometry(
      { ...mockSignals.vens[0].bands[0], from: "2026-09-22T09:30:00Z", to: "2026-09-22T09:30:05Z" },
      from,
      to,
    );
    expect(g.widthPct).toBeGreaterThan(0.3);
  });

  it("clips a band that starts before the window", () => {
    const g = bandGeometry(
      { ...mockSignals.vens[0].bands[0], from: "2026-09-22T08:00:00Z" },
      from,
      to,
    );
    expect(g.leftPct).toBe(0);
  });
});

describe("SignalsPage", () => {
  it("draws a row for a VEN that was under a signal", () => {
    renderSignals();
    expect(screen.getByTestId("signals-row-ven-1")).toBeVisible();
    expect(screen.getByTestId("signals-band-ven-1-0")).toBeVisible();
  });

  /* A VEN with nothing in force is not a row of empty space — it simply was
   * not targeted, and listing it would imply it was. */
  it("omits a VEN that was under nothing", () => {
    renderSignals();
    expect(screen.queryByTestId("signals-row-ven-2")).not.toBeInTheDocument();
  });

  it("says when the whole fleet was under nothing, rather than showing blank", async () => {
    const { useFleetSignals } = await import("../api/hooks");
    vi.mocked(useFleetSignals).mockReturnValueOnce({
      data: { ...mockSignals, vens: [] as FleetSignals["vens"] },
      isError: false,
    } as unknown as ReturnType<typeof useFleetSignals>);
    renderSignals();
    expect(screen.getByTestId("signals-empty")).toBeVisible();
  });

  /* Fewer bands than the VTN sent must not look like a quiet grid. */
  it("warns when an event could not be read", async () => {
    const { useFleetSignals } = await import("../api/hooks");
    vi.mocked(useFleetSignals).mockReturnValueOnce({
      data: { ...mockSignals, rejectedEvents: 2 },
      isError: false,
    } as unknown as ReturnType<typeof useFleetSignals>);
    renderSignals();
    expect(screen.getByTestId("signals-rejected")).toHaveTextContent("2 event(s)");
  });
});
