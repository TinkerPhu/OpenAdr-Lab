import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { TariffPage } from "../pages/Tariff";
import { buildSignalRegistry } from "../utils/signalRegistry";
import type { PatternInterval } from "../api/types";

const MIDNIGHT = Date.parse("2026-10-18T00:00:00Z");
const HOUR = 3_600_000;

const REGISTRY = buildSignalRegistry([
  { payloadType: "PRICE", units: "KWH", currency: "EUR" },
  { payloadType: "EXPORT_PRICE", units: "KWH", currency: "EUR" },
]);

const SEED_EVENT = {
  id: "ev-seed",
  programID: "prog-1",
  eventName: "tou-pricing-day-ahead",
  priority: null,
  duration: "P9999Y",
  targets: [],
  createdDateTime: "2026-10-01T00:00:00Z",
  intervals: [{ id: 0, payloads: [{ type: "PRICE", values: [0.08] }] }],
};
const OTHER_EVENT = {
  id: "ev-cap",
  programID: "prog-1",
  eventName: "cap",
  intervals: [{ id: 0, payloads: [{ type: "IMPORT_CAPACITY_LIMIT", values: [5] }] }],
};

const PATTERN: PatternInterval[] = Array.from({ length: 24 }, (_, h) => ({
  start: new Date(MIDNIGHT + h * HOUR).toISOString(),
  end: new Date(MIDNIGHT + (h + 1) * HOUR).toISOString(),
  payloads: [{ type: "PRICE", value: 0.08 }],
}));

let eventsData: Record<string, unknown>[] = [SEED_EVENT, OTHER_EVENT];

vi.mock("../components/PlanComparison", () => ({
  PlanComparison: (props: { publishedAt: string; targets: string[] }) => (
    <div data-testid="plan-comparison-stub" data-published-at={props.publishedAt} data-targets={props.targets.join(",")} />
  ),
}));

const loadMock = vi.fn();
const createMock = vi.fn();
const previewArgs: unknown[] = [];

vi.mock("../api/hooks", () => ({
  useSignalRegistry: () => ({ data: REGISTRY, isError: false }),
  useEvents: () => ({ data: eventsData }),
  useEventPattern: () => ({ mutate: loadMock, isError: false }),
  useSignalPreview: (event: unknown) => {
    previewArgs.push(event);
    return {
      data: event
        ? {
            bands: [
              {
                from: new Date(MIDNIGHT).toISOString(),
                to: new Date(MIDNIGHT + HOUR).toISOString(),
                eventID: "draft",
                eventName: "d",
                payloadType: "PRICE",
                value: 0.5,
              },
            ],
            truncated: false,
            maxLoopedIntervals: 2000,
          }
        : undefined,
    };
  },
  useFleetSignals: () => ({ data: { from: "", to: "", vens: [], rejectedEvents: 0 } }),
  useFleetHistory: () => ({ data: undefined }),
  useFleetPower: () => ({ data: undefined }),
  useCreateEvent: () => ({ mutate: createMock, isPending: false, isError: false }),
}));

const chartProps: Record<string, unknown>[] = [];
vi.mock("../components/FleetTariffChart", () => ({
  TARIFF_AXIS_ID: "tariff",
  FleetTariffChart: (props: Record<string, unknown>) => {
    chartProps.push(props);
    return <div data-testid="tariff-chart-stub" />;
  },
}));
vi.mock("../components/FleetPowerChart", () => ({
  FleetPowerChart: () => <div data-testid="power-chart-stub" />,
}));

function chooseSeed() {
  loadMock.mockImplementation((_event, opts) => opts.onSuccess(PATTERN));
  fireEvent.change(screen.getByTestId("tariff-event-select"), { target: { value: "ev-seed" } });
}

