/**
 * A small inline chart, drawn as an SVG path.
 *
 * Deliberately not a charting library: the fleet's phase-0 question is "is it
 * going up or down, and did it move when the event landed", which a polyline
 * answers. A dependency is worth adding when the questions need axes, zoom and
 * tooltips — the phase-1 fleet views — not before.
 */

export type Sample = { ts: string; value: number };

/**
 * The polyline for these samples inside a `width` x `height` box.
 *
 * Exported separately from the component because the projection is the part
 * that can be wrong in a way rendering cannot show: a flat series must not
 * divide by a zero range, and a series crossing zero must keep its sign.
 */
export function sparklinePath(samples: Sample[], width: number, height: number): string {
  if (samples.length === 0) return "";

  const values = samples.map((s) => s.value);
  let min = Math.min(...values);
  let max = Math.max(...values);
  if (min === max) {
    // A flat series has no range to scale against. Give it one so it draws as
    // a line through the middle rather than dividing by zero.
    min -= 1;
    max += 1;
  }
  // Zero belongs on the chart whenever the series crosses it: export below the
  // line and import above is the whole shape of a site's day, and a scale that
  // omitted zero would hide the crossing.
  if (min > 0) min = 0;
  if (max < 0) max = 0;

  const x = (i: number) =>
    samples.length === 1 ? width / 2 : (i / (samples.length - 1)) * width;
  const y = (v: number) => height - ((v - min) / (max - min)) * height;

  return samples.map((s, i) => `${i === 0 ? "M" : "L"}${x(i).toFixed(1)},${y(s.value).toFixed(1)}`).join(" ");
}

/** Where zero sits in the box, or `null` when the series never reaches it. */
export function zeroLineY(samples: Sample[], height: number): number | null {
  if (samples.length === 0) return null;
  const values = samples.map((s) => s.value);
  const min = Math.min(0, ...values);
  const max = Math.max(0, ...values);
  if (min === max) return null;
  return height - ((0 - min) / (max - min)) * height;
}
