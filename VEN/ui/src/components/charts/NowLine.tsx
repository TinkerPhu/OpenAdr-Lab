import { ReferenceLine } from "recharts";
import { COLOR_NOW } from "../controller/types";

/**
 * The "NOW" vertical reference line shared by every time-series chart. Returns the
 * element directly (not a wrapping component) so it can be spliced as a normal
 * `<ReferenceLine>` child of `<ComposedChart>` — recharts inspects its direct children's
 * types to compute axis domains and positioning; wrapping this in an intermediate
 * component would change what type recharts sees at that position in the tree.
 */
export function renderNowLine(yAxisId: string, nowMs: number) {
  return (
    <ReferenceLine
      yAxisId={yAxisId}
      x={nowMs}
      stroke={COLOR_NOW}
      strokeDasharray="3 3"
      label={{ value: "NOW", position: "top", fontSize: 9, fill: COLOR_NOW }}
    />
  );
}
