/**
 * Canonical per-unit value formatting, shared by every chart's tooltip (and, for power,
 * the axis tick too via `formatPowerTick`). One rule per unit, defined once — before this
 * module existed, the same physical unit (e.g. CO2 rate) was formatted with a different
 * decimal-precision rule depending which chart file happened to render it.
 */
import { formatPowerTick } from "./axisDomain";

/** Power [kW] — magnitude-aware: Watts (integer) below 1 kW, kW (2dp) at/above. Delegates to
 * the axis-tick formatter so tooltip and axis tick can never disagree with each other. */
export function formatPowerValue(valueKw: number): string {
  return formatPowerTick(valueKw);
}

/** Explicit-sign power display (e.g. "+1.00 kW", "-4.20 kW") for net/bidirectional power
 * values (import vs. export, charge vs. discharge). Signs from `valueKw` itself, not from
 * the formatted string: `formatPowerValue` rounds sub-watt magnitudes to whole Watts
 * (`Math.round(valueKw*1000)`), which would otherwise silently collapse a tiny negative
 * residual (e.g. -0.0002 kW) to a sign-losing "-0" -> "0" in a template string. */
export function formatSignedPowerValue(valueKw: number): string {
  const magnitude = formatPowerValue(Math.abs(valueKw));
  if (valueKw > 0) return `+${magnitude}`;
  if (valueKw < 0) return `-${magnitude}`;
  return magnitude;
}

/** Cost rate [€/h] — 4 decimal places. */
export function formatCostRateEurH(valueEurH: number): string {
  return `${valueEurH.toFixed(4)} €/h`;
}

/** CO2 rate [g/h] — 1 decimal place. */
export function formatCo2RateGH(valueGH: number): string {
  return `${valueGH.toFixed(1)} g/h`;
}

/** CO2 intensity [g/kWh] — a distinct physical quantity from CO2 rate (per-energy
 * intensity, not a per-time rate); 3 decimal places. */
export function formatCo2IntensityGKwh(valueGKwh: number): string {
  return `${valueGKwh.toFixed(3)} g/kWh`;
}

/** Tariff [€/kWh] — 4 decimal places. */
export function formatTariffEurKwh(valueEurKwh: number): string {
  return `${valueEurKwh.toFixed(4)} €/kWh`;
}

/** State of charge — input is a 0–1 fraction; displayed as a percentage, 1 decimal place. */
export function formatSocPct(socFraction: number): string {
  return `${(socFraction * 100).toFixed(1)} %`;
}

/** Temperature [°C] — 1 decimal place. */
export function formatTemperatureC(valueC: number): string {
  return `${valueC.toFixed(1)} °C`;
}

/** Energy [kWh] — 1 decimal place. */
export function formatEnergyKwh(valueKwh: number): string {
  return `${valueKwh.toFixed(1)} kWh`;
}
