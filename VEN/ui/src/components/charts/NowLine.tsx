import { ReferenceLine } from "recharts";
import { COLOR_NOW } from "../controller/types";

/**
 * A vertical time marker (now, a commitment start, ...), shared by every time-series chart.
 * Returns the element directly (not a wrapping component) so it can be spliced as a normal
 * `<ReferenceLine>` child of `<ComposedChart>` — recharts inspects its direct children's
 * types to compute axis domains and positioning; wrapping this in an intermediate component
 * would change what type recharts sees at that position in the tree.
 *
 * Deliberately text-free: a marker's colour and its position on the time axis say what it is,
 * while a label crowds the top of the plot — and recharts draws a `position: "top"` label
 * *above* the plot area, where the chart's own SVG clips it to a sliver of colour anyway.
 */
export function renderTimeMarkerLine(yAxisId: string, tsMs: number, color: string) {
  return (
    <ReferenceLine
      key={`time-marker-${color}`}
      yAxisId={yAxisId}
      x={tsMs}
      stroke={color}
      strokeDasharray="3 3"
    />
  );
}

/** The "now" marker every time-series chart draws. */
export function renderNowLine(yAxisId: string, nowMs: number) {
  return renderTimeMarkerLine(yAxisId, nowMs, COLOR_NOW);
}
