import { render, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ServiceDelivered } from "../components/ServiceDelivered";
import type { FleetService, FleetServiceReport } from "../api/types";

const CHANGED = "2026-10-22T08:00:00.000Z";
const NOW = Date.parse("2026-10-22T16:00:00Z");

const report = (unserved: number | null, carry: number | null, vens = 20): FleetServiceReport => ({
  unservedEnergyKwh: { kwh: unserved, contributingVens: unserved === null ? 0 : vens },
  carryOverKwh: { kwh: carry, contributingVens: carry === null ? 0 : vens },
  vens: [
    {
      venName: "ven-1",
      unservedEnergyKwh: unserved,
      carryOverKwh: carry,
      assets: [{ assetId: "ev", unservedEnergyKwh: unserved, carryOverKwh: carry }],
    },
  ],
});
const answer = (r: FleetServiceReport): FleetService => ({
  from: "",
  to: "",
  available: true,
  report: r,
});

const queries: Record<string, { data?: FleetService; isError?: boolean }> = {};
const asked: [string, string][] = [];
vi.mock("../api/hooks", () => ({
  useFleetService: (from: string, to: string) => {
    asked.push([from, to]);
    return queries[from < CHANGED ? "before" : "after"] ?? {};
  },
}));

const show = () => render(<ServiceDelivered changedAt={CHANGED} nowMs={NOW} />);

describe("ServiceDelivered", () => {
  beforeEach(() => {
    asked.length = 0;
    for (const k of Object.keys(queries)) delete queries[k];
  });

  it("asks for two windows of equal length around the switch", () => {
    queries.before = { data: answer(report(0, -2)) };
    queries.after = { data: answer(report(9, -30)) };
    show();
    const lengths = asked.map(([from, to]) => Date.parse(to) - Date.parse(from));
    expect(new Set(lengths).size).toBe(1);
    expect(lengths[0]).toBe(8 * 3600_000);
  });

  it("shows more unserved energy after than before", () => {
    queries.before = { data: answer(report(0, -2)) };
    queries.after = { data: answer(report(21, -30)) };
    show();
    expect(screen.getByTestId("service-unserved")).toHaveTextContent("0.0 kWh → 21.0 kWh");
    expect(screen.getByTestId("service-carry-over")).toHaveTextContent("-2.0 kWh → -30.0 kWh");
  });

  it("says how many VENs each side is built from when a VEN dropped out", () => {
    queries.before = { data: answer(report(0, 0, 20)) };
    queries.after = { data: answer(report(5, 0, 19)) };
    show();
    expect(screen.getByTestId("service-contributors")).toHaveTextContent("19 VENs after, 20 before");
  });

  it("says the window is unavailable, and why, instead of showing zero", () => {
    queries.before = {
      data: { from: "", to: "", available: false, reason: "the window starts before the raw telemetry horizon" },
    };
    queries.after = { data: answer(report(5, 0)) };
    show();
    expect(screen.getByTestId("service-unavailable")).toHaveTextContent("raw telemetry horizon");
    expect(screen.queryByTestId("service-unserved")).toBeNull();
  });

  it("says no data where a window holds no declared figure", () => {
    queries.before = { data: answer(report(null, null)) };
    queries.after = { data: answer(report(null, null)) };
    show();
    expect(screen.getByTestId("service-unserved")).toHaveTextContent("no data");
  });

  it("lists each VEN's assets in a drill-down", () => {
    queries.before = { data: answer(report(0, 0)) };
    queries.after = { data: answer(report(15, -4)) };
    show();
    const row = screen.getByTestId("service-asset-ven-1-ev");
    expect(within(row).getByText("ven-1")).toBeInTheDocument();
    expect(row).toHaveTextContent("0.0 kWh → 15.0 kWh");
  });

  it("explains the store is not connected on an error", () => {
    queries.before = { isError: true };
    queries.after = { isError: true };
    show();
    expect(screen.getByTestId("service-error")).toBeInTheDocument();
  });
});
