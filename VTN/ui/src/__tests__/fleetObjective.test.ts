import { describe, expect, it } from "vitest";
import {
  fleetObjectiveChoices, latestObjectiveChange, objectiveCounts, objectiveMix, sourceLabel,
} from "../utils/fleetObjective";
import { instantBefore } from "../utils/fleetPlan";
import type { FleetVenLive } from "../api/types";

function ven(name: string, available?: string[]): FleetVenLive {
  return {
    venName: name,
    netPowerKw: 0,
    state: "online",
    receivedAt: "2026-10-17T12:00:00Z",
    objective: available
      ? {
          objective: available[0],
          source: "profile",
          set_at: null,
          profile_objective: available[0],
          available_objectives: available,
        }
      : null,
  };
}

describe("fleetObjectiveChoices", () => {
  it("offers what every declaring VEN accepts and ignores VENs that declare nothing", () => {
    const vens = [
      ven("ven-1", ["min_cost", "opportunistic", "min_ghg"]),
      ven("ven-2", ["min_cost", "opportunistic"]),
      ven("ven-3"),
    ];
    expect(fleetObjectiveChoices(vens)).toEqual(["min_cost", "opportunistic"]);
  });

  it("offers nothing when no VEN declares its objectives", () => {
    expect(fleetObjectiveChoices([ven("ven-1")])).toEqual([]);
  });
});

describe("objectiveMix", () => {
  it("lists the objectives behind a slot, most VENs first", () => {
    expect(objectiveMix({ min_cost: 2, opportunistic: 3, unknown: 2 })).toBe(
      "opportunistic 3 · min_cost 2 · unknown 2",
    );
  });
});

describe("objectiveCounts", () => {
  it("counts the VENs under each objective in force and skips VENs that report none", () => {
    const vens = [ven("ven-1", ["opportunistic"]), ven("ven-2", ["min_cost"]), ven("ven-3", ["opportunistic"]), ven("ven-4")];
    expect(objectiveCounts(vens)).toEqual({ opportunistic: 2, min_cost: 1 });
  });
});

describe("latestObjectiveChange", () => {
  it("is the newest change any VEN reported, or none", () => {
    const vens = [
      { ...ven("ven-1"), objectiveChangedAt: "2026-10-17T12:05:00Z" },
      { ...ven("ven-2"), objectiveChangedAt: "2026-10-17T12:20:00+02:00" },
      { ...ven("ven-3"), objectiveChangedAt: null },
    ];
    expect(latestObjectiveChange(vens)).toBe("2026-10-17T12:05:00Z");
    expect(latestObjectiveChange([ven("ven-1")])).toBeUndefined();
  });
});

describe("instantBefore", () => {
  it("is one second before the change, so a plan adopted in answer to it is not counted", () => {
    expect(instantBefore("2026-10-17T12:05:00Z")).toBe("2026-10-17T12:04:59.000Z");
  });
});

describe("sourceLabel", () => {
  it("says who set the objective in the VEN UI's words", () => {
    expect(sourceLabel("ven_ui")).toBe("VEN UI");
    expect(sourceLabel("fleet")).toBe("fleet");
  });
});
