import { ReferenceLine } from "recharts";
import { COLOR_NOW } from "../controller/types";

/**
 * A labelled vertical time marker ("NOW", a commitment start, ...), shared by every
 * time-series chart. Returns the element directly (not a wrapping component) so it can be
 * spliced as a normal `<ReferenceLine>` child of `<ComposedChart>` — recharts inspects its
 * direct children's types to compute axis domains and positioning; wrapping this in an
 * intermediate component would change what type recharts sees at that position in the tree.
 *
 * The label sits INSIDE the plot: recharts draws a `position: "top"` label *above* the plot
 * area, where the chart's own SVG clips it away — measured on the live page, a "top" label's
 * box sat 9 px above the surface, leaving a sliver of colour at the upper edge and no readable
 * text (which is how this was found: a user saw the commitment-start marker as a "nearly
 * invisible blue gadget" at the top edge).
 *
 * `row` stacks labels: two markers at nearly the same time (a commitment starting in the
 * first plan slot sits right next to NOW) would otherwise print on top of each other.
 */
export function renderTimeMarkerLine(
  yAxisId: string,
  tsMs: number,
  { label, color, row = 0 }: { label: string; color: string; row?: number }
) {
  return (
    <ReferenceLine
      key={`time-marker-${label}`}
      yAxisId={yAxisId}
      x={tsMs}
      stroke={color}
      strokeDasharray="3 3"
      label={{
        value: label,
        position: "insideTop",
        dy: row * LABEL_ROW_HEIGHT_PX,
        fontSize: 9,
        fill: color,
      }}
    />
  );
}

const LABEL_ROW_HEIGHT_PX = 10;

/** The "NOW" marker every time-series chart draws. */
export function renderNowLine(yAxisId: string, nowMs: number) {
  return renderTimeMarkerLine(yAxisId, nowMs, { label: "NOW", color: COLOR_NOW });
}
