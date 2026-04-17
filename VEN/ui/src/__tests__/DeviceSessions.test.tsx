import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { BrowserRouter } from "react-router-dom";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { DeviceSessionsPage } from "../pages/DeviceSessions";
import type { EvSession, HeaterTarget, ShiftableLoad } from "../api/types";

// ─── Mock data ───────────────────────────────────────────────────────────────

const sampleEvSession: EvSession = {
  id: "ev-001",
  target_soc: 0.8,
  departure_time: "2026-04-11T14:00:00Z",
  opportunistic: false,
  created_at: "2026-04-11T06:00:00Z",
  updated_at: "2026-04-11T06:00:00Z",
};

const sampleHeaterTarget: HeaterTarget = {
  id: "ht-001",
  target_temp_c: 55,
  ready_by: "2026-04-11T08:00:00Z",
  created_at: "2026-04-11T06:00:00Z",
  updated_at: "2026-04-11T06:00:00Z",
};

const sampleLoads: ShiftableLoad[] = [
  {
    id: "sl-001",
    asset_id: "wm",
    power_kw: 2.0,
    duration_min: 60,
    earliest_start: "2026-04-11T10:00:00Z",
    latest_end: "2026-04-11T16:00:00Z",
    created_at: "2026-04-11T06:00:00Z",
    updated_at: "2026-04-11T06:00:00Z",
  },
];

// ─── Mocks ───────────────────────────────────────────────────────────────────

const mockEvSession = vi.fn((): EvSession | undefined => undefined);
const mockHeaterTarget = vi.fn((): HeaterTarget | undefined => undefined);
const mockShiftableLoads = vi.fn((): ShiftableLoad[] => []);

const mockPostEvSession = vi.fn();
const mockDeleteEvSession = vi.fn();
const mockPostHeaterTarget = vi.fn();
const mockDeleteHeaterTarget = vi.fn();
const mockPostShiftableLoad = vi.fn();
const mockDeleteShiftableLoad = vi.fn();

vi.mock("../api/hooks", () => ({
  useEvSession: () => ({
    data: mockEvSession(),
    isLoading: false,
    isError: false,
  }),
  usePostEvSession: () => ({
    mutateAsync: mockPostEvSession,
    isPending: false,
  }),
  useDeleteEvSession: () => ({
    mutateAsync: mockDeleteEvSession,
    isPending: false,
  }),
  useHeaterTarget: () => ({
    data: mockHeaterTarget(),
    isLoading: false,
    isError: false,
  }),
  usePostHeaterTarget: () => ({
    mutateAsync: mockPostHeaterTarget,
    isPending: false,
  }),
  useDeleteHeaterTarget: () => ({
    mutateAsync: mockDeleteHeaterTarget,
    isPending: false,
  }),
  useShiftableLoads: () => ({
    data: mockShiftableLoads(),
    isLoading: false,
    isError: false,
  }),
  usePostShiftableLoad: () => ({
    mutateAsync: mockPostShiftableLoad,
    isPending: false,
  }),
  useDeleteShiftableLoad: () => ({
    mutateAsync: mockDeleteShiftableLoad,
    isPending: false,
  }),
  useBaselineOverride: () => ({
    data: undefined,
    isLoading: false,
    isError: false,
  }),
  usePostBaselineOverride: () => ({
    mutateAsync: vi.fn(),
    isPending: false,
  }),
  useDeleteBaselineOverride: () => ({
    mutateAsync: vi.fn(),
    isPending: false,
  }),
}));

// ─── Wrapper ─────────────────────────────────────────────────────────────────

function renderPage() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <BrowserRouter>
        <DeviceSessionsPage />
      </BrowserRouter>
    </QueryClientProvider>,
  );
}

// ─── Tests ───────────────────────────────────────────────────────────────────

