import { describe, it, expect } from "vitest";
import {
  formatPowerValue,
  formatSignedPowerValue,
  formatCostRateEurH,
  formatCo2RateGH,
  formatCo2IntensityGKwh,
  formatTariffEurKwh,
  formatSocPct,
  formatTemperatureC,
  formatFixedOrDash,
  formatKwOrDash,
} from "@lab/charts/unitFormat";

describe("canonical per-unit formatting", () => {
  it("power: Watts below 1kW, kW at/above, matching the axis-tick rule exactly", () => {
    expect(formatPowerValue(0.45)).toBe("450 W");
    expect(formatPowerValue(1)).toBe("1 kW");
    expect(formatPowerValue(2.345)).toBe("2.35 kW");
    expect(formatPowerValue(-0.003)).toBe("-3 W");
  });

  it("signed power: explicit +/- prefix, from the real value not the rounded string", () => {
    expect(formatSignedPowerValue(1.0)).toBe("+1 kW");
    expect(formatSignedPowerValue(-4.2)).toBe("-4.2 kW");
    expect(formatSignedPowerValue(0)).toBe("0 W");
    // Sub-watt negative residual that rounds to 0 W — sign must still be visible,
    // not silently dropped by the underlying Math.round(-0.2) -> "-0" -> "0" collapse.
    expect(formatSignedPowerValue(-0.0002)).toBe("-0 W");
    expect(formatSignedPowerValue(0.0002)).toBe("+0 W");
  });

  it("cost rate: 4 decimal places, unit €/h", () => {
    expect(formatCostRateEurH(0.05)).toBe("0.0500 €/h");
    expect(formatCostRateEurH(-0.02)).toBe("-0.0200 €/h");
  });

  it("CO2 rate: 1 decimal place, unit g/h", () => {
    expect(formatCo2RateGH(750.44)).toBe("750.4 g/h");
  });

  it("CO2 intensity: 3 decimal places, unit g/kWh, distinct from CO2 rate", () => {
    expect(formatCo2IntensityGKwh(300.1234)).toBe("300.123 g/kWh");
  });

  it("tariff: 4 decimal places, unit €/kWh, never a bare €", () => {
    const out = formatTariffEurKwh(0.2);
    expect(out).toBe("0.2000 €/kWh");
    expect(out).not.toMatch(/^\S+\s€$/);
  });

  it("SoC: input is a 0-1 fraction, displayed as % with 1 decimal", () => {
    expect(formatSocPct(0.755)).toBe("75.5 %");
  });

  it("temperature: 1 decimal place, unit °C", () => {
    expect(formatTemperatureC(21.06)).toBe("21.1 °C");
  });
});

// One rule for "a number that may be missing", replacing five copies in the VEN UI and one in the
// VTN UI (each with its own idea of the dash, of NaN and of the decimals).
describe("formatFixedOrDash", () => {
  it("shows a dash for a missing value: null, undefined and NaN", () => {
    expect(formatFixedOrDash(null, 1)).toBe("—");
    expect(formatFixedOrDash(undefined, 1)).toBe("—");
    expect(formatFixedOrDash(Number.NaN, 1)).toBe("—");
  });

  it("uses exactly the decimals it is given", () => {
    expect(formatFixedOrDash(1.26, 1)).toBe("1.3");
    expect(formatFixedOrDash(1.26, 3)).toBe("1.260");
    expect(formatFixedOrDash(1234.5, 0)).toBe("1235");
  });

  it("keeps zero and negative values", () => {
    expect(formatFixedOrDash(0, 2)).toBe("0.00");
    expect(formatFixedOrDash(-2.5, 1)).toBe("-2.5");
  });
});

describe("formatKwOrDash", () => {
  it("shows a dash, with no unit, for a missing value", () => {
    expect(formatKwOrDash(null)).toBe("—");
    expect(formatKwOrDash(undefined)).toBe("—");
    expect(formatKwOrDash(Number.NaN)).toBe("—");
  });

  it("defaults to two decimals", () => {
    expect(formatKwOrDash(1.5)).toBe("1.50 kW");
    expect(formatKwOrDash(0)).toBe("0.00 kW");
    expect(formatKwOrDash(-2.5)).toBe("-2.50 kW");
  });

  it("takes the decimals when a caller needs fewer", () => {
    expect(formatKwOrDash(1.25, 1)).toBe("1.3 kW");
  });
});
