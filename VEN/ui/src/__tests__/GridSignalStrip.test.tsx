import { render, screen } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import type { SignalsState } from "../api/types";
import { GridSignalStrip } from "../components/controller/GridSignalStrip";

const mockSignals = vi.fn((): SignalsState => ({
  alerts: [],
  simple: [],
  dispatch: [],
  capacity: {
    import_limit_kw: null,
    export_limit_kw: null,
    import_subscription_kw: null,
    import_reservation_kw: null,
    export_subscription_kw: null,
    export_reservation_kw: null,
    import_limit_event_id: null,
    export_limit_event_id: null,
    last_updated: null,
  },
}));

vi.mock("../api/hooks", () => ({
  useSignals: () => ({ data: mockSignals() }),
}));

describe("GridSignalStrip", () => {
  beforeEach(() => {
    mockSignals.mockReset();
    mockSignals.mockReturnValue({
      alerts: [],
      simple: [],
      dispatch: [],
      capacity: {
        import_limit_kw: null,
        export_limit_kw: null,
        import_subscription_kw: null,
        import_reservation_kw: null,
        export_subscription_kw: null,
        export_reservation_kw: null,
        import_limit_event_id: null,
        export_limit_event_id: null,
        last_updated: null,
      },
    });
  });

  it("renders nothing when no grid signal is active", () => {
    render(<GridSignalStrip />);
    expect(screen.queryByTestId("signal-strip")).not.toBeInTheDocument();
  });

  it("shows an alert chip while an alert window is active", () => {
    const base = mockSignals();
    mockSignals.mockReturnValue({
      ...base,
      alerts: [{
        alert_type: "GRID_EMERGENCY",
        start: "2026-07-12T10:00:00Z",
        end: "2026-07-12T11:00:00Z",
        event_id: "evt-a",
        message: "shed all load",
      }],
    });
    render(<GridSignalStrip />);
    expect(screen.getByTestId("signal-chip-alert")).toHaveTextContent("GRID_EMERGENCY");
  });

  it("shows SIMPLE level, dispatch setpoint, and capacity chips", () => {
    const base = mockSignals();
    mockSignals.mockReturnValue({
      ...base,
      simple: [
        { level: 1, start: "2026-07-12T10:00:00Z", end: "2026-07-12T11:00:00Z", event_id: "e1" },
        { level: 2, start: "2026-07-12T10:00:00Z", end: "2026-07-12T11:00:00Z", event_id: "e2" },
      ],
      dispatch: [
        { setpoint_kw: 2.0, start: "2026-07-12T10:00:00Z", end: "2026-07-12T11:00:00Z", event_id: "e3" },
      ],
      capacity: { ...base.capacity, import_limit_kw: 5.0, import_reservation_kw: 3.0 },
    });
    render(<GridSignalStrip />);
    expect(screen.getByTestId("signal-chip-simple")).toHaveTextContent("SIMPLE L2");
    expect(screen.getByTestId("signal-chip-dispatch")).toHaveTextContent("2 kW");
    expect(screen.getByTestId("signal-chip-capacity")).toHaveTextContent("limit 5");
  });

  it("labels an upcoming window 'from' instead of 'until'", () => {
    const base = mockSignals();
    const future = new Date(Date.now() + 3_600_000).toISOString();
    const futureEnd = new Date(Date.now() + 7_200_000).toISOString();
    mockSignals.mockReturnValue({
      ...base,
      alerts: [{
        alert_type: "GRID_EMERGENCY",
        start: future,
        end: futureEnd,
        event_id: "evt-f",
        message: "scheduled shed",
      }],
    });
    render(<GridSignalStrip />);
    expect(screen.getByTestId("signal-chip-alert")).toHaveTextContent(/from/);
  });
});
