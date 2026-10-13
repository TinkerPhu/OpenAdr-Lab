/**
 * Where the sun is, for charts that want to show day and night.
 *
 * A faithful port of `solar_position` in `VEN/src/entities/solar.rs` — the
 * elevation path only, not the PV array geometry built on top of it. It is the
 * standard low-precision USNO formula (a fraction of a degree, which is far
 * more than a background wash needs) and it has **no equation-of-time term**:
 * it goes from mean longitude to ecliptic longitude to right ascension, and
 * takes the hour angle from Greenwich mean sidereal time.
 *
 * **This is a second implementation of one concept, in a second language**, and
 * `one-concept-one-function` says not to do that. It is accepted here because
 * the alternatives are worse: a clock-based approximation is wrong by hours for
 * half the year at this latitude (sunset at 47.45°N swings from ~16:40 in
 * December to ~21:30 in June), and there is no channel that could carry the
 * Rust answer to a browser — `SolarPosition` is not serialized and no endpoint
 * exposes it. Recorded in docs/reference/TECHNICAL_DEBTS.md; if the two ever
 * need to agree to better than a degree, the fix is to serve elevation from the
 * backend, not to make this smarter.
 *
 * Deliberately NOT ported: `natural_irradiance_at`, the crude fixed
 * 06:00–18:00 UTC half-sine with no latitude or date. Approximating daylight
 * that way is the thing this module exists to avoid.
 */

const DEG = Math.PI / 180;
const RAD = 180 / Math.PI;

/**
 * Julian Date, with the time of day folded into the day number.
 *
 * Every field is read with a UTC getter, and that is load-bearing: the Rust
 * reads `.hour()` on a `DateTime<Utc>`, so a local-time getter here would shift
 * every result by the browser's offset — a bug that still looks like a
 * plausible sunrise, just the wrong one.
 */
function julianDate(tsMs: number): number {
  const t = new Date(tsMs);
  const year = t.getUTCFullYear();
  const month = t.getUTCMonth() + 1;
  const day =
    t.getUTCDate() +
    t.getUTCHours() / 24 +
    t.getUTCMinutes() / 1440 +
    t.getUTCSeconds() / 86400;

  const y = month <= 2 ? year - 1 : year;
  const m = month <= 2 ? month + 12 : month;

  const a = Math.floor(y / 100);
  const b = 2 - a + Math.floor(a / 4);
  return (
    Math.floor(365.25 * (y + 4716)) + Math.floor(30.6001 * (m + 1)) + day + b - 1524.5
  );
}

/**
 * Solar elevation [degrees] at an instant and place. Negative below the
 * horizon, as the Rust's own midnight test pins.
 *
 * `l` and `lambdaSun` are deliberately left unreduced (no mod 360) exactly as
 * the original has them — trigonometry makes it harmless, and "tidying" it is
 * how a port stops being a port. `% 360` on `theta` keeps the sign of the
 * dividend in both languages, so it carries across directly.
 */
export function solarElevationDeg(
  tsMs: number,
  latitudeDeg: number,
  longitudeDeg: number,
): number {
  const n = julianDate(tsMs) - 2451545.0;
  const l = 280.46 + 0.9856474 * n;
  const g = (357.528 + 0.9856003 * n) * DEG;
  const lambdaSun = l + 1.915 * Math.sin(g) + 0.02 * Math.sin(2 * g);
  const epsilon = (23.439 - 0.0000004 * n) * DEG;
  const lambdaRad = lambdaSun * DEG;
  const latRad = latitudeDeg * DEG;

  const alpha = Math.atan2(Math.cos(epsilon) * Math.sin(lambdaRad), Math.cos(lambdaRad));
  const delta = Math.asin(Math.sin(epsilon) * Math.sin(lambdaRad));
  const theta = ((280.46061837 + 360.98564736629 * n + longitudeDeg) % 360) * DEG;
  const hAngle = theta - alpha;

  return (
    Math.asin(
      Math.sin(latRad) * Math.sin(delta) +
        Math.cos(latRad) * Math.cos(delta) * Math.cos(hAngle),
    ) * RAD
  );
}
