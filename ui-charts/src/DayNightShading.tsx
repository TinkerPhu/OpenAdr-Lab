import { ReferenceArea } from "recharts";
import { solarElevationDeg } from "./solarPosition";

/**
 * Day/night background wash for a time-axis chart.
 *
 * A fleet of PV sites exports at midday and imports after dark, so time of day
 * is the biggest single driver of the shape on screen — but nothing on a chart
 * says which part of the window was daylight, and the reader has to reconstruct
 * it from the axis labels. This paints it.
 *
 * Returns elements directly rather than a wrapping component, for the same
 * reason `ZoneShading.tsx` and `NowLine.tsx` do: recharts inspects its direct
 * children's types to decide how to render and position them, so an
 * intermediate component would change what it sees.
 */

/** Sun this far above the horizon: full daylight, no wash at all. */
const CLEAR_ABOVE_DEG = 6;
/** Sun this far below: full night. Between the two is dawn/dusk. */
const DARK_BELOW_DEG = -12;

/**
 * Darkest the wash ever gets.
 *
 * Deliberately modest: this sits behind twenty coloured lines and a legend, and
 * its job is orientation, not atmosphere. A night band you notice while reading
 * a curve is too dark.
 */
const MAX_ALPHA = 0.16;

/** Night is blue-black rather than neutral grey — grey reads as "disabled". */
const NIGHT_RGB = "22, 32, 58";

/** Smooth 0..1 ramp, so dawn and dusk are a gradient rather than a switch. */
function smoothstep(edge0: number, edge1: number, x: number): number {
  const t = Math.min(1, Math.max(0, (x - edge0) / (edge1 - edge0)));
  return t * t * (3 - 2 * t);
}

/** How dark the wash is at one instant: 0 in daylight, MAX_ALPHA at night. */
export function nightAlphaAt(
  tsMs: number,
  latitudeDeg: number,
  longitudeDeg: number,
): number {
  const elevation = solarElevationDeg(tsMs, latitudeDeg, longitudeDeg);
  // 1 at/below DARK_BELOW_DEG, 0 at/above CLEAR_ABOVE_DEG.
  const darkness = 1 - smoothstep(DARK_BELOW_DEG, CLEAR_ABOVE_DEG, elevation);
  return darkness * MAX_ALPHA;
}

export interface DayNightSpec {
  tMin: number;
  tMax: number;
  latitudeDeg: number;
  longitudeDeg: number;
  /** Bands across the window. Fixed count, not fixed duration, so the element
   *  cost is the same for a 15-minute window and a 24-hour one. */
  steps?: number;
}

export interface DayNightBand {
  x1: number;
  x2: number;
  alpha: number;
}

/**
 * The wash as plain numbers — the whole calculation, with no React in it, so it
 * can be tested by reading values rather than by rendering a chart.
 */
export function dayNightBands({
  tMin,
  tMax,
  latitudeDeg,
  longitudeDeg,
  steps = 48,
}: DayNightSpec): DayNightBand[] {
  if (!(tMax > tMin) || steps < 1) return [];
  const width = (tMax - tMin) / steps;
  const bands: DayNightBand[] = [];
  for (let i = 0; i < steps; i++) {
    const x1 = tMin + i * width;
    const x2 = x1 + width;
    // Sampled at the midpoint: a band is a constant-colour approximation of a
    // continuously changing sun, and its middle is the honest representative.
    bands.push({ x1, x2, alpha: nightAlphaAt(x1 + width / 2, latitudeDeg, longitudeDeg) });
  }
  return bands;
}

export function renderDayNightShading(yAxisId: string, spec: DayNightSpec) {
  return dayNightBands(spec).map((band) => (
    <ReferenceArea
      key={band.x1}
      yAxisId={yAxisId}
      x1={band.x1}
      x2={band.x2}
      fill={`rgba(${NIGHT_RGB}, ${band.alpha.toFixed(4)})`}
      // Explicit: recharts defaults ReferenceArea's fillOpacity to 0.5, which
      // would silently halve every alpha computed above.
      fillOpacity={1}
      // Deliberately not reusing ZoneShading's fill, whose alpha grows with the
      // band index (0.04 * (i+1)) — fine for a handful of signal windows,
      // opaque black by band 13.
      ifOverflow="hidden"
    />
  ));
}