describe("DeviceSessionsPage", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockEvSession.mockReturnValue(undefined);
    mockHeaterTarget.mockReturnValue(undefined);
    mockShiftableLoads.mockReturnValue([]);
  });

  it("renders page title", () => {
    renderPage();
    expect(screen.getByTestId("page-title")).toHaveTextContent("Device Sessions");
  });

  // ── EV ─────────────────────────────────────────────────────────────────────

  it("shows 'No active EV session' when no session exists", () => {
    renderPage();
    expect(screen.getByTestId("ev-no-session")).toBeInTheDocument();
    expect(screen.getByTestId("ev-planning-btn")).toBeInTheDocument();
  });

  it("shows EV session card when session exists", () => {
    mockEvSession.mockReturnValue(sampleEvSession);
    renderPage();
    expect(screen.getByTestId("ev-session-card")).toBeInTheDocument();
    expect(screen.getByTestId("ev-target-soc")).toHaveTextContent("80%");
    expect(screen.getByTestId("ev-unplan-btn")).toBeInTheDocument();
  });

  it("opens EV Planning dialog when button clicked", async () => {
    const user = userEvent.setup();
    renderPage();
    await user.click(screen.getByTestId("ev-planning-btn"));
    expect(screen.getByTestId("ev-dialog")).toBeInTheDocument();
    expect(screen.getByTestId("ev-dialog-confirm")).toBeInTheDocument();
  });

  it("calls deleteEvSession on Unplug", async () => {
    mockEvSession.mockReturnValue(sampleEvSession);
    const user = userEvent.setup();
    renderPage();
    await user.click(screen.getByTestId("ev-unplan-btn"));
    expect(mockDeleteEvSession).toHaveBeenCalled();
  });

  // ── Heater ─────────────────────────────────────────────────────────────────

  it("shows 'No active heater target' when no target exists", () => {
    renderPage();
    expect(screen.getByTestId("heater-no-target")).toBeInTheDocument();
    expect(screen.getByTestId("heater-set-btn")).toBeInTheDocument();
  });

  it("shows heater target card when target exists", () => {
    mockHeaterTarget.mockReturnValue(sampleHeaterTarget);
    renderPage();
    expect(screen.getByTestId("heater-target-card")).toBeInTheDocument();
    expect(screen.getByTestId("heater-temp")).toHaveTextContent("55°C");
  });

  it("calls deleteHeaterTarget on Clear", async () => {
    mockHeaterTarget.mockReturnValue(sampleHeaterTarget);
    const user = userEvent.setup();
    renderPage();
    await user.click(screen.getByTestId("heater-clear-btn"));
    expect(mockDeleteHeaterTarget).toHaveBeenCalled();
  });

  // ── Shiftable Loads ────────────────────────────────────────────────────────

  it("shows empty state when no loads exist", () => {
    renderPage();
    expect(screen.getByTestId("shiftable-empty")).toBeInTheDocument();
  });

  it("renders shiftable loads table with rows", () => {
    mockShiftableLoads.mockReturnValue(sampleLoads);
    renderPage();
    expect(screen.getByTestId("shiftable-table")).toBeInTheDocument();
    expect(screen.getByTestId("shiftable-row-sl-001")).toBeInTheDocument();
    expect(screen.getByTestId("shiftable-asset-sl-001")).toHaveTextContent("wm");
    expect(screen.getByTestId("shiftable-power-sl-001")).toHaveTextContent("2");
  });

  it("opens Add Load dialog when button clicked", async () => {
    const user = userEvent.setup();
    renderPage();
    await user.click(screen.getByTestId("shiftable-add-btn"));
    expect(screen.getByTestId("shiftable-dialog")).toBeInTheDocument();
  });

  it("calls deleteShiftableLoad with id on delete", async () => {
    mockShiftableLoads.mockReturnValue(sampleLoads);
    const user = userEvent.setup();
    renderPage();
    await user.click(screen.getByTestId("shiftable-delete-sl-001"));
    expect(mockDeleteShiftableLoad).toHaveBeenCalledWith("sl-001");
  });
});
