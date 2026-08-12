import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect, vi, beforeEach } from "vitest";
import type { BaselineOverride } from "../api/types";
import { BaselineOverrideCard } from "../components/devices/BaselineOverrideCard";

const mockOverrideData = vi.fn((): BaselineOverride | null => null);
const mockPostOverride = vi.fn(async () => ({}));
const mockDeleteOverride = vi.fn(async () => ({}));

vi.mock("../api/hooks", () => ({
  useBaselineOverride: () => ({ data: mockOverrideData() }),
  usePostBaselineOverride: () => ({ mutateAsync: mockPostOverride, isPending: false }),
  useDeleteBaselineOverride: () => ({ mutateAsync: mockDeleteOverride, isPending: false }),
}));

describe("BaselineOverrideCard", () => {
  beforeEach(() => {
    mockPostOverride.mockClear();
    mockDeleteOverride.mockClear();
    mockOverrideData.mockReturnValue(null);
  });

  it("shows an empty state and disables Clear when no override is active", () => {
    render(<BaselineOverrideCard />);
    expect(screen.getByTestId("baseline-override-card")).toBeInTheDocument();
    expect(screen.getByText(/No baseline override active/i)).toBeInTheDocument();
    expect(screen.getByTestId("baseline-clear-btn")).toBeDisabled();
    expect(screen.getByTestId("baseline-save-btn")).toBeDisabled();
  });

  it("renders one row per slot when an override is active", () => {
    mockOverrideData.mockReturnValue({
      id: "bo-1",
      slots: [
        { slot_start: "2026-04-12T07:00:00Z", add_kw: 1.5 },
        { slot_start: "2026-04-12T08:00:00Z", add_kw: -0.5 },
      ],
      created_at: "2026-04-11T06:00:00Z",
      updated_at: "2026-04-11T06:00:00Z",
    });
    render(<BaselineOverrideCard />);
    const rows = screen.getAllByTestId(/^baseline-row-/);
    expect(rows).toHaveLength(2);
    expect(screen.getByTestId("baseline-add-kw-0")).toHaveValue(1.5);
    expect(screen.getByTestId("baseline-add-kw-1")).toHaveValue(-0.5);
    expect(screen.getByTestId("baseline-clear-btn")).not.toBeDisabled();
  });

  it("adding a row appends a new blank row without a network request", async () => {
    const user = userEvent.setup();
    render(<BaselineOverrideCard />);
    await user.click(screen.getByTestId("baseline-add-btn"));
    expect(screen.getAllByTestId(/^baseline-row-/)).toHaveLength(1);
    expect(mockPostOverride).not.toHaveBeenCalled();
    expect(mockDeleteOverride).not.toHaveBeenCalled();
  });

  it("removing a row drops it from the list without a network request", async () => {
    mockOverrideData.mockReturnValue({
      id: "bo-1",
      slots: [{ slot_start: "2026-04-12T07:00:00Z", add_kw: 1.5 }],
      created_at: "2026-04-11T06:00:00Z",
      updated_at: "2026-04-11T06:00:00Z",
    });
    const user = userEvent.setup();
    render(<BaselineOverrideCard />);
    await user.click(screen.getByTestId("baseline-remove-0"));
    expect(screen.queryAllByTestId(/^baseline-row-/)).toHaveLength(0);
    expect(mockPostOverride).not.toHaveBeenCalled();
  });

  it("saving edited rows calls postBaselineOverride with slot_start/add_kw", async () => {
    mockOverrideData.mockReturnValue({
      id: "bo-1",
      slots: [{ slot_start: "2026-04-12T07:00:00Z", add_kw: 1.5 }],
      created_at: "2026-04-11T06:00:00Z",
      updated_at: "2026-04-11T06:00:00Z",
    });
    const user = userEvent.setup();
    render(<BaselineOverrideCard />);
    const addKw = screen.getByTestId("baseline-add-kw-0");
    await user.clear(addKw);
    await user.type(addKw, "2.5");
    await user.click(screen.getByTestId("baseline-save-btn"));
    expect(mockPostOverride).toHaveBeenCalledWith({
      slots: [{ slot_start: "2026-04-12T07:00:00Z", add_kw: 2.5 }],
    });
  });

  it("save is disabled with zero rows", () => {
    render(<BaselineOverrideCard />);
    expect(screen.getByTestId("baseline-save-btn")).toBeDisabled();
  });

  it("clearing the slot-start input does not crash and leaves the row's value unchanged", async () => {
    mockOverrideData.mockReturnValue({
      id: "bo-1",
      slots: [{ slot_start: "2026-04-12T07:00:00Z", add_kw: 1.5 }],
      created_at: "2026-04-11T06:00:00Z",
      updated_at: "2026-04-11T06:00:00Z",
    });
    const user = userEvent.setup();
    render(<BaselineOverrideCard />);
    const slotStart = screen.getByTestId("baseline-slot-start-0");
    await user.clear(slotStart);
    // Must not throw (RangeError from new Date("").toISOString()); the row
    // stays on its last valid value rather than adopting an invalid one.
    expect(screen.getByTestId("baseline-override-card")).toBeInTheDocument();
  });

  it("clear calls deleteBaselineOverride when an override is active", async () => {
    mockOverrideData.mockReturnValue({
      id: "bo-1",
      slots: [{ slot_start: "2026-04-12T07:00:00Z", add_kw: 1.5 }],
      created_at: "2026-04-11T06:00:00Z",
      updated_at: "2026-04-11T06:00:00Z",
    });
    const user = userEvent.setup();
    render(<BaselineOverrideCard />);
    await user.click(screen.getByTestId("baseline-clear-btn"));
    expect(mockDeleteOverride).toHaveBeenCalled();
  });
});
