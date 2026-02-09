import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { BrowserRouter } from "react-router-dom";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { EventsPage } from "../pages/Events";

const mockEvents = [
  { id: "e1", programID: "p1", eventName: "Peak Event", priority: 0, intervalPeriod: { start: "2026-02-09T14:00:00Z", duration: "PT4H" }, createdDateTime: "2026-01-01", intervals: [{ id: 0 }] },
  { id: "e2", programID: "p1", eventName: "Off-Peak Event", createdDateTime: "2026-01-02", intervals: [] },
  { id: "e3", programID: "p2", eventName: "EV Charge", priority: 5, createdDateTime: "2026-01-03", intervals: [] },
];

const mockPrograms = [
  { id: "p1", programName: "Program Alpha", createdDateTime: "2026-01-01" },
  { id: "p2", programName: "Program Beta", createdDateTime: "2026-01-02" },
];

const useEventsMock = vi.fn(() => ({
  data: mockEvents,
  dataUpdatedAt: Date.now(),
}));

const createMock = vi.fn();
const updateMock = vi.fn();
const deleteMock = vi.fn();

vi.mock("../api/hooks", () => ({
  useEvents: () => useEventsMock(),
  usePrograms: () => ({ data: mockPrograms, dataUpdatedAt: Date.now() }),
  useCreateEvent: () => ({ mutate: createMock, isPending: false }),
  useUpdateEvent: () => ({ mutate: updateMock, isPending: false }),
  useDeleteEvent: () => ({ mutate: deleteMock, isPending: false }),
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
    createMock.mockClear();
    updateMock.mockClear();
    deleteMock.mockClear();
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

  it("displays program name instead of program ID", () => {
    renderEvents();
    expect(screen.getByTestId("event-row-e1")).toHaveTextContent("Program Alpha");
    expect(screen.getByTestId("event-row-e3")).toHaveTextContent("Program Beta");
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

  it("displays priority and start time columns", () => {
    renderEvents();
    expect(screen.getByTestId("event-row-e1")).toHaveTextContent("0");
    expect(screen.getByTestId("event-row-e3")).toHaveTextContent("5");
  });

  it("opens create event dialog", async () => {
    renderEvents();
    await userEvent.click(screen.getByTestId("create-event-btn"));
    expect(screen.getByTestId("event-form-dialog")).toBeVisible();
  });

  it("opens edit dialog with pre-filled data", async () => {
    renderEvents();
    await userEvent.click(screen.getByTestId("edit-event-e1"));
    expect(screen.getByTestId("event-name-input")).toHaveValue("Peak Event");
  });

  it("opens confirm dialog on delete click", async () => {
    renderEvents();
    await userEvent.click(screen.getByTestId("delete-event-e1"));
    expect(screen.getByTestId("confirm-dialog")).toBeVisible();
  });

  it("calls delete on confirm", async () => {
    renderEvents();
    await userEvent.click(screen.getByTestId("delete-event-e1"));
    await userEvent.click(screen.getByTestId("confirm-dialog-ok"));
    expect(deleteMock).toHaveBeenCalledWith(
      "e1",
      expect.objectContaining({ onSuccess: expect.any(Function) }),
    );
  });

  it("saves updated eventName on edit", async () => {
    renderEvents();
    await userEvent.click(screen.getByTestId("edit-event-e1"));
    const nameInput = screen.getByTestId("event-name-input");
    await userEvent.clear(nameInput);
    await userEvent.type(nameInput, "Renamed Event");
    await userEvent.click(screen.getByTestId("event-form-submit"));
    expect(updateMock).toHaveBeenCalledWith(
      expect.objectContaining({
        input: expect.objectContaining({ eventName: "Renamed Event" }),
      }),
      expect.objectContaining({ onSuccess: expect.any(Function) }),
    );
  });

  it("saves updated priority on edit", async () => {
    renderEvents();
    await userEvent.click(screen.getByTestId("edit-event-e1"));
    const priorityInput = screen.getByTestId("event-priority-input");
    await userEvent.clear(priorityInput);
    await userEvent.type(priorityInput, "10");
    await userEvent.click(screen.getByTestId("event-form-submit"));
    expect(updateMock).toHaveBeenCalledWith(
      expect.objectContaining({
        input: expect.objectContaining({ priority: 10 }),
      }),
      expect.objectContaining({ onSuccess: expect.any(Function) }),
    );
  });

  it("saves updated start time on edit", async () => {
    renderEvents();
    await userEvent.click(screen.getByTestId("edit-event-e1"));
    const startInput = screen.getByTestId("event-start-input");
    await userEvent.clear(startInput);
    await userEvent.type(startInput, "2026-03-01T10:00:00Z");
    await userEvent.click(screen.getByTestId("event-form-submit"));
    expect(updateMock).toHaveBeenCalledWith(
      expect.objectContaining({
        input: expect.objectContaining({
          intervalPeriod: expect.objectContaining({ start: "2026-03-01T10:00:00Z" }),
        }),
      }),
      expect.objectContaining({ onSuccess: expect.any(Function) }),
    );
  });

  it("saves updated duration on edit", async () => {
    renderEvents();
    await userEvent.click(screen.getByTestId("edit-event-e1"));
    const durationInput = screen.getByTestId("event-duration-input");
    await userEvent.clear(durationInput);
    await userEvent.type(durationInput, "PT2H");
    await userEvent.click(screen.getByTestId("event-form-submit"));
    expect(updateMock).toHaveBeenCalledWith(
      expect.objectContaining({
        input: expect.objectContaining({
          intervalPeriod: expect.objectContaining({ duration: "PT2H" }),
        }),
      }),
      expect.objectContaining({ onSuccess: expect.any(Function) }),
    );
  });

  it("saves updated intervals on edit", async () => {
    renderEvents();
    await userEvent.click(screen.getByTestId("edit-event-e1"));
    const intervalsInput = screen.getByTestId("event-intervals-input");
    fireEvent.change(intervalsInput, { target: { value: '[{"id":1}]' } });
    await userEvent.click(screen.getByTestId("event-form-submit"));
    expect(updateMock).toHaveBeenCalledWith(
      expect.objectContaining({
        input: expect.objectContaining({ intervals: [{ id: 1 }] }),
      }),
      expect.objectContaining({ onSuccess: expect.any(Function) }),
    );
  });

  it("saves updated targets on edit", async () => {
    renderEvents();
    await userEvent.click(screen.getByTestId("edit-event-e1"));
    const targetsInput = screen.getByTestId("event-targets-input");
    fireEvent.change(targetsInput, { target: { value: '[{"type":"VEN_NAME","values":["ven-3"]}]' } });
    await userEvent.click(screen.getByTestId("event-form-submit"));
    expect(updateMock).toHaveBeenCalledWith(
      expect.objectContaining({
        input: expect.objectContaining({
          targets: [{ type: "VEN_NAME", values: ["ven-3"] }],
        }),
      }),
      expect.objectContaining({ onSuccess: expect.any(Function) }),
    );
  });

  it("submits create event with all fields", async () => {
    renderEvents();
    await userEvent.click(screen.getByTestId("create-event-btn"));
    await userEvent.type(screen.getByTestId("event-name-input"), "New Event");
    await userEvent.type(screen.getByTestId("event-priority-input"), "3");
    await userEvent.type(screen.getByTestId("event-start-input"), "2026-04-01T08:00:00Z");
    await userEvent.type(screen.getByTestId("event-duration-input"), "PT1H");
    await userEvent.click(screen.getByTestId("event-form-submit"));
    expect(createMock).toHaveBeenCalledWith(
      expect.objectContaining({
        eventName: "New Event",
        programID: "p1",
        priority: 3,
        intervalPeriod: { start: "2026-04-01T08:00:00Z", duration: "PT1H" },
        intervals: [],
      }),
      expect.objectContaining({ onSuccess: expect.any(Function) }),
    );
  });
});
