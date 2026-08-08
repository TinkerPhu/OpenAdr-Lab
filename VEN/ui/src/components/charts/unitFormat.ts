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

/** Cost rate [€/h] — 4 decimal places. */
export function formatCostRateEurH(value: number): string {
  return `${value.toFixed(4)} €/h`;
}

/** CO2 rate [g/h] — 1 decimal place. */
export function formatCo2RateGH(value: number): string {
  return `${value.toFixed(1)} g/h`;
}

/** CO2 intensity [g/kWh] — a distinct physical quantity from CO2 rate (per-energy
 * intensity, not a per-time rate); 3 decimal places. */
export function formatCo2IntensityGKwh(value: number): string {
  return `${value.toFixed(3)} g/kWh`;
}

/** Tariff [€/kWh] — 4 decimal places. */
export function formatTariffEurKwh(value: number): string {
  return `${value.toFixed(4)} €/kWh`;
}

/** State of charge — input is a 0–1 fraction; displayed as a percentage, 1 decimal place. */
export function formatSocPct(fraction: number): string {
  return `${(fraction * 100).toFixed(1)} %`;
}

/** Temperature [°C] — 1 decimal place. */
export function formatTemperatureC(value: number): string {
  return `${value.toFixed(1)} °C`;
}
