import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { BrowserRouter } from "react-router-dom";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { EventsPage } from "../pages/Events";

const mockEvents = [
  {
    id: "e1", programID: "p1", eventName: "emergency-load-shed", priority: 0,
    intervalPeriod: { start: "2026-02-09T14:00:00Z", duration: "PT30M" },
    createdDateTime: "2024-01-01",
    intervals: [{ id: 0, payloads: [{ type: "SIMPLE", values: [0] }] }],
  },
  {
    id: "e2", programID: "p1", eventName: "peak-shave-afternoon", priority: 3,
    createdDateTime: "2024-01-02",
    intervals: [{ id: 0, payloads: [{ type: "IMPORT_CAPACITY_LIMIT", values: [50] }] }],
  },
  {
    id: "e3", programID: "p2", eventName: "connectivity-check",
    createdDateTime: "2024-01-03",
    intervals: [{ id: 0, payloads: [{ type: "SIMPLE", values: [0] }] }],
  },
];

const mockPrograms = [
  { id: "p1", programName: "Program Alpha" },
  { id: "p2", programName: "Program Beta" },
];

const useEventsMock = vi.fn(() => ({
  data: mockEvents,
  dataUpdatedAt: Date.now(),
}));

vi.mock("../api/hooks", () => ({
  useEvents: () => useEventsMock(),
  usePrograms: () => ({ data: mockPrograms }),
}));

function renderEvents() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <BrowserRouter>
        <EventsPage />
      </BrowserRouter>
    </QueryClientProvider>,
  );
}

describe("EventsPage", () => {
  beforeEach(() => {
    useEventsMock.mockReturnValue({
      data: mockEvents,
      dataUpdatedAt: Date.now(),
    });
  });

  it("renders heading and last updated", () => {
    renderEvents();
    expect(screen.getByTestId("events-heading")).toBeVisible();
    expect(screen.getByTestId("events-heading")).toHaveTextContent("Events");
    expect(screen.getByTestId("events-last-updated")).toBeVisible();
  });

  it("renders events table with rows", () => {
    renderEvents();
    expect(screen.getByTestId("events-table")).toBeVisible();
    expect(screen.getByTestId("event-row-e1")).toBeVisible();
    expect(screen.getByTestId("event-row-e2")).toBeVisible();
    expect(screen.getByTestId("event-row-e3")).toBeVisible();
  });

  it("displays event name, program, priority, and payload type", () => {
    renderEvents();
    expect(screen.getByTestId("event-row-e1")).toHaveTextContent("emergency-load-shed");
    expect(screen.getByTestId("event-row-e1")).toHaveTextContent("Program Alpha");
    expect(screen.getByTestId("event-row-e1")).toHaveTextContent("0");
    expect(screen.getByTestId("event-row-e1")).toHaveTextContent("SIMPLE");
    expect(screen.getByTestId("event-row-e2")).toHaveTextContent("IMPORT_CAPACITY_LIMIT");
  });

  it("shows status chips", () => {
    renderEvents();
    expect(screen.getByTestId("event-status-e1")).toBeVisible();
    expect(screen.getByTestId("event-status-e3")).toHaveTextContent("immediate");
  });

  it("filters events by search query", async () => {
    renderEvents();
    const search = screen.getByTestId("events-search");
    await userEvent.type(search, "emergency");
    expect(screen.getByTestId("event-row-e1")).toBeVisible();
    expect(screen.queryByTestId("event-row-e2")).not.toBeInTheDocument();
    expect(screen.queryByTestId("event-row-e3")).not.toBeInTheDocument();
  });

  it("shows empty state when no events match", async () => {
    renderEvents();
    const search = screen.getByTestId("events-search");
    await userEvent.type(search, "zzz-no-match");
    expect(screen.getByTestId("events-empty")).toBeVisible();
    expect(screen.getByTestId("events-empty")).toHaveTextContent("No events");
  });

  it("opens detail panel when row is clicked", async () => {
    renderEvents();
    await userEvent.click(screen.getByTestId("event-row-e1"));
    expect(screen.getByTestId("event-detail-panel")).toBeVisible();
    expect(screen.getByTestId("event-detail-title")).toHaveTextContent("emergency-load-shed");
  });

  it("shows empty state when data is empty", () => {
    useEventsMock.mockReturnValue({ data: [], dataUpdatedAt: Date.now() });
    renderEvents();
    expect(screen.getByTestId("events-empty")).toBeVisible();
  });
});
