import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { BrowserRouter } from "react-router-dom";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { DevicesPage } from "../pages/Devices";
import type { UserRequestWithSession, EvSettings } from "../api/types";

// ─── Mock data ───────────────────────────────────────────────────────────────

function makeEvRequest(overrides: Partial<UserRequestWithSession> = {}): UserRequestWithSession {
  return {
    id: "ur-ev-001",
    asset_id: "ev",
    target_energy_kwh: 20,
    target_soc: 0.8,
    desired_power_kw: 7.0,
    completion_policy: "CONTINUE",
    deadlines: [{ latest_end: "2026-04-12T07:00:00Z", max_total_cost_eur: null, max_marginal_rate_eur_kwh: null, min_completion: 0.8 }],
    max_total_cost_eur: null,
    tier_count: 1,
    session_id: "sess-ev-001",
    session_type: "ev",
    status: "ACTIVE",
    estimated_cost_eur: 1.23,
    estimated_co2_g: 300,
    interruptible: false,
    tolerance_min: null,
    budget_eur: null,
    created_at: "2026-04-11T06:00:00Z",
    updated_at: "2026-04-11T06:00:00Z",
    session: {
      type: "ev",
      id: "sess-ev-001",
      target_soc: 0.8,
      departure_time: "2026-04-12T07:00:00Z",
      soft_deadline: false,
      created_at: "2026-04-11T06:00:00Z",
      updated_at: "2026-04-11T06:00:00Z",
    },
    ...overrides,
  };
}

function makeHeaterRequest(overrides: Partial<UserRequestWithSession> = {}): UserRequestWithSession {
  return {
    id: "ur-ht-001",
    asset_id: "heater",
    target_energy_kwh: 5,
    target_soc: null,
    desired_power_kw: 2,
    completion_policy: "STOP",
    deadlines: [{ latest_end: "2026-04-12T09:00:00Z", max_total_cost_eur: null, max_marginal_rate_eur_kwh: null, min_completion: 1.0 }],
    max_total_cost_eur: null,
    tier_count: 1,
    session_id: "sess-ht-001",
    session_type: "heater",
    status: "ACTIVE",
    estimated_cost_eur: 0.34,
    estimated_co2_g: 80,
    interruptible: false,
    tolerance_min: null,
    budget_eur: null,
    created_at: "2026-04-11T06:00:00Z",
    updated_at: "2026-04-11T06:00:00Z",
    session: {
      type: "heater",
      id: "sess-ht-001",
      target_temp_c: 55,
      ready_by: "2026-04-12T09:00:00Z",
      created_at: "2026-04-11T06:00:00Z",
      updated_at: "2026-04-11T06:00:00Z",
    },
    ...overrides,
  };
}

function makeShiftableRequest(id: string, overrides: Partial<UserRequestWithSession> = {}): UserRequestWithSession {
  return {
    id,
    asset_id: "wm",
    target_energy_kwh: 0,
    target_soc: null,
    desired_power_kw: 0,
    completion_policy: "STOP",
    deadlines: [],
    max_total_cost_eur: null,
    tier_count: 0,
    session_id: `sess-${id}`,
    session_type: "shiftable_load",
    status: "ACTIVE",
    estimated_cost_eur: 0.12,
    estimated_co2_g: 30,
    interruptible: false,
    tolerance_min: null,
    budget_eur: null,
    created_at: "2026-04-11T06:00:00Z",
    updated_at: "2026-04-11T06:00:00Z",
    session: {
      type: "shiftable_load",
      id: `sess-${id}`,
      asset_id: "wm",
      power_kw: 2.0,
      duration_min: 60,
      earliest_start: "2026-04-11T10:00:00Z",
      latest_end: "2026-04-11T16:00:00Z",
      created_at: "2026-04-11T06:00:00Z",
      updated_at: "2026-04-11T06:00:00Z",
    },
    ...overrides,
  };
}

// ─── Mocks ───────────────────────────────────────────────────────────────────

const mockRequestsData = vi.fn((): UserRequestWithSession[] => []);
const mockEvSettingsData = vi.fn((): EvSettings => ({
  opportunistic_charging_enabled: true,
  paused_by_active_session: false,
}));
const mockPostRequest = vi.fn();
const mockDeleteRequest = vi.fn();
const mockPutEvSettings = vi.fn();

vi.mock("../api/hooks", () => ({
  useRequests: () => ({
    data: mockRequestsData(),
    isLoading: false,
    isError: false,
    error: null,
  }),
  useEvSettings: () => ({
    data: mockEvSettingsData(),
    isLoading: false,
  }),
  usePostRequest: () => ({
    mutateAsync: mockPostRequest,
    isPending: false,
  }),
  useDeleteRequest: () => ({
    mutateAsync: mockDeleteRequest,
    isPending: false,
  }),
  usePutEvSettings: () => ({
    mutate: mockPutEvSettings,
    isPending: false,
  }),
}));

