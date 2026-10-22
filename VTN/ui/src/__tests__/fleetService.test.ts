import { describe, expect, it } from "vitest";
import { serviceWindows, joinAssets } from "../utils/fleetService";
import type { FleetServiceReport } from "../api/types";

const CHANGED = "2026-10-22T08:00:00.000Z";
const at = (iso: string) => Date.parse(iso);

describe("serviceWindows", () => {
  it("compares the span since the switch with the equal span before it", () => {
    const w = serviceWindows(CHANGED, at("2026-10-22T16:00:00Z"));
    expect(w?.after).toEqual({ from: CHANGED, to: "2026-10-22T16:00:00.000Z" });
    expect(w?.before).toEqual({ from: "2026-10-22T00:00:00.000Z", to: CHANGED });
  });

  it("rounds the end down to five minutes, so the windows do not change every second", () => {
    const w = serviceWindows(CHANGED, at("2026-10-22T09:07:41Z"));
    expect(w?.after.to).toBe("2026-10-22T09:05:00.000Z");
    expect(Date.parse(w!.before.to) - Date.parse(w!.before.from)).toBe(65 * 60_000);
  });

  it("says nothing until the switch is five minutes old", () => {
    expect(serviceWindows(CHANGED, at("2026-10-22T08:04:59Z"))).toBeUndefined();
  });
});

const report = (assets: [string, string, number | null, number | null][]): FleetServiceReport => ({
  unservedEnergyKwh: { kwh: null, contributingVens: 0 },
  carryOverKwh: { kwh: null, contributingVens: 0 },
  vens: [...new Set(assets.map((a) => a[0]))].map((venName) => ({
    venName,
    unservedEnergyKwh: null,
    carryOverKwh: null,
    assets: assets
      .filter((a) => a[0] === venName)
      .map((a) => ({ assetId: a[1], unservedEnergyKwh: a[2], carryOverKwh: a[3] })),
  })),
});

describe("joinAssets", () => {
  it("lines up each VEN's asset before and after, and keeps one that is on only one side", () => {
    const rows = joinAssets(
      report([["ven-1", "ev", 0, -5], ["ven-2", "ev", 3, null]]),
      report([["ven-1", "ev", 12, -20], ["ven-1", "heater", 1, null]]),
    );
    expect(rows.map((r) => `${r.venName}/${r.assetId}`)).toEqual(["ven-1/ev", "ven-1/heater", "ven-2/ev"]);
    expect(rows[0]).toMatchObject({ unserved: { before: 0, after: 12 }, carryOver: { before: -5, after: -20 } });
    expect(rows[1]).toMatchObject({ unserved: { before: null, after: 1 } });
    expect(rows[2]).toMatchObject({ unserved: { before: 3, after: null } });
  });
});
