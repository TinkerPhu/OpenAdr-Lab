import { describe, expect, it } from "vitest";
import {
  COLOR_ASSET_FALLBACK,
  KIND_COLORS,
  SHIFTABLE_LOAD_DISPLAY,
  assetColor,
  assetLabel,
  assetLegend,
} from "../components/controller/assetDisplay";
import { makeSim } from "./fixtures/sim";

describe("assetDisplay: shown by kind, never by id", () => {
  it("labels and colours an asset by its kind whatever its id", () => {
    const sim = makeSim({ "car-a": { asset_type: "ev" }, roof: { asset_type: "pv" } });
    expect(assetLabel(sim, "car-a")).toBe("EV");
    expect(assetColor(sim, "car-a")).toBe(KIND_COLORS.ev);
    expect(assetLabel(sim, "roof")).toBe("PV");
    expect(assetColor(sim, "roof")).toBe(KIND_COLORS.pv);
  });

  it("adds the id when the VEN holds more than one asset of that kind", () => {
    const sim = makeSim({
      "pv-east": { asset_type: "pv" },
      "pv-west": { asset_type: "pv" },
      battery: { asset_type: "battery" },
    });
    expect(assetLabel(sim, "pv-east")).toBe("PV · pv-east");
    expect(assetLabel(sim, "pv-west")).toBe("PV · pv-west");
    expect(assetLabel(sim, "battery")).toBe("Battery");
  });

  it("names a known shiftable load by its own display entry", () => {
    const sim = makeSim({ wm: { asset_type: "shiftable_load" }, sauna: { asset_type: "shiftable_load" } });
    expect(assetLabel(sim, "wm")).toBe(SHIFTABLE_LOAD_DISPLAY.wm.label);
    expect(assetColor(sim, "wm")).toBe(SHIFTABLE_LOAD_DISPLAY.wm.color);
    expect(assetLabel(sim, "sauna")).toBe("sauna");
    expect(assetColor(sim, "sauna")).toBe(KIND_COLORS.shiftable_load);
  });

  it("does not guess a kind from the id of an asset the roster does not hold", () => {
    const sim = makeSim({});
    expect(assetLabel(sim, "ev")).toBe("ev");
    expect(assetColor(sim, "ev")).toBe(COLOR_ASSET_FALLBACK);
  });

  it("builds legend labels with each asset's planning role", () => {
    const sim = makeSim({ "car-a": { asset_type: "ev" }, house: { asset_type: "base_load" } });
    expect(assetLegend(sim, ["car-a", "house"]).labelMap).toEqual({
      "car-a": "EV (planned)",
      house: "Base Load (forecast)",
    });
  });
});
