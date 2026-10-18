import { fireEvent, render, screen } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { PublishDialog } from "../components/PublishDialog";
import { draftFromPattern, type SignalDraft } from "../utils/signalDraft";
import { buildSignalRegistry } from "../utils/signalRegistry";

const createMock = vi.fn();
vi.mock("../api/hooks", () => ({
  useCreateEvent: () => ({ mutate: createMock, isPending: false, isError: false }),
}));

const REGISTRY = buildSignalRegistry([
  { payloadType: "PRICE", units: "KWH", currency: "EUR" },
  { payloadType: "EXPORT_PRICE", units: "KWH", currency: "EUR" },
]);

const MIDNIGHT = Date.parse("2026-10-18T00:00:00Z");
const HOUR = 3_600_000;

function dayDraft(over: Partial<SignalDraft> = {}): SignalDraft {
  const d = draftFromPattern(
    { programID: "prog-1", eventName: "tou-pricing-day-ahead", duration: "P9999Y", targets: [] },
    Array.from({ length: 24 }, (_, h) => ({
      start: new Date(MIDNIGHT + h * HOUR).toISOString(),
      end: new Date(MIDNIGHT + (h + 1) * HOUR).toISOString(),
      payloads: [{ type: "PRICE", value: 0.1 }],
    })),
    REGISTRY,
  );
  return { ...d, ...over };
}

function open(draft: SignalDraft, replacedPriorities: (number | null)[]) {
  const onClose = vi.fn();
  render(
    <PublishDialog
      open
      draft={draft}
      registry={REGISTRY}
      replacedPriorities={replacedPriorities}
      onClose={onClose}
    />,
  );
  return onClose;
}

describe("PublishDialog", () => {
  beforeEach(() => createMock.mockClear());

  it("says who the event reaches: everyone when there are no targets", () => {
    open(dayDraft(), [null]);
    expect(screen.getByTestId("publish-targets")).toHaveTextContent("all VENs");
  });

  it("names the targeted VENs", () => {
    open(dayDraft({ targets: ["ven-1", "ven-2"] }), [null]);
    expect(screen.getByTestId("publish-targets")).toHaveTextContent("ven-1, ven-2");
  });

  it("shows the priority it will take and what it beats", () => {
    open(dayDraft(), [5, 3, null]);
    expect(screen.getByTestId("publish-priority")).toHaveTextContent("2");
    expect(screen.queryByTestId("publish-tie-note")).toBeNull();
  });

  it("explains the tie when an existing event already holds priority 0", () => {
    open(dayDraft(), [0]);
    expect(screen.getByTestId("publish-priority")).toHaveTextContent("0");
    expect(screen.getByTestId("publish-tie-note")).toHaveTextContent("newer");
  });

  it("states that existing events are left unchanged", () => {
    open(dayDraft(), [null]);
    expect(screen.getByTestId("publish-supersede-note")).toHaveTextContent("left unchanged");
  });

  it("posts the event with its contract and closes on success", () => {
    const onClose = open(dayDraft(), [null]);
    fireEvent.click(screen.getByTestId("publish-confirm"));
    expect(createMock).toHaveBeenCalledTimes(1);
    const [input, opts] = createMock.mock.calls[0];
    expect(input).toMatchObject({
      programID: "prog-1",
      eventName: "tou-pricing-day-ahead",
      priority: 0,
      duration: "P9999Y",
      payloadDescriptors: [{ payloadType: "PRICE", units: "KWH", currency: "EUR" }],
    });
    expect(input.intervals).toHaveLength(24);
    opts.onSuccess();
    expect(onClose).toHaveBeenCalled();
  });

  it("will not publish an invalid draft", () => {
    const draft = dayDraft();
    draft.intervals[0] = { ...draft.intervals[0], values: { PRICE: null } };
    open(draft, [null]);
    expect(screen.getByTestId("publish-confirm")).toBeDisabled();
  });
});