describe("TariffPage", () => {
  beforeEach(() => {
    eventsData = [SEED_EVENT, OTHER_EVENT];
    loadMock.mockReset();
    createMock.mockReset();
    chartProps.length = 0;
    previewArgs.length = 0;
  });

  it("offers only events whose signals the editor can edit", () => {
    render(<TariffPage />);
    const options = Array.from(
      screen.getByTestId("tariff-event-select").querySelectorAll("option"),
    ).map((o) => o.textContent);
    expect(options.some((t) => t?.includes("tou-pricing-day-ahead"))).toBe(true);
    expect(options.some((t) => t?.includes("cap"))).toBe(false);
  });

  it("loads the seeded tariff as 24 rows that repeat forever", async () => {
    render(<TariffPage />);
    chooseSeed();
    await waitFor(() => expect(screen.getAllByTestId(/^cell-PRICE-/)).toHaveLength(24));
    expect(screen.getByTestId("recurrence-kind")).toHaveValue("forever");
    expect(screen.getByTestId("recurrence-period")).toHaveTextContent("repeats every 24 h");
  });

  it("draws the previewed draft beside the live tariff, and looks ahead", async () => {
    render(<TariffPage />);
    chooseSeed();
    // The preview is debounced, so the draft's bands arrive a moment after the chart first renders.
    await waitFor(() => {
      const latest = chartProps[chartProps.length - 1];
      expect((latest?.draftBands as unknown[] | undefined)?.length).toBe(1);
    });
    const last = chartProps[chartProps.length - 1];
    expect(last.aheadMinutes).toBeGreaterThan(0);
    // The chart is also where the draft is edited with the pointer.
    expect(last.overlay).toBeTruthy();
  });

  it("asks for a preview only while the draft is publishable", async () => {
    render(<TariffPage />);
    chooseSeed();
    await waitFor(() => expect(screen.getAllByTestId(/^cell-PRICE-/)).toHaveLength(24));
    await waitFor(() => expect(previewArgs[previewArgs.length - 1]).not.toBeNull());
    fireEvent.change(screen.getByTestId("cell-PRICE-0"), { target: { value: "" } });
    await waitFor(() => expect(previewArgs[previewArgs.length - 1]).toBeNull());
    expect(screen.getByTestId("tariff-issues")).toBeVisible();
    expect(screen.getByTestId("tariff-publish-btn")).toBeDisabled();
  });

  it("shapes the whole series with an operator and keeps it a repeating tariff", async () => {
    render(<TariffPage />);
    chooseSeed();
    await waitFor(() => expect(screen.getAllByTestId(/^cell-PRICE-/)).toHaveLength(24));
    fireEvent.change(screen.getByTestId("op-param-factor"), { target: { value: "2" } });
    fireEvent.click(screen.getByTestId("operator-apply"));
    await waitFor(() =>
      expect((screen.getByTestId("cell-PRICE-0") as HTMLInputElement).value).toBe("0.16"),
    );
    expect(screen.getByTestId("recurrence-kind")).toHaveValue("forever");
  });

  it("offers no before/after comparison when the tariff was not published recently", async () => {
    render(<TariffPage />);
    chooseSeed(); // the seeded event is weeks old
    await waitFor(() => expect(screen.getAllByTestId(/^cell-PRICE-/)).toHaveLength(24));
    expect(screen.queryByTestId("tariff-comparison-card")).toBeNull();
  });

  it("compares the fleet's plans before and after the newest recent tariff, from the VTN's own record", async () => {
    const publishedAt = new Date(Date.now() - 10 * 60_000).toISOString();
    eventsData = [
      SEED_EVENT,
      OTHER_EVENT,
      { ...SEED_EVENT, id: "ev-new", eventName: "tou-edited", createdDateTime: publishedAt, targets: ["ven-1"] },
    ];
    render(<TariffPage />);
    chooseSeed();
    const stub = await screen.findByTestId("plan-comparison-stub");
    expect(stub).toHaveAttribute("data-published-at", publishedAt);
    expect(stub).toHaveAttribute("data-targets", "ven-1");
    expect(screen.getByTestId("tariff-comparison-card")).toHaveTextContent("tou-edited");
  });

  it("ignores a recent event that carries none of the draft's signals", async () => {
    eventsData = [
      SEED_EVENT,
      { ...OTHER_EVENT, id: "ev-cap-new", createdDateTime: new Date(Date.now() - 60_000).toISOString() },
    ];
    render(<TariffPage />);
    chooseSeed();
    await waitFor(() => expect(screen.getAllByTestId(/^cell-PRICE-/)).toHaveLength(24));
    expect(screen.queryByTestId("tariff-comparison-card")).toBeNull();
  });

  it("discards edits back to the loaded series", async () => {
    render(<TariffPage />);
    chooseSeed();
    await waitFor(() => expect(screen.getAllByTestId(/^cell-PRICE-/)).toHaveLength(24));
    fireEvent.change(screen.getByTestId("cell-PRICE-3"), { target: { value: "0.4" } });
    expect((screen.getByTestId("cell-PRICE-3") as HTMLInputElement).value).toBe("0.4");
    fireEvent.click(screen.getByTestId("tariff-discard-btn"));
    await waitFor(() =>
      expect((screen.getByTestId("cell-PRICE-3") as HTMLInputElement).value).toBe("0.08"),
    );
  });

  it("publishes the edited draft and says when", async () => {
    render(<TariffPage />);
    chooseSeed();
    await waitFor(() => expect(screen.getAllByTestId(/^cell-PRICE-/)).toHaveLength(24));
    fireEvent.change(screen.getByTestId("cell-PRICE-17"), { target: { value: "0.45" } });
    fireEvent.click(screen.getByTestId("tariff-publish-btn"));
    fireEvent.click(await screen.findByTestId("publish-confirm"));
    const [input, opts] = createMock.mock.calls[0];
    expect(input.duration).toBe("P9999Y");
    expect(input.intervals[17].payloads[0].values).toEqual([0.45]);
    opts.onSuccess();
    expect(await screen.findByTestId("tariff-published-note")).toHaveTextContent("Published at");
  });
});
