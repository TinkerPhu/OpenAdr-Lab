import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect, vi } from "vitest";
import { DiagnosticCell } from "../components/raw-diagnostics/DiagnosticCell";

function renderCell(props: {
  title?: string;
  isLoading?: boolean;
  isError?: boolean;
  onRefresh?: () => void;
}) {
  const { title = "Test Cell", isLoading = false, isError = false, onRefresh = vi.fn() } = props;
  return render(
    <DiagnosticCell title={title} isLoading={isLoading} isError={isError} onRefresh={onRefresh}>
      <div data-testid="chart-child">chart content</div>
    </DiagnosticCell>
  );
}

describe("DiagnosticCell", () => {
  it("renders title and refresh button", () => {
    renderCell({ title: "Simulator State" });
    expect(screen.getByText("Simulator State")).toBeInTheDocument();
    expect(screen.getByTestId("refresh-btn-simulator-state")).toBeInTheDocument();
  });

  it("shows children when not loading and not error", () => {
    renderCell({});
    expect(screen.getByTestId("chart-child")).toBeInTheDocument();
  });

  it("shows loading indicator and hides children while loading", () => {
    renderCell({ isLoading: true });
    expect(screen.getByTestId("loading-indicator-test-cell")).toBeInTheDocument();
    expect(screen.queryByTestId("chart-child")).not.toBeInTheDocument();
  });

  it("shows error message and hides children when isError", () => {
    renderCell({ isError: true });
    expect(screen.getByTestId("error-msg-test-cell")).toHaveTextContent("Failed to load data");
    expect(screen.queryByTestId("chart-child")).not.toBeInTheDocument();
  });

  it("calls onRefresh when refresh button is clicked", async () => {
    const onRefresh = vi.fn();
    renderCell({ onRefresh });
    await userEvent.click(screen.getByTestId("refresh-btn-test-cell"));
    expect(onRefresh).toHaveBeenCalledOnce();
  });

  it("does not call onRefresh on mount", () => {
    const onRefresh = vi.fn();
    renderCell({ onRefresh });
    expect(onRefresh).not.toHaveBeenCalled();
  });
});
