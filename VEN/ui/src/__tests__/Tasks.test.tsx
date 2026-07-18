import { render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { BrowserRouter } from "react-router-dom";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { TasksPage } from "../pages/Tasks";
import type { TaskStatusEntry } from "../api/types";

// ─── Mocks ───────────────────────────────────────────────────────────────────

const mockTasks = vi.fn((): TaskStatusEntry[] => []);
const mockDataUpdatedAt = vi.fn((): number => Date.now());

vi.mock("../api/hooks", () => ({
  useTasksStatus: () => ({
    data: mockTasks(),
    dataUpdatedAt: mockDataUpdatedAt(),
  }),
}));

// ─── Wrapper ─────────────────────────────────────────────────────────────────

function renderPage() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <BrowserRouter>
        <TasksPage />
      </BrowserRouter>
    </QueryClientProvider>,
  );
}

// ─── Tests ───────────────────────────────────────────────────────────────────

describe("TasksPage", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockTasks.mockReturnValue([]);
    mockDataUpdatedAt.mockReturnValue(Date.now());
  });

  it("renders heading and last-updated text", () => {
    renderPage();
    expect(screen.getByTestId("tasks-heading")).toHaveTextContent("Background Tasks");
    expect(screen.getByTestId("tasks-last-updated")).toBeInTheDocument();
  });

  it("shows empty state when no tasks recorded", () => {
    renderPage();
    expect(screen.getByTestId("tasks-empty")).toHaveTextContent("No task status recorded yet");
  });

  it("renders one row per task with name, last run, and outcome", () => {
    mockTasks.mockReturnValue([
      {
        name: "poll_events",
        last_run_ts: "2026-07-18T10:00:00Z",
        last_success: null,
        restart_count: 0,
      },
      {
        name: "sim_tick",
        last_run_ts: "2026-07-18T10:05:00Z",
        last_success: false,
        restart_count: 3,
      },
    ]);
    renderPage();

    expect(screen.getByTestId("task-row-poll_events")).toBeInTheDocument();
    expect(screen.getByTestId("task-row-sim_tick")).toBeInTheDocument();
    expect(screen.getByTestId("task-row-poll_events")).toHaveTextContent("running");
    expect(screen.getByTestId("task-row-sim_tick")).toHaveTextContent("panicked");
  });

  it("distinguishes a healthy task (0 restarts) from a restarted one visually", () => {
    mockTasks.mockReturnValue([
      { name: "healthy_task", last_run_ts: null, last_success: null, restart_count: 0 },
      { name: "flaky_task", last_run_ts: null, last_success: false, restart_count: 5 },
    ]);
    renderPage();

    const healthyChip = screen.getByTestId("task-restart-chip-healthy_task");
    const flakyChip = screen.getByTestId("task-restart-chip-flaky_task");
    expect(healthyChip.textContent).toBe("0");
    expect(flakyChip.textContent).toBe("5");
    // MUI Chip color prop surfaces as a class/data attribute — assert they differ.
    expect(healthyChip.className).not.toBe(flakyChip.className);
  });
});
