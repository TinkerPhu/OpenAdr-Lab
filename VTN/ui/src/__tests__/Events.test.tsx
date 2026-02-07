import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { BrowserRouter } from "react-router-dom";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { EventsPage } from "../pages/Events";

const mockEvents = [
  { id: "e1", programID: "p1", eventName: "Peak Event", createdDateTime: "2026-01-01", intervals: [{ id: 0 }] },
  { id: "e2", programID: "p1", eventName: "Off-Peak Event", createdDateTime: "2026-01-02", intervals: [] },
  { id: "e3", programID: "p2", eventName: "EV Charge", createdDateTime: "2026-01-03", intervals: [] },
];

const useEventsMock = vi.fn(() => ({
  data: mockEvents,
  dataUpdatedAt: Date.now(),
}));

vi.mock("../api/hooks", () => ({
  useEvents: () => useEventsMock(),
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

  it("filters events by search query", async () => {
    renderEvents();
    const search = screen.getByTestId("events-search");
    await userEvent.type(search, "Peak Event");
    expect(screen.getByTestId("event-row-e1")).toBeVisible();
    expect(screen.getByTestId("event-row-e2")).toBeVisible();
    expect(screen.queryByTestId("event-row-e3")).not.toBeInTheDocument();
  });

  it("shows empty state when no events match", async () => {
    renderEvents();
    const search = screen.getByTestId("events-search");
    await userEvent.type(search, "zzz-no-match");
    expect(screen.getByTestId("events-empty")).toBeVisible();
    expect(screen.getByTestId("events-empty")).toHaveTextContent("No events");
  });

  it("opens JSON dialog when row is clicked", async () => {
    renderEvents();
    await userEvent.click(screen.getByTestId("event-row-e1"));
    expect(screen.getByTestId("json-dialog")).toBeVisible();
    expect(screen.getByTestId("json-dialog-title")).toHaveTextContent("Event: Peak Event");
  });

  it("shows empty state when data is empty", () => {
    useEventsMock.mockReturnValue({ data: [], dataUpdatedAt: Date.now() });
    renderEvents();
    expect(screen.getByTestId("events-empty")).toBeVisible();
  });
});
