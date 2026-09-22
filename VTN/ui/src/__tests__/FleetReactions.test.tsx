import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, it, expect, vi } from "vitest";
import { FleetReactions } from "../components/FleetReactions";
import { formatKw } from "../utils/power";

const mockReactions = {
  eventID: "ev-9f3",
  vensSeen: 2,
  vens: [
    {
      venName: "ven-1",
      seenAt: "2026-09-22T10:00:00Z",
      seenReceivedAt: "2026-09-22T10:00:01Z",
      modificationDateTime: "2026-09-22T09:59:00+00:00",
      replannedAt: "2026-09-22T10:00:20Z",
      replanAttributed: true,
      powerBeforeW: 4000,
      powerAfterW: 1500,
      deltaW: -2500,
    },
    {
      venName: "ven-2",
      seenAt: "2026-09-22T10:00:02Z",
      seenReceivedAt: "2026-09-22T10:00:02Z",
      modificationDateTime: null,
      replannedAt: null,
      replanAttributed: false,
      powerBeforeW: null,
      powerAfterW: null,
      deltaW: null,
    },
  ],
};

vi.mock("../api/hooks", () => ({
  useFleetReactions: vi.fn(() => ({ data: mockReactions, isError: false })),
}));

function renderReactions() {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={queryClient}>
      <FleetReactions />
    </QueryClientProvider>,
  );
}

describe("formatKw", () => {
  it("shows a missing reading as a dash, never as zero", () => {
    expect(formatKw(null)).toBe("—");
    expect(formatKw(undefined)).toBe("—");
    expect(formatKw(0)).toBe("0.00 kW");
  });

  it("keeps export negative", () => {
    expect(formatKw(-2500)).toBe("-2.50 kW");
  });
});

describe("FleetReactions", () => {
  it("asks for an event before claiming anything about one", async () => {
    const user = userEvent.setup();
    renderReactions();
    const input = screen.getByTestId("fleet-reactions-input");
    await user.type(input, "ev-9f3");
    expect(input).toHaveValue("ev-9f3");
  });

  it("shows how many VENs said they saw the event", () => {
    renderReactions();
    expect(screen.getByTestId("fleet-reactions-count")).toHaveTextContent("2 VEN(s)");
  });

  it("shows the power change across the event, signed", () => {
    renderReactions();
    expect(screen.getByTestId("fleet-reaction-ven-1")).toHaveTextContent("-2.50 kW");
  });

  /* A VEN that saw the event and never replanned is a real outcome — the one
   * an operator most wants to notice — not missing data to hide. */
  it("says so when a VEN saw the event and never replanned", () => {
    renderReactions();
    expect(screen.getByTestId("fleet-reaction-ven-2")).toHaveTextContent("never");
  });

  /* Unknown power must not render as 0.00 kW: that is a claim about the site
   * we do not have. */
  it("shows a VEN that published no power as unknown rather than zero", () => {
    renderReactions();
    const row = screen.getByTestId("fleet-reaction-ven-2");
    expect(row).toHaveTextContent("—");
    expect(row).not.toHaveTextContent("0.00 kW");
  });

  it("explains a missing store instead of an empty table", async () => {
    const { useFleetReactions } = await import("../api/hooks");
    vi.mocked(useFleetReactions).mockReturnValueOnce({
      data: undefined,
      isError: true,
    } as ReturnType<typeof useFleetReactions>);
    renderReactions();
    expect(screen.getByTestId("fleet-reactions-error")).toBeVisible();
  });

  /* "The VEN said so" and "it happened nearby in time" are different claims,
   * and only the first answers whether the event worked. */
  it("says whether a replan was attributed by the VEN or inferred from timing", () => {
    renderReactions();
    expect(screen.getByTestId("fleet-reaction-ven-1")).toHaveTextContent("named this event");
  });
});
