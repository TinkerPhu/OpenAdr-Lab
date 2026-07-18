import { render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { BrowserRouter } from "react-router-dom";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { EventLogPage } from "../pages/EventLog";
import type { EventLogEntry } from "../api/types";

// ─── Mocks ───────────────────────────────────────────────────────────────────

const mockEntries = vi.fn((): EventLogEntry[] => []);
const mockDataUpdatedAt = vi.fn((): number => Date.now());

vi.mock("../api/hooks", () => ({
  useEventLog: () => ({
    data: mockEntries(),
    dataUpdatedAt: mockDataUpdatedAt(),
  }),
}));

// ─── Wrapper ─────────────────────────────────────────────────────────────────

function renderPage() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <BrowserRouter>
        <EventLogPage />
      </BrowserRouter>
    </QueryClientProvider>,
  );
}

// ─── Tests ───────────────────────────────────────────────────────────────────

describe("EventLogPage", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockEntries.mockReturnValue([]);
    mockDataUpdatedAt.mockReturnValue(Date.now());
  });

  it("renders heading and last-updated text", () => {
    renderPage();
    expect(screen.getByTestId("event-log-heading")).toHaveTextContent("Event Log");
    expect(screen.getByTestId("event-log-last-updated")).toBeInTheDocument();
  });

  it("shows empty state when no events recorded", () => {
    renderPage();
    expect(screen.getByTestId("event-log-empty")).toHaveTextContent("No events recorded yet");
  });

  it("renders one row per entry with category and message", () => {
    mockEntries.mockReturnValue([
      {
        id: "evt-1",
        created_at: "2026-07-18T10:00:00Z",
        category: "vtn_connection",
        message: "connection refused",
      },
      {
        id: "evt-2",
        created_at: "2026-07-18T10:05:00Z",
        category: "task_supervisor",
        message: "sim_tick panicked",
      },
    ]);
    renderPage();

    expect(screen.getByTestId("event-log-row-evt-1")).toBeInTheDocument();
    expect(screen.getByTestId("event-log-row-evt-2")).toBeInTheDocument();
    expect(screen.getByTestId("event-log-row-evt-1")).toHaveTextContent("connection refused");
    expect(screen.getByTestId("event-log-category-evt-2")).toHaveTextContent("task_supervisor");
  });

  it("renders newest entries first", () => {
    mockEntries.mockReturnValue([
      { id: "evt-old", created_at: "2026-07-18T09:00:00Z", category: "storage", message: "old" },
      { id: "evt-new", created_at: "2026-07-18T10:00:00Z", category: "storage", message: "new" },
    ]);
    renderPage();

    const rows = screen.getAllByTestId(/^event-log-row-/);
    expect(rows[0]).toHaveAttribute("data-testid", "event-log-row-evt-new");
    expect(rows[1]).toHaveAttribute("data-testid", "event-log-row-evt-old");
  });
});
