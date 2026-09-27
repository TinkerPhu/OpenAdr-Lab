import { formatSignedPowerValue } from "@lab/charts/unitFormat";

/**
 * Tooltip text for an export-capability value, shared by SiteHeadroomChart and
 * CapacityForecastChart so the two never explain the same number differently.
 *
 * The curve is signed NET GRID POWER while the site exports as hard as it can,
 * so it legitimately goes positive whenever the draw it cannot switch off
 * exceeds what it can generate — base load, or a heater whose thermostat is
 * forcing it on. ven-3 shows both: no battery and no sun at night leaves the
 * line sitting at its base load (+0.68 kW), spiking to +6.68 kW for the ~8
 * minutes a forced reheat runs.
 *
 * A bare "+6.68 kW" under a legend that says "Export" reads as a contradiction,
 * which is exactly how this was first reported. The number is right; it just has
 * to say what it means.
 */
export function formatExportCapabilityValue(valueKw: number): string {
  const signed = formatSignedPowerValue(valueKw);
  return valueKw > 0 ? `${signed} (net import — nothing left to export)` : signed;
}
