import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { BrowserRouter } from "react-router-dom";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { UserRequestsPage } from "../pages/UserRequests";
import type { UserRequestWithSession } from "../api/types";

// ─── Mock data ───────────────────────────────────────────────────────────────

const sampleRequest: UserRequestWithSession = {
  id: "ur-001",
  asset_id: "ev",
  target_energy_kwh: 20,
  target_soc: 0.8,
  desired_power_kw: 7.0,
  completion_policy: "CONTINUE",
  deadlines: [
    {
      latest_end: "2026-04-12T07:00:00Z",
      max_total_cost_eur: 3.0,
      max_marginal_rate_eur_kwh: null,
      min_completion: 0.8,
    },
  ],
  max_total_cost_eur: null,
  tier_count: 1,
  session_id: "sess-001",
  session_type: "ev",
  status: "ACTIVE",
  estimated_cost_eur: 1.42,
  estimated_co2_g: 350.0,
  interruptible: false,
  tolerance_min: null,
  budget_eur: null,
  created_at: "2026-04-11T06:00:00Z",
  updated_at: "2026-04-11T06:00:00Z",
  session: null,
};

const completedRequest: UserRequestWithSession = {
  ...sampleRequest,
  id: "ur-002",
  asset_id: "heater",
  target_soc: null,
  target_energy_kwh: 5.0,
  status: "COMPLETED",
  estimated_cost_eur: 0.85,
  estimated_co2_g: 120.0,
  session_type: "heater",
};

// ─── Mocks ───────────────────────────────────────────────────────────────────

const mockRequests = vi.fn((): UserRequestWithSession[] => []);
const mockPostRequest = vi.fn();
const mockDeleteRequest = vi.fn();

vi.mock("../api/hooks", () => ({
  useRequests: () => ({
    data: mockRequests(),
    isLoading: false,
    isError: false,
    error: null,
  }),
  usePostRequest: () => ({
    mutateAsync: mockPostRequest,
    isPending: false,
  }),
  useDeleteRequest: () => ({
    mutateAsync: mockDeleteRequest,
    isPending: false,
  }),
}));

// ─── Wrapper ─────────────────────────────────────────────────────────────────

function renderPage() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <BrowserRouter>
        <UserRequestsPage />
      </BrowserRouter>
    </QueryClientProvider>,
  );
}

// ─── Tests ───────────────────────────────────────────────────────────────────

describe("UserRequestsPage", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockRequests.mockReturnValue([]);
  });

  it("renders heading and 'New User Request' button", () => {
    renderPage();
    expect(screen.getByText("User Requests")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /new user request/i })).toBeInTheDocument();
  });

  it("shows empty message when no requests exist", () => {
    renderPage();
    expect(screen.getByText("No user requests yet")).toBeInTheDocument();
  });

  it("renders request rows with correct data", () => {
    mockRequests.mockReturnValue([sampleRequest, completedRequest]);
    renderPage();

    // Check asset IDs are rendered
    expect(screen.getByText("ev")).toBeInTheDocument();
    expect(screen.getByText("heater")).toBeInTheDocument();

    // Check status chips
    expect(screen.getByText("ACTIVE")).toBeInTheDocument();
    expect(screen.getByText("COMPLETED")).toBeInTheDocument();

    // Check target display: SoC 80% for EV, 5.0 kWh for heater
    expect(screen.getByText("SoC 80%")).toBeInTheDocument();
    expect(screen.getByText("5.0 kWh")).toBeInTheDocument();

    // Check cost display
    expect(screen.getByText("1.420")).toBeInTheDocument();
    expect(screen.getByText("0.850")).toBeInTheDocument();
  });

  it("delete button is disabled for completed requests", () => {
    mockRequests.mockReturnValue([completedRequest]);
    renderPage();

    const btn = screen.getByTestId("DeleteIcon").closest("button")!;
    expect(btn).toBeDisabled();
  });

  it("opens create dialog when 'New User Request' is clicked", async () => {
    const user = userEvent.setup();
    renderPage();

    await user.click(screen.getByRole("button", { name: /new user request/i }));
    expect(screen.getByText("New User Request", { selector: "h2" })).toBeInTheDocument();
    expect(screen.getByLabelText(/request json/i)).toBeInTheDocument();
  });

  it("opens cancel dialog when delete button is clicked on active request", async () => {
    const user = userEvent.setup();
    mockRequests.mockReturnValue([sampleRequest]);
    renderPage();

    const btn = screen.getByTestId("DeleteIcon").closest("button")!;
    await user.click(btn);
    expect(screen.getByText(/cancel user request for asset/i)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /confirm cancel/i })).toBeInTheDocument();
  });

  it("cancel dialog shows asset name", async () => {
    const user = userEvent.setup();
    mockRequests.mockReturnValue([sampleRequest]);
    renderPage();

    const btn = screen.getByTestId("DeleteIcon").closest("button")!;
    await user.click(btn);
    expect(screen.getByText(/cancel user request for asset/i)).toBeInTheDocument();
  });

  it("policy column shows 'CONTINUE'", () => {
    mockRequests.mockReturnValue([sampleRequest]);
    renderPage();
    expect(screen.getByText("CONTINUE")).toBeInTheDocument();
  });

  it("shows CO₂ values", () => {
    mockRequests.mockReturnValue([sampleRequest]);
    renderPage();
    expect(screen.getByText("350.0")).toBeInTheDocument();
  });
});
