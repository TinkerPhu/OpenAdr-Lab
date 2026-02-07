import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { BrowserRouter } from "react-router-dom";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { ProgramsPage } from "../pages/Programs";

const mockPrograms = [
  { id: "p1", programName: "Program Alpha", createdDateTime: "2026-01-01" },
  { id: "p2", programName: "Program Beta", createdDateTime: "2026-01-02" },
  { id: "p3", programName: "Program Gamma", createdDateTime: "2026-01-03" },
];

const useProgramsMock = vi.fn(() => ({
  data: mockPrograms,
  dataUpdatedAt: Date.now(),
}));

const createMock = vi.fn();
const updateMock = vi.fn();
const deleteMock = vi.fn();

vi.mock("../api/hooks", () => ({
  usePrograms: () => useProgramsMock(),
  useCreateProgram: () => ({ mutate: createMock, isPending: false }),
  useUpdateProgram: () => ({ mutate: updateMock, isPending: false }),
  useDeleteProgram: () => ({ mutate: deleteMock, isPending: false }),
}));

function renderPrograms() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <BrowserRouter>
        <ProgramsPage />
      </BrowserRouter>
    </QueryClientProvider>,
  );
}

describe("ProgramsPage", () => {
  beforeEach(() => {
    useProgramsMock.mockReturnValue({
      data: mockPrograms,
      dataUpdatedAt: Date.now(),
    });
  });

  it("renders heading and last updated", () => {
    renderPrograms();
    expect(screen.getByTestId("programs-heading")).toBeVisible();
    expect(screen.getByTestId("programs-heading")).toHaveTextContent("Programs");
    expect(screen.getByTestId("programs-last-updated")).toBeVisible();
  });

  it("renders program list with items", () => {
    renderPrograms();
    expect(screen.getByTestId("programs-list")).toBeVisible();
    expect(screen.getByTestId("program-item-p1")).toBeVisible();
    expect(screen.getByTestId("program-item-p2")).toBeVisible();
    expect(screen.getByTestId("program-item-p3")).toBeVisible();
  });

  it("filters programs by search query", async () => {
    renderPrograms();
    const search = screen.getByTestId("programs-search");
    await userEvent.type(search, "Alpha");
    expect(screen.getByTestId("program-item-p1")).toBeVisible();
    expect(screen.queryByTestId("program-item-p2")).not.toBeInTheDocument();
    expect(screen.queryByTestId("program-item-p3")).not.toBeInTheDocument();
  });

  it("shows empty state when no programs match", async () => {
    renderPrograms();
    const search = screen.getByTestId("programs-search");
    await userEvent.type(search, "zzz-no-match");
    expect(screen.getByTestId("programs-empty")).toBeVisible();
    expect(screen.getByTestId("programs-empty")).toHaveTextContent("No programs");
  });

  it("shows empty state when data is empty", () => {
    useProgramsMock.mockReturnValue({ data: [], dataUpdatedAt: Date.now() });
    renderPrograms();
    expect(screen.getByTestId("programs-empty")).toBeVisible();
  });

  it("opens JSON dialog when program is clicked", async () => {
    renderPrograms();
    await userEvent.click(screen.getByText("Program Alpha"));
    expect(screen.getByTestId("json-dialog")).toBeVisible();
    expect(screen.getByTestId("json-dialog-title")).toHaveTextContent("Program: Program Alpha");
  });

  it("opens create program dialog", async () => {
    renderPrograms();
    await userEvent.click(screen.getByTestId("create-program-btn"));
    expect(screen.getByTestId("program-form-dialog")).toBeVisible();
  });

  it("submits new program", async () => {
    renderPrograms();
    await userEvent.click(screen.getByTestId("create-program-btn"));
    await userEvent.type(screen.getByTestId("program-name-input"), "New Program");
    await userEvent.click(screen.getByTestId("program-form-submit"));
    expect(createMock).toHaveBeenCalledWith(
      { programName: "New Program" },
      expect.objectContaining({ onSuccess: expect.any(Function) }),
    );
  });

  it("opens edit dialog with pre-filled data", async () => {
    renderPrograms();
    await userEvent.click(screen.getByTestId("edit-program-p1"));
    expect(screen.getByTestId("program-name-input")).toHaveValue("Program Alpha");
  });

  it("opens confirm dialog on delete click", async () => {
    renderPrograms();
    await userEvent.click(screen.getByTestId("delete-program-p1"));
    expect(screen.getByTestId("confirm-dialog")).toBeVisible();
  });

  it("calls delete on confirm", async () => {
    renderPrograms();
    await userEvent.click(screen.getByTestId("delete-program-p1"));
    await userEvent.click(screen.getByTestId("confirm-dialog-ok"));
    expect(deleteMock).toHaveBeenCalledWith(
      "p1",
      expect.objectContaining({ onSuccess: expect.any(Function) }),
    );
  });
});
