import { describe, it, expect } from "vitest";
import { formatExportCapabilityValue } from "../components/controller/charts/capabilityFormat";

describe("formatExportCapabilityValue", () => {
  it("shows a real export as a plain signed value", () => {
    expect(formatExportCapabilityValue(-3.24)).toBe("-3.24 kW");
  });

  it("explains a positive value as net import rather than export", () => {
    // ven-3 at night: no battery, no sun, so the site cannot export at all and
    // the curve sits at its base load. A bare "+0.68 kW" under an "Export"
    // legend is what made this look like a bug when first reported.
    // formatPowerValue renders sub-kW magnitudes in watts, so this reads
    // "+680 W" rather than "+0.68 kW" -- the shared unit formatter's choice,
    // not this helper's.
    const text = formatExportCapabilityValue(0.68);
    expect(text).toContain("+680 W");
    expect(text).toContain("net import");
  });

  it("explains a forced-heater spike the same way", () => {
    expect(formatExportCapabilityValue(6.68)).toContain("net import");
  });

  it("leaves zero unannotated — nothing is flowing either way", () => {
    expect(formatExportCapabilityValue(0)).toBe("0 W");
  });
});
