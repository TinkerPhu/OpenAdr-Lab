/**
 * A small inline chart, drawn as an SVG path.
 *
 * Deliberately not a charting library: the fleet's phase-0 question is "is it
 * going up or down, and did it move when the event landed", which a polyline
 * answers. A dependency is worth adding when the questions need axes, zoom and
 * tooltips — the phase-1 fleet views — not before.
 *
 * The projection lives in `utils/sparkline` so it can be tested as arithmetic
 * rather than through a render.
 */
import type { Sample } from "../utils/sparkline";
import { sparklinePath, zeroLineY } from "../utils/sparkline";

export function Sparkline({
  samples,
  width = 320,
  height = 64,
  label,
}: {
  samples: Sample[];
  width?: number;
  height?: number;
  label: string;
}) {
  const path = sparklinePath(samples, width, height);
  const zero = zeroLineY(samples, height);

  if (samples.length === 0) {
    return (
      <span data-testid="sparkline-empty">no samples in this window</span>
    );
  }

  return (
    <svg
      width={width}
      height={height}
      viewBox={`0 0 ${width} ${height}`}
      role="img"
      aria-label={label}
      data-testid="sparkline"
    >
      {zero !== null && (
        <line
          x1={0}
          x2={width}
          y1={zero}
          y2={zero}
          stroke="currentColor"
          strokeOpacity={0.25}
          strokeDasharray="2 3"
        />
      )}
      <path d={path} fill="none" stroke="currentColor" strokeWidth={1.5} />
    </svg>
  );
}
