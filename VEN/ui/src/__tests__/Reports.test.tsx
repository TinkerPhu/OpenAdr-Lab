import { render, screen, fireEvent } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { BrowserRouter } from "react-router-dom";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { ReportsPage, buildExampleResources } from "../pages/Reports";
import type { VtnEvent } from "../api/types";

const mockEvents = [
  {
    id: "e1", programID: "p1", eventName: "emergency-load-shed",
    intervals: [{ id: 0, payloads: [{ type: "SIMPLE", values: [0] }] }],
  },
  {
    id: "e2", programID: "p1", eventName: "peak-shave",
    intervals: [{ id: 0, payloads: [{ type: "IMPORT_CAPACITY_LIMIT", values: [50] }] }],
  },
];

const mockPrograms = [{ id: "p1", programName: "Program Alpha" }];
const mockReports = [
  { id: "r1", clientName: "ven-1", reportName: "test-report", programID: "p1", eventID: "e1", createdDateTime: "2024-01-01" },
];
// WP-T6 (docs/plans/ven-ui-transparency.md): wires GET /obligations.
const mockObligations = [
  {
    id: "ob-1", event_id: "e1", program_id: "p1", payload_type: "USAGE",
    reading_type: "DIRECT_READ", resource_name: null,
    due_at: new Date(Date.now() + 3_600_000).toISOString(),
    interval_duration_s: 900, fulfilled: false,
    created_at: new Date().toISOString(), historical: true,
  },
  {
    id: "ob-2", event_id: "e2", program_id: "p1", payload_type: "USAGE",
    reading_type: "DIRECT_READ", resource_name: null,
    due_at: new Date(Date.now() - 3_600_000).toISOString(),
    interval_duration_s: 900, fulfilled: false,
    created_at: new Date().toISOString(), historical: true,
  },
];

const mutateMock = vi.fn();
const updateMutateMock = vi.fn();

vi.mock("../api/hooks", () => ({
  useSignals: () => ({ data: undefined }),
  useReports: () => ({ data: mockReports, dataUpdatedAt: Date.now() }),
  useEvents: () => ({ data: mockEvents }),
  usePrograms: () => ({ data: mockPrograms }),
  useObligations: () => ({ data: mockObligations }),
  useSubmitReport: () => ({ mutate: mutateMock, isPending: false }),
  useUpdateReport: () => ({ mutate: updateMutateMock, isPending: false }),
}));

vi.mock("../App", () => ({
  useVenContext: () => ({ venUrl: "http://localhost:8081", venName: "ven-1", setVenUrl: vi.fn(), api: {} }),
}));

function renderReports() {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={queryClient}>
      <BrowserRouter>
        <ReportsPage />
      </BrowserRouter>
    </QueryClientProvider>,
  );
}

