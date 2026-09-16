/**
 * Time-marker lines. A `position: "top"` label is drawn *above* the plot area, where the
 * chart's SVG clips it — measured on the live page, the commitment-start label's box sat 9 px
 * above the surface and showed only as a sliver of colour at the upper edge. These pin the
 * label inside the plot, and the row stacking that keeps two markers at nearly the same time
 * readable.
 */
import { describe, it, expect } from "vitest";
import { renderNowLine, renderTimeMarkerLine } from "../components/charts/NowLine";

type MarkerLabel = { value: string; position: string; dy: number; fill: string };
const labelOf = (el: ReturnType<typeof renderNowLine>) =>
  (el.props as { label: MarkerLabel }).label;

describe("time marker lines", () => {
  it("draws the NOW label inside the plot, not above its top edge", () => {
    const label = labelOf(renderNowLine("power", 1_000));
    expect(label.value).toBe("NOW");
    expect(label.position).toBe("insideTop");
    expect(label.dy).toBe(0);
  });

  it("stacks a second marker's label below the first", () => {
    const label = labelOf(
      renderTimeMarkerLine("power", 2_000, { label: "START", color: "#6A1B9A", row: 1 })
    );
    expect(label.position).toBe("insideTop");
    expect(label.dy).toBeGreaterThan(0);
    expect(label.fill).toBe("#6A1B9A");
  });

  it("puts the marker on the axis and time it was given", () => {
    const el = renderTimeMarkerLine("power", 2_000, { label: "START", color: "#6A1B9A" });
    const props = el.props as { x: number; yAxisId: string; stroke: string };
    expect(props.x).toBe(2_000);
    expect(props.yAxisId).toBe("power");
    expect(props.stroke).toBe("#6A1B9A");
  });
});
