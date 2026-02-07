import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { BrowserRouter } from "react-router-dom";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { ReportsPage } from "../pages/Reports";

const mockReports = [
  {
    id: "r1",
    programID: "p1",
    eventID: "e1",
    clientName: "ven-1",
    reportName: "power-report",
    resources: [],
    createdDateTime: "2026-01-01",
  },
  {
    id: "r2",
    programID: "p2",
    eventID: "e2",
    clientName: "ven-2",
    reportName: null,
    resources: [],
    createdDateTime: "2026-01-02",
  },
];

const useReportsMock = vi.fn(() => ({
  data: mockReports,
  dataUpdatedAt: Date.now(),
}));

const deleteMock = vi.fn();

vi.mock("../api/hooks", () => ({
  useReports: () => useReportsMock(),
  useDeleteReport: () => ({ mutate: deleteMock, isPending: false }),
}));

function renderReports() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
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
    useReportsMock.mockReturnValue({
      data: mockReports,
      dataUpdatedAt: Date.now(),
    });
  });

  it("renders heading and last updated", () => {
    renderReports();
    expect(screen.getByTestId("reports-heading")).toBeVisible();
    expect(screen.getByTestId("reports-heading")).toHaveTextContent("Reports");
    expect(screen.getByTestId("reports-last-updated")).toBeVisible();
  });

  it("renders report rows", () => {
    renderReports();
    expect(screen.getByTestId("reports-table")).toBeVisible();
    expect(screen.getByTestId("report-row-r1")).toBeVisible();
    expect(screen.getByTestId("report-row-r2")).toBeVisible();
  });

  it("shows client name and report name", () => {
    renderReports();
    expect(screen.getByText("ven-1")).toBeVisible();
    expect(screen.getByText("power-report")).toBeVisible();
  });

  it("filters reports by search query", async () => {
    renderReports();
    const search = screen.getByTestId("reports-search");
    await userEvent.type(search, "ven-1");
    expect(screen.getByTestId("report-row-r1")).toBeVisible();
    expect(screen.queryByTestId("report-row-r2")).not.toBeInTheDocument();
  });

  it("shows empty state when no reports", () => {
    useReportsMock.mockReturnValue({ data: [], dataUpdatedAt: Date.now() });
    renderReports();
    expect(screen.getByTestId("reports-empty")).toBeVisible();
    expect(screen.getByTestId("reports-empty")).toHaveTextContent("No reports");
  });

  it("opens JSON dialog when report row is clicked", async () => {
    renderReports();
    await userEvent.click(screen.getByTestId("report-row-r1"));
    expect(screen.getByTestId("json-dialog")).toBeVisible();
    expect(screen.getByTestId("json-dialog-title")).toHaveTextContent("Report: power-report");
  });

  it("opens confirm dialog on delete click", async () => {
    renderReports();
    await userEvent.click(screen.getByTestId("delete-report-r1"));
    expect(screen.getByTestId("confirm-dialog")).toBeVisible();
  });

  it("calls delete on confirm", async () => {
    renderReports();
    await userEvent.click(screen.getByTestId("delete-report-r1"));
    await userEvent.click(screen.getByTestId("confirm-dialog-ok"));
    expect(deleteMock).toHaveBeenCalledWith(
      "r1",
      expect.objectContaining({ onSuccess: expect.any(Function) }),
    );
  });
});
