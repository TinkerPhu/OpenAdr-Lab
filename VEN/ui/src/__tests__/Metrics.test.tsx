import { render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { BrowserRouter } from "react-router-dom";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { MetricsPage } from "../pages/Metrics";

// ─── Mock data ───────────────────────────────────────────────────────────────

const samplePrometheusText = [
  "# HELP plan_cycle_total Total plan cycles",
  "# TYPE plan_cycle_total counter",
  'plan_cycle_total{trigger="rate_change"} 12',
  'plan_cycle_total{trigger="schedule"} 48',
  "# HELP plan_solve_seconds MILP solve duration",
  "plan_solve_seconds 1.234",
].join("\n");

// ─── Mocks ───────────────────────────────────────────────────────────────────

const mockMetrics = vi.fn((): string => "");
const mockDataUpdatedAt = vi.fn((): number => Date.now());

vi.mock("../api/hooks", () => ({
  useSignals: () => ({ data: undefined }),
  useMetrics: () => ({
    data: mockMetrics(),
    dataUpdatedAt: mockDataUpdatedAt(),
    isLoading: false,
  }),
}));

// ─── Wrapper ─────────────────────────────────────────────────────────────────

function renderPage() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <BrowserRouter>
        <MetricsPage />
      </BrowserRouter>
    </QueryClientProvider>,
  );
}

// ─── Tests ───────────────────────────────────────────────────────────────────

describe("MetricsPage", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockMetrics.mockReturnValue("");
    mockDataUpdatedAt.mockReturnValue(Date.now());
  });

  it("renders heading and last-updated text", () => {
    renderPage();
    expect(screen.getByTestId("metrics-heading")).toHaveTextContent("Metrics");
    expect(screen.getByTestId("metrics-last-updated")).toBeInTheDocument();
  });

  it("shows empty state when no metrics available", () => {
    renderPage();
    expect(screen.getByTestId("metrics-empty")).toHaveTextContent("No metrics available");
  });

  it("parses and displays prometheus metrics with labels", () => {
    mockMetrics.mockReturnValue(samplePrometheusText);
    renderPage();

    // Should render table for plan_cycle_total
    expect(screen.getByTestId("metrics-table-plan_cycle_total")).toBeInTheDocument();
    // Should render table for plan_solve_seconds
    expect(screen.getByTestId("metrics-table-plan_solve_seconds")).toBeInTheDocument();

    // Check values are displayed
    expect(screen.getByText("12")).toBeInTheDocument();
    expect(screen.getByText("48")).toBeInTheDocument();
    expect(screen.getByText("1.234")).toBeInTheDocument();
  });

  it("renders label strings for labeled metrics", () => {
    mockMetrics.mockReturnValue(samplePrometheusText);
    renderPage();

    expect(screen.getByText('trigger="rate_change"')).toBeInTheDocument();
    expect(screen.getByText('trigger="schedule"')).toBeInTheDocument();
  });

  it("renders dash for metrics without labels", () => {
    mockMetrics.mockReturnValue("simple_metric 42\n");
    renderPage();

    expect(screen.getByText("42")).toBeInTheDocument();
    // The "—" (em-dash) for empty labels
    expect(screen.getByText("—")).toBeInTheDocument();
  });
});
