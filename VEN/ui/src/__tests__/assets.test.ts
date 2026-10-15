import { describe, expect, it } from "vitest";
import { assetsOfType, assetsInRosterOrder, stackOrder } from "../api/assets";
import { makeSim } from "./fixtures/sim";

describe("assetsOfType (W6)", () => {
  it("lists the assets of one type in roster order, whatever their ids", () => {
    const sim = makeSim({
      "car-b": { asset_type: "ev", roster_index: 3 },
      house: { asset_type: "base_load", roster_index: 0 },
      "car-a": { asset_type: "ev", roster_index: 1 },
    });
    expect(assetsOfType(sim, "ev").map(([id]) => id)).toEqual(["car-a", "car-b"]);
    expect(assetsOfType(sim, "battery")).toEqual([]);
    expect(assetsInRosterOrder(sim).map(([id]) => id)).toEqual(["house", "car-a", "car-b"]);
  });

  it("stacks generation nearest the axis, by type and whatever the ids", () => {
    const types: Record<string, string> = {
      "car-a": "ev",
      "roof-west": "pv",
      house: "base_load",
      "roof-east": "pv",
      wm: "shiftable_load",
    };
    expect(stackOrder(["car-a", "roof-west", "house", "roof-east", "wm"], (id) => types[id])).toEqual(
      ["roof-west", "roof-east", "house", "car-a", "wm"],
    );
  });

  it("is empty without a snapshot", () => {
    expect(assetsOfType(undefined, "ev")).toEqual([]);
  });
});
