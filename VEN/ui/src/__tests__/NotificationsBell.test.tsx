import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect, vi, beforeEach } from "vitest";
import type { UserNotification } from "../api/types";
import { NotificationsBell } from "../components/NotificationsBell";

const mockNotificationsData = vi.fn((): UserNotification[] => []);

vi.mock("../api/hooks", () => ({
  useSignals: () => ({ data: undefined }),
  useNotifications: () => ({ data: mockNotificationsData() }),
}));

function makeNotification(overrides: Partial<UserNotification> = {}): UserNotification {
  return {
    id: "n-1",
    created_at: "2026-07-12T10:00:00Z",
    severity: "INFO",
    message: "VTN connection restored",
    asset_id: null,
    event_id: null,
    ...overrides,
  };
}

describe("NotificationsBell", () => {
  beforeEach(() => {
    mockNotificationsData.mockReturnValue([]);
  });

  it("shows the badge count and lists notifications newest first", async () => {
    mockNotificationsData.mockReturnValue([
      makeNotification({ id: "n-old", message: "VTN connection restored", severity: "INFO" }),
      makeNotification({
        id: "n-new",
        created_at: "2026-07-12T11:00:00Z",
        severity: "ALERT",
        message: "Grid emergency (GRID_EMERGENCY): shed all load",
        event_id: "evt-a",
      }),
    ]);
    const user = userEvent.setup();
    render(<NotificationsBell />);

    expect(screen.getByTestId("notifications-bell")).toHaveTextContent("2");

    await user.click(screen.getByTestId("notifications-bell"));
    const items = screen.getAllByTestId(/^notification-item-/);
    expect(items).toHaveLength(2);
    // Ring arrives oldest-first; the panel shows newest first.
    expect(items[0]).toHaveTextContent("Grid emergency");
    expect(within(items[0]).getByText("ALERT")).toBeInTheDocument();
    expect(items[1]).toHaveTextContent("VTN connection restored");
  });

  it("shows an empty-state message when there are no notifications", async () => {
    const user = userEvent.setup();
    render(<NotificationsBell />);
    await user.click(screen.getByTestId("notifications-bell"));
    expect(screen.getByTestId("notifications-empty")).toBeInTheDocument();
  });
});
