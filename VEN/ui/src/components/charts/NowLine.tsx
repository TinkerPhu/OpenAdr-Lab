import { ReferenceLine } from "recharts";
import { COLOR_NOW } from "../controller/types";

/**
 * A vertical time marker (now, a commitment start, ...), shared by every time-series chart.
 * Returns the element directly (not a wrapping component) so it can be spliced as a normal
 * `<ReferenceLine>` child of `<ComposedChart>` — recharts inspects its direct children's
 * types to compute axis domains and positioning; wrapping this in an intermediate component
 * would change what type recharts sees at that position in the tree.
 *
 * A `label` is optional and drawn INSIDE the plot: recharts draws a `position: "top"` label
 * *above* the plot area, where the chart's own SVG clips it away — measured on the live page,
 * a "top" label's box sat 9 px above the surface, leaving a sliver of colour at the upper edge
 * and no readable text.
 */
export function renderTimeMarkerLine(
  yAxisId: string,
  tsMs: number,
  { label, color }: { label?: string; color: string }
) {
  return (
    <ReferenceLine
      key={`time-marker-${label ?? tsMs}`}
      yAxisId={yAxisId}
      x={tsMs}
      stroke={color}
      strokeDasharray="3 3"
      label={label ? { value: label, position: "insideTop", fontSize: 9, fill: color } : undefined}
    />
  );
}

/** The "now" marker every time-series chart draws — the line alone: its position against the
 * time axis says what it is, and the text only crowded the top of the plot. */
export function renderNowLine(yAxisId: string, nowMs: number) {
  return renderTimeMarkerLine(yAxisId, nowMs, { color: COLOR_NOW });
}
