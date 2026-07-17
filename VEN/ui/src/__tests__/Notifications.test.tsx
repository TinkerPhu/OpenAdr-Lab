import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect, vi, beforeEach } from "vitest";
import type { UserNotification, UserNotificationSeverity } from "../api/types";
import { NotificationsPage } from "../pages/Notifications";

const mockHistoryData = vi.fn((): UserNotification[] => []);
let lastSeverityArg: UserNotificationSeverity | undefined;

vi.mock("../api/hooks", () => ({
  useNotificationHistory: (severity?: UserNotificationSeverity) => {
    lastSeverityArg = severity;
    const data = mockHistoryData().filter(
      (n) => severity === undefined || n.severity === severity
    );
    return { data, isLoading: false };
  },
}));

function makeNotification(overrides: Partial<UserNotification> = {}): UserNotification {
  return {
    id: "n-1",
    created_at: "2026-07-12T10:00:00Z",
    severity: "INFO",
    message: "VTN connection restored",
    asset_id: null,
    event_id: null,
    dedup_key: null,
    count: 1,
    last_seen_at: "2026-07-12T10:00:00Z",
    ...overrides,
  };
}

describe("NotificationsPage", () => {
  beforeEach(() => {
    mockHistoryData.mockReturnValue([]);
    lastSeverityArg = undefined;
  });

  it("renders a deduplicated notification with ×N and first/last seen", () => {
    mockHistoryData.mockReturnValue([
      makeNotification({
        id: "n-dedup",
        severity: "ALERT",
        message: "storage error",
        dedup_key: "storage-error",
        count: 17,
        created_at: "2026-07-12T09:00:00Z",
        last_seen_at: "2026-07-12T11:30:00Z",
      }),
    ]);
    render(<NotificationsPage />);
    const item = screen.getByTestId("notification-history-item-n-dedup");
    expect(item).toHaveTextContent("storage error ×17");
    expect(item).toHaveTextContent(/first .+ — last .+/);
  });

  it("renders a single notification without a count marker", () => {
    mockHistoryData.mockReturnValue([
      makeNotification({ id: "n-single", message: "plan updated" }),
    ]);
    render(<NotificationsPage />);
    const item = screen.getByTestId("notification-history-item-n-single");
    expect(item).toHaveTextContent("plan updated");
    expect(item.textContent).not.toContain("×");
  });

  it("narrows the list via the severity filter", async () => {
    mockHistoryData.mockReturnValue([
      makeNotification({ id: "n-info", severity: "INFO", message: "info msg" }),
      makeNotification({ id: "n-alert", severity: "ALERT", message: "alert msg" }),
    ]);
    const user = userEvent.setup();
    render(<NotificationsPage />);
    expect(screen.getAllByTestId(/^notification-history-item-/)).toHaveLength(2);

    await user.click(screen.getByTestId("severity-filter-alert"));
    expect(lastSeverityArg).toBe("ALERT");
    const items = screen.getAllByTestId(/^notification-history-item-/);
    expect(items).toHaveLength(1);
    expect(items[0]).toHaveTextContent("alert msg");
    expect(within(items[0]).getByText("ALERT")).toBeInTheDocument();
  });

  it("shows newest first", () => {
    mockHistoryData.mockReturnValue([
      makeNotification({ id: "n-old", message: "older" }),
      makeNotification({
        id: "n-new",
        message: "newer",
        created_at: "2026-07-12T12:00:00Z",
        last_seen_at: "2026-07-12T12:00:00Z",
      }),
    ]);
    render(<NotificationsPage />);
    const items = screen.getAllByTestId(/^notification-history-item-/);
    expect(items[0]).toHaveTextContent("newer");
    expect(items[1]).toHaveTextContent("older");
  });

  it("shows an empty state", () => {
    render(<NotificationsPage />);
    expect(screen.getByTestId("notifications-history-empty")).toBeInTheDocument();
  });
});
