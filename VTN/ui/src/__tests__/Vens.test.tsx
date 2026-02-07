import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { BrowserRouter } from "react-router-dom";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { VensPage } from "../pages/Vens";

const mockVens = [
  { id: "v1", venName: "ven-1", createdDateTime: "2026-01-01" },
  { id: "v2", venName: "ven-2", createdDateTime: "2026-01-02" },
  { id: "v3", venName: "ven-3", createdDateTime: "2026-01-03" },
];

const useVensMock = vi.fn(() => ({
  data: mockVens,
  dataUpdatedAt: Date.now(),
}));

vi.mock("../api/hooks", () => ({
  useVens: () => useVensMock(),
}));

function renderVens() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <BrowserRouter>
        <VensPage />
      </BrowserRouter>
    </QueryClientProvider>,
  );
}

describe("VensPage", () => {
  beforeEach(() => {
    useVensMock.mockReturnValue({
      data: mockVens,
      dataUpdatedAt: Date.now(),
    });
  });

  it("renders heading and last updated", () => {
    renderVens();
    expect(screen.getByTestId("vens-heading")).toBeVisible();
    expect(screen.getByTestId("vens-heading")).toHaveTextContent("VENs");
    expect(screen.getByTestId("vens-last-updated")).toBeVisible();
  });

  it("renders VEN list with items", () => {
    renderVens();
    expect(screen.getByTestId("vens-list")).toBeVisible();
    expect(screen.getByTestId("ven-item-v1")).toBeVisible();
    expect(screen.getByTestId("ven-item-v2")).toBeVisible();
    expect(screen.getByTestId("ven-item-v3")).toBeVisible();
  });

  it("filters VENs by search query", async () => {
    renderVens();
    const search = screen.getByTestId("vens-search");
    await userEvent.type(search, "ven-1");
    expect(screen.getByTestId("ven-item-v1")).toBeVisible();
    expect(screen.queryByTestId("ven-item-v2")).not.toBeInTheDocument();
    expect(screen.queryByTestId("ven-item-v3")).not.toBeInTheDocument();
  });

  it("shows empty state when no VENs match", async () => {
    renderVens();
    const search = screen.getByTestId("vens-search");
    await userEvent.type(search, "zzz-no-match");
    expect(screen.getByTestId("vens-empty")).toBeVisible();
    expect(screen.getByTestId("vens-empty")).toHaveTextContent("No VENs");
  });

  it("shows empty state when data is empty", () => {
    useVensMock.mockReturnValue({ data: [], dataUpdatedAt: Date.now() });
    renderVens();
    expect(screen.getByTestId("vens-empty")).toBeVisible();
  });

  it("opens JSON dialog when VEN is clicked", async () => {
    renderVens();
    await userEvent.click(screen.getByText("ven-1"));
    expect(screen.getByTestId("json-dialog")).toBeVisible();
    expect(screen.getByTestId("json-dialog-title")).toHaveTextContent("VEN: ven-1");
  });
});