// ─── Wrapper ─────────────────────────────────────────────────────────────────

function renderPage() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <BrowserRouter>
        <DevicesPage />
      </BrowserRouter>
    </QueryClientProvider>,
  );
}

// ─── Tests ───────────────────────────────────────────────────────────────────

describe("DevicesPage", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockRequestsData.mockReturnValue([]);
    mockEvSettingsData.mockReturnValue({
      opportunistic_charging_enabled: true,
      paused_by_active_session: false,
    });
  });

  // 1. All idle
  it("shows idle state for all devices when no active requests", () => {
    renderPage();
    expect(screen.getByTestId("ev-idle-view")).toBeInTheDocument();
    expect(screen.getByTestId("ev-plan-btn")).toBeInTheDocument();
    expect(screen.getByTestId("heater-idle-view")).toBeInTheDocument();
    expect(screen.getByTestId("heater-set-btn")).toBeInTheDocument();
    expect(screen.getByTestId("shiftable-empty")).toBeInTheDocument();
  });

  // 2. EV active request
  it("shows active EV view when an active EV request exists", () => {
    mockRequestsData.mockReturnValue([makeEvRequest()]);
    renderPage();
    expect(screen.getByTestId("ev-active-view")).toBeInTheDocument();
    expect(screen.getByTestId("ev-target-soc")).toHaveTextContent("80%");
    expect(screen.getByTestId("ev-unplan-btn")).toBeInTheDocument();
  });

  // 3. Click Plan Charging opens dialog
  it("opens Plan Charging dialog when button clicked", async () => {
    const user = userEvent.setup();
    renderPage();
    await user.click(screen.getByTestId("ev-plan-btn"));
    expect(screen.getByTestId("ev-dialog")).toBeInTheDocument();
  });

  // 4. Confirm EV dialog calls postRequest
  it("calls postRequest with EV body on dialog confirm", async () => {
    const user = userEvent.setup();
    renderPage();
    await user.click(screen.getByTestId("ev-plan-btn"));
    await user.click(screen.getByTestId("ev-dialog-confirm"));
    expect(mockPostRequest).toHaveBeenCalledWith(
      expect.objectContaining({
        asset_id: "ev",
        target_soc: expect.any(Number),
        deadlines: expect.any(Array),
      }),
    );
  });

  // 5. Click Unplan calls deleteRequest
  it("calls deleteRequest when Unplan clicked", async () => {
    mockRequestsData.mockReturnValue([makeEvRequest()]);
    const user = userEvent.setup();
    renderPage();
    await user.click(screen.getByTestId("ev-unplan-btn"));
    expect(mockDeleteRequest).toHaveBeenCalledWith("ur-ev-001");
  });

  // 6. Heater active request
  it("shows active heater view when an active heater request exists", () => {
    mockRequestsData.mockReturnValue([makeHeaterRequest()]);
    renderPage();
    expect(screen.getByTestId("heater-active-view")).toBeInTheDocument();
    expect(screen.getByTestId("heater-temp")).toHaveTextContent("55°C");
  });

  // 7. Click Set Target opens dialog
  it("opens Set Target dialog when button clicked", async () => {
    const user = userEvent.setup();
    renderPage();
    await user.click(screen.getByTestId("heater-set-btn"));
    expect(screen.getByTestId("heater-dialog")).toBeInTheDocument();
  });

  // 8. Confirm heater dialog calls postRequest
  it("calls postRequest with heater body on dialog confirm", async () => {
    const user = userEvent.setup();
    renderPage();
    await user.click(screen.getByTestId("heater-set-btn"));
    await user.click(screen.getByTestId("heater-dialog-confirm"));
    expect(mockPostRequest).toHaveBeenCalledWith(
      expect.objectContaining({
        asset_id: "heater",
        target_temp_c: expect.any(Number),
        deadlines: expect.any(Array),
      }),
    );
  });

  // 9. Click Clear calls deleteRequest
  it("calls deleteRequest when Clear clicked on heater", async () => {
    mockRequestsData.mockReturnValue([makeHeaterRequest()]);
    const user = userEvent.setup();
    renderPage();
    await user.click(screen.getByTestId("heater-clear-btn"));
    expect(mockDeleteRequest).toHaveBeenCalledWith("ur-ht-001");
  });

  // 10. Shiftable load row
  it("shows shiftable load row with correct data", () => {
    mockRequestsData.mockReturnValue([makeShiftableRequest("sl-001")]);
    renderPage();
    const row = screen.getByTestId("shiftable-row-sl-001");
    expect(row).toBeInTheDocument();
    expect(within(row).getByText(/wm/)).toBeInTheDocument();
    expect(within(row).getByText(/2/)).toBeInTheDocument();
    expect(within(row).getByText(/60/)).toBeInTheDocument();
  });

  // 11. Click Add Load opens dialog
  it("opens Add Load dialog when button clicked", async () => {
    const user = userEvent.setup();
    renderPage();
    await user.click(screen.getByTestId("shiftable-add-btn"));
    expect(screen.getByTestId("shiftable-dialog")).toBeInTheDocument();
  });

  // 12. Confirm Add Load calls postRequest
  it("calls postRequest with shiftable body on dialog confirm", async () => {
    const user = userEvent.setup();
    renderPage();
    await user.click(screen.getByTestId("shiftable-add-btn"));
    await user.click(screen.getByTestId("shiftable-dialog-confirm"));
    expect(mockPostRequest).toHaveBeenCalledWith(
      expect.objectContaining({
        power_kw: expect.any(Number),
        duration_min: expect.any(Number),
      }),
    );
  });

  // 13. Click [×] on shiftable load calls deleteRequest
  it("calls deleteRequest when cancel clicked on shiftable load", async () => {
    mockRequestsData.mockReturnValue([makeShiftableRequest("sl-001")]);
    const user = userEvent.setup();
    renderPage();
    await user.click(screen.getByTestId("shiftable-cancel-sl-001"));
    expect(mockDeleteRequest).toHaveBeenCalledWith("sl-001");
  });

  // 14. Surplus toggle rendered and checked
  it("renders surplus toggle switch that is checked", () => {
    renderPage();
    const sw = screen.getByTestId("ev-opportunistic-charging-switch");
    expect(sw).toBeInTheDocument();
    // MUI Switch: data-testid is on span wrapper; query the underlying input for checked state
    const input = sw.querySelector("input[type='checkbox']")!;
    expect(input).toBeChecked();
  });

  // 15. Toggle surplus calls putEvSettings
  it("calls putEvSettings when surplus toggle clicked", async () => {
    const user = userEvent.setup();
    renderPage();
    await user.click(screen.getByTestId("ev-opportunistic-charging-switch"));
    expect(mockPutEvSettings).toHaveBeenCalledWith({ opportunistic_charging_enabled: false });
  });

  // 16. Paused state: chip shown, switch disabled
  it("shows paused chip and disables switch when paused_by_active_session", () => {
    mockEvSettingsData.mockReturnValue({
      opportunistic_charging_enabled: true,
      paused_by_active_session: true,
    });
    renderPage();
    expect(screen.getByTestId("ev-opportunistic-paused-chip")).toBeInTheDocument();
    // MUI Switch uses aria-disabled on the span wrapper, not the HTML disabled attribute
    const sw = screen.getByTestId("ev-opportunistic-charging-switch");
    expect(sw).toHaveAttribute("aria-disabled", "true");
  });

  // 17. All Requests accordion expands
  it("expands All Requests accordion and shows table", async () => {
    mockRequestsData.mockReturnValue([makeEvRequest(), makeHeaterRequest()]);
    const user = userEvent.setup();
    renderPage();
    const accordion = screen.getByTestId("all-requests-accordion");
    expect(accordion).toBeInTheDocument();
    // Click to expand
    await user.click(within(accordion).getByRole("button"));
    expect(screen.getByTestId("all-requests-table")).toBeInTheDocument();
    expect(screen.getByTestId("all-requests-row-ur-ev-001")).toBeInTheDocument();
    expect(screen.getByTestId("all-requests-row-ur-ht-001")).toBeInTheDocument();
  });

  // 18. Cancel in All Requests — enabled for ACTIVE, disabled for non-ACTIVE
  it("cancel in All Requests is enabled for ACTIVE and disabled for done", async () => {
    const done = makeEvRequest({ id: "ur-done", status: "COMPLETED" });
    const active = makeHeaterRequest();
    mockRequestsData.mockReturnValue([active, done]);
    const user = userEvent.setup();
    renderPage();
    // Expand accordion
    await user.click(within(screen.getByTestId("all-requests-accordion")).getByRole("button"));
    // Active row cancel is enabled
    expect(screen.getByTestId("all-requests-cancel-ur-ht-001")).not.toBeDisabled();
    // Done row cancel is disabled
    expect(screen.getByTestId("all-requests-cancel-ur-done")).toBeDisabled();
  });

  // Additional: EV soft_deadline chip
  it("shows soft deadline chip when EV session has soft_deadline", () => {
    const evReq = makeEvRequest();
    if (evReq.session && evReq.session.type === "ev") {
      evReq.session.soft_deadline = true;
    }
    mockRequestsData.mockReturnValue([evReq]);
    renderPage();
    expect(screen.getByTestId("ev-soft-deadline-chip")).toBeInTheDocument();
  });

  // Additional: EV estimated cost
  it("shows estimated cost on active EV card", () => {
    mockRequestsData.mockReturnValue([makeEvRequest()]);
    renderPage();
    expect(screen.getByTestId("ev-estimated-cost")).toHaveTextContent("€1.23");
  });
});
