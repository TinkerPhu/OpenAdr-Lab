import { render, screen } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { MetricsPage } from "../pages/Metrics";

const useMetricsMock = vi.fn();

vi.mock("../api/hooks", () => ({
  useMetrics: () => useMetricsMock(),
}));

const PROM = `# HELP http_requests_total requests
# TYPE http_requests_total counter
http_requests_total{method="GET",code="200"} 12
http_requests_total{method="POST",code="500"} 3
process_open_fds 42
broken_value{a="b"} NaN
`;

describe("MetricsPage", () => {
  beforeEach(() => {
    useMetricsMock.mockReset();
  });

  it("says so when there are no metrics", () => {
    useMetricsMock.mockReturnValue({ data: "", dataUpdatedAt: 0 });
    render(<MetricsPage />);
    expect(screen.getByTestId("metrics-empty")).toBeTruthy();
    expect(screen.getByTestId("metrics-last-updated").textContent).toContain("—");
  });

  it("groups rows by metric name, one table per name", () => {
    useMetricsMock.mockReturnValue({ data: PROM, dataUpdatedAt: Date.now() });
    render(<MetricsPage />);
    expect(screen.queryByTestId("metrics-empty")).toBeNull();
    const table = screen.getByTestId("metrics-table-http_requests_total");
    expect(table.querySelectorAll("tbody tr")).toHaveLength(2);
    expect(screen.getByTestId("metrics-table-process_open_fds")).toBeTruthy();
  });

  it("shows labels and values, and skips comment lines", () => {
    useMetricsMock.mockReturnValue({ data: PROM, dataUpdatedAt: Date.now() });
    render(<MetricsPage />);
    expect(screen.getByText('method="GET", code="200"')).toBeTruthy();
    expect(screen.getByText("12")).toBeTruthy();
    expect(screen.queryByText(/HELP/)).toBeNull();
  });

  it("renders an unlabelled sample with a dash and a NaN value as NaN", () => {
    useMetricsMock.mockReturnValue({ data: PROM, dataUpdatedAt: Date.now() });
    render(<MetricsPage />);
    expect(screen.getByTestId("metrics-table-process_open_fds").textContent).toContain("—");
    expect(screen.getByTestId("metrics-table-broken_value").textContent).toContain("NaN");
  });
});
