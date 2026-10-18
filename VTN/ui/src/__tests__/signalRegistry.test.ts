import { describe, it, expect } from "vitest";
import { buildSignalRegistry, lookupSignal, UnknownSignalType } from "../utils/signalRegistry";

const WIRE_ROWS = [
  { payloadType: "PRICE", units: "KWH", currency: "EUR" },
  { payloadType: "EXPORT_PRICE", units: "KWH", currency: "EUR" },
  // A type the lab reads on the wire but this editor has no declaration for.
  { payloadType: "IMPORT_CAPACITY_LIMIT", units: "KW", currency: null },
];

describe("buildSignalRegistry", () => {
  const registry = buildSignalRegistry(WIRE_ROWS);

  it("registers PRICE and EXPORT_PRICE with the unit the BFF declared", () => {
    expect(registry.PRICE).toMatchObject({ units: "KWH", currency: "EUR", step: 0.01 });
    expect(registry.EXPORT_PRICE).toMatchObject({ units: "KWH", currency: "EUR" });
  });

  it("does not register a type the editor has no declaration for", () => {
    expect(registry.IMPORT_CAPACITY_LIMIT).toBeUndefined();
  });

  it("does not invent an entry for a type the BFF did not declare", () => {
    expect(buildSignalRegistry([]).PRICE).toBeUndefined();
  });
});

describe("lookupSignal", () => {
  const registry = buildSignalRegistry(WIRE_ROWS);

  it("returns the entry", () => {
    expect(lookupSignal(registry, "PRICE").label).toBe("Import tariff");
  });

  it("names the type it refuses", () => {
    expect(() => lookupSignal(registry, "GHG")).toThrow(UnknownSignalType);
    expect(() => lookupSignal(registry, "GHG")).toThrow(/GHG/);
  });
});
