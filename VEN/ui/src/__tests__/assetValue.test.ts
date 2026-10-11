import { describe, expect, it } from "vitest";
import { assetValue } from "../api/assetValue";
import { makeAsset } from "./fixtures/sim";

describe("assetValue", () => {
  it("returns a state value the asset reports beside its typed fields", () => {
    expect(assetValue(makeAsset({ soc: 0.42 }), "soc")).toBe(0.42);
  });

  it("is undefined for a value the asset does not report, or no asset at all", () => {
    expect(assetValue(makeAsset(), "soc")).toBeUndefined();
    expect(assetValue(undefined, "soc")).toBeUndefined();
  });

  it("never hands out a typed field that is not a number", () => {
    expect(assetValue(makeAsset({ asset_type: "ev" }), "asset_type")).toBeUndefined();
  });
});