describe("ReportsPage", () => {
  beforeEach(() => {
    mutateMock.mockReset();
    updateMutateMock.mockReset();
  });

  it("renders heading and reports table", () => {
    renderReports();
    expect(screen.getByTestId("reports-heading")).toHaveTextContent("Reports");
    expect(screen.getByTestId("reports-table")).toBeVisible();
    expect(screen.getByTestId("report-row-r1")).toBeVisible();
  });

  it("opens form when Submit Report is clicked", async () => {
    renderReports();
    await userEvent.click(screen.getByTestId("submit-report-btn"));
    expect(screen.getByTestId("report-form")).toBeVisible();
    expect(screen.getByTestId("report-suggest-btn")).toBeVisible();
  });

  it("Suggest Example fills resources and reportName", async () => {
    renderReports();
    await userEvent.click(screen.getByTestId("submit-report-btn"));
    await userEvent.click(screen.getByTestId("report-suggest-btn"));

    const resourcesInput = screen.getByTestId("report-resources-input") as HTMLTextAreaElement;
    const parsed = JSON.parse(resourcesInput.value);
    expect(parsed).toHaveLength(1);
    expect(parsed[0].resourceName).toBe("ven-1-meter");
    expect(parsed[0].intervals[0].payloads[0].type).toBe("SIMPLE");
    // SIMPLE with value 0 → suggests 1
    expect(parsed[0].intervals[0].payloads[0].values[0]).toBe(1);

    const nameInput = screen.getByTestId("report-name-input") as HTMLInputElement;
    expect(nameInput.value).toMatch(/^report-emergency-load-shed-\d{4}-\d{2}-\d{2}-\d{2}-\d{2}-\d{2}$/);
  });

  it("does not overwrite reportName if already filled", async () => {
    renderReports();
    await userEvent.click(screen.getByTestId("submit-report-btn"));

    const nameInput = screen.getByTestId("report-name-input");
    fireEvent.change(nameInput, { target: { value: "my-custom-name" } });
    await userEvent.click(screen.getByTestId("report-suggest-btn"));

    expect((nameInput as HTMLInputElement).value).toBe("my-custom-name");
  });

  it("renders Edit button on each report row", () => {
    renderReports();
    expect(screen.getByTestId("report-edit-r1")).toBeVisible();
  });

  it("clicking Edit opens form in edit mode with populated fields", async () => {
    renderReports();
    await userEvent.click(screen.getByTestId("report-edit-r1"));

    expect(screen.getByTestId("report-form")).toBeVisible();
    expect(screen.getByText("Edit Report")).toBeVisible();

    const nameInput = screen.getByTestId("report-name-input") as HTMLInputElement;
    expect(nameInput.value).toBe("test-report");

    const submitBtn = screen.getByTestId("report-submit-btn");
    expect(submitBtn).toHaveTextContent("Update");
  });

  it("submitting in edit mode calls updateMutate", async () => {
    renderReports();
    await userEvent.click(screen.getByTestId("report-edit-r1"));
    await userEvent.click(screen.getByTestId("report-submit-btn"));

    expect(updateMutateMock).toHaveBeenCalledTimes(1);
    expect(updateMutateMock.mock.calls[0][0]).toMatchObject({
      id: "r1",
      payload: expect.objectContaining({ eventID: "e1" }),
    });
    expect(mutateMock).not.toHaveBeenCalled();
  });

  it("renders a not-yet-due obligation with a Pending status", () => {
    renderReports();
    expect(screen.getByTestId("obligation-row-ob-1")).toHaveTextContent("Pending");
  });

  it("renders a past-due, unfulfilled obligation with an Overdue status", () => {
    renderReports();
    expect(screen.getByTestId("obligation-row-ob-2")).toHaveTextContent("Overdue");
  });
});

describe("buildExampleResources", () => {
  it("returns SIMPLE value 1 for SIMPLE payload with value 0", () => {
    const event: VtnEvent = {
      id: "e1",
      intervals: [{ id: 0, payloads: [{ type: "SIMPLE", values: [0] }] }],
    };
    const result = JSON.parse(buildExampleResources(event, "ven-1"));
    expect(result[0].resourceName).toBe("ven-1-meter");
    expect(result[0].intervals[0].payloads[0].values[0]).toBe(1);
  });

  it("applies offset to non-zero values", () => {
    const event: VtnEvent = {
      id: "e2",
      intervals: [{ id: 0, payloads: [{ type: "IMPORT_CAPACITY_LIMIT", values: [50] }] }],
    };
    const result = JSON.parse(buildExampleResources(event, "ven-2"));
    const val = result[0].intervals[0].payloads[0].values[0];
    // Should be within ±5% of 50 → between 47 and 53
    expect(val).toBeGreaterThanOrEqual(47);
    expect(val).toBeLessThanOrEqual(53);
  });

  it("keeps zero for non-SIMPLE payload with value 0", () => {
    const event: VtnEvent = {
      id: "e3",
      intervals: [{ id: 0, payloads: [{ type: "IMPORT_CAPACITY_LIMIT", values: [0] }] }],
    };
    const result = JSON.parse(buildExampleResources(event, "ven-1"));
    expect(result[0].intervals[0].payloads[0].values[0]).toBe(0);
  });

  it("handles event with no intervals", () => {
    const event: VtnEvent = { id: "e4" };
    const result = JSON.parse(buildExampleResources(event, "ven-1"));
    expect(result[0].resourceName).toBe("ven-1-meter");
    expect(result[0].intervals).toEqual([]);
  });

  it("handles multiple intervals and payloads", () => {
    const event: VtnEvent = {
      id: "e5",
      intervals: [
        { id: 0, payloads: [{ type: "SIMPLE", values: [0] }, { type: "IMPORT_CAPACITY_LIMIT", values: [100] }] },
        { id: 1, payloads: [{ type: "SIMPLE", values: [0] }] },
      ],
    };
    const result = JSON.parse(buildExampleResources(event, "test-ven"));
    expect(result[0].intervals).toHaveLength(2);
    expect(result[0].intervals[0].payloads).toHaveLength(2);
    expect(result[0].intervals[0].payloads[0].values[0]).toBe(1);
    expect(result[0].intervals[1].payloads[0].values[0]).toBe(1);
  });
});
