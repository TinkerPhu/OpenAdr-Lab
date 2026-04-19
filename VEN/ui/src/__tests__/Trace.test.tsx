import { render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { BrowserRouter } from "react-router-dom";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { TracePage } from "../pages/Trace";
import type { TraceEntry } from "../api/types";

// ─── Mock data ───────────────────────────────────────────────────────────────

const sampleEntries: TraceEntry[] = [
  {
    type: "PlanCycle",
    ts: "2026-04-11T12:00:00Z",
    trigger_reason: "rate_change",
    total_slots: 48,
  },
  {
    type: "OpenAdrArrived",
    ts: "2026-04-11T11:55:00Z",
    event_name: "TOU_SIGNAL",
    signal_type: "SIMPLE",
    value: 1.0,
    interval: 0,
  },
  {
    type: "OpenAdrExpired",
    ts: "2026-04-11T11:50:00Z",
    event_name: "OLD_EVENT",
  },
  {
    type: "RateChange",
    ts: "2026-04-11T11:45:00Z",
    interval_start: "2026-04-11T12:00:00Z",
    import_eur_kwh: 0.3012,
    export_eur_kwh: 0.0821,
  },
  {
    type: "CapacityChange",
    ts: "2026-04-11T11:40:00Z",
    import_limit_kw: 10,
    export_limit_kw: null,
  },
  {
    type: "PacketTransition",
    ts: "2026-04-11T11:35:00Z",
    packet_id: "aabbccdd-1234-5678-9012-aabbccddeeff",
    asset_id: "ev",
    from_status: "PENDING",
    to_status: "SCHEDULED",
  },
  {
    type: "RequestTransition",
    ts: "2026-04-11T11:30:00Z",
    request_id: "11223344-5566-7788-9900-aabbccddeeff",
    asset_id: "heater",
    from_status: "ACTIVE",
    to_status: "COMPLETED",
  },
];

// ─── Mocks ───────────────────────────────────────────────────────────────────

const mockTrace = vi.fn((): TraceEntry[] | undefined => undefined);
const mockDataUpdatedAt = vi.fn((): number => Date.now());

vi.mock("../api/hooks", () => ({
  useTrace: () => ({
    data: mockTrace(),
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
        <TracePage />
      </BrowserRouter>
    </QueryClientProvider>,
  );
}

// ─── Tests ───────────────────────────────────────────────────────────────────

describe("TracePage", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockTrace.mockReturnValue(undefined);
    mockDataUpdatedAt.mockReturnValue(Date.now());
  });

  it("renders heading", () => {
    renderPage();
    expect(screen.getByTestId("trace-heading")).toHaveTextContent("Decision Trace");
  });

  it("shows empty state when no entries", () => {
    renderPage();
    expect(screen.getByText("No trace events yet")).toBeInTheDocument();
  });

  it("renders table with trace entries", () => {
    mockTrace.mockReturnValue(sampleEntries);
    renderPage();

    const table = screen.getByTestId("trace-table");
    expect(table).toBeInTheDocument();

    // Should render rows for each entry
    for (let i = 0; i < sampleEntries.length; i++) {
      expect(screen.getByTestId(`trace-row-${i}`)).toBeInTheDocument();
    }
  });

  it("renders PlanCycle entry details", () => {
    mockTrace.mockReturnValue([sampleEntries[0]]);
    renderPage();

    expect(screen.getByText("PlanCycle")).toBeInTheDocument();
    expect(screen.getByText(/rate_change/)).toBeInTheDocument();
    expect(screen.getByText(/48 slots/)).toBeInTheDocument();
  });

  it("renders OpenAdrArrived entry details", () => {
    mockTrace.mockReturnValue([sampleEntries[1]]);
    renderPage();

    expect(screen.getByText("OpenAdrArrived")).toBeInTheDocument();
    expect(screen.getByText(/TOU_SIGNAL/)).toBeInTheDocument();
  });

  it("renders RateChange with formatted tariffs", () => {
    mockTrace.mockReturnValue([sampleEntries[3]]);
    renderPage();

    expect(screen.getByText("RateChange")).toBeInTheDocument();
    expect(screen.getByText(/0\.3012/)).toBeInTheDocument();
    expect(screen.getByText(/0\.0821/)).toBeInTheDocument();
  });

  it("renders CapacityChange with null handling", () => {
    mockTrace.mockReturnValue([sampleEntries[4]]);
    renderPage();

    expect(screen.getByText("CapacityChange")).toBeInTheDocument();
    expect(screen.getByText(/10 kW/)).toBeInTheDocument();
  });

  it("renders PacketTransition with truncated ID", () => {
    mockTrace.mockReturnValue([sampleEntries[5]]);
    renderPage();

    expect(screen.getByText("PacketTransition")).toBeInTheDocument();
    expect(screen.getByText("aabbccdd")).toBeInTheDocument();
    expect(screen.getByText(/PENDING → SCHEDULED/)).toBeInTheDocument();
  });

  it("renders RequestTransition entry", () => {
    mockTrace.mockReturnValue([sampleEntries[6]]);
    renderPage();

    expect(screen.getByText("RequestTransition")).toBeInTheDocument();
    expect(screen.getByText(/ACTIVE → COMPLETED/)).toBeInTheDocument();
    expect(screen.getByText("11223344")).toBeInTheDocument();
  });

  it("shows correct entry count in subtitle", () => {
    mockTrace.mockReturnValue(sampleEntries);
    renderPage();

    expect(screen.getByText(/Last 7 controller events/)).toBeInTheDocument();
  });
});
