/**
 * Time-marker lines. A `position: "top"` label is drawn *above* the plot area, where the
 * chart's SVG clips it — measured on the live page, a label's box sat 9 px above the surface
 * and showed only as a sliver of colour at the upper edge.
 */
import { describe, it, expect } from "vitest";
import { renderNowLine, renderTimeMarkerLine } from "../components/charts/NowLine";

type MarkerProps = {
  x: number;
  yAxisId: string;
  stroke: string;
  label?: { value: string; position: string; fill: string };
};
const propsOf = (el: ReturnType<typeof renderNowLine>) => el.props as MarkerProps;

describe("time marker lines", () => {
  it("draws the now marker as a bare line — its place on the time axis says what it is", () => {
    const props = propsOf(renderNowLine("power", 1_000));
    expect(props.label).toBeUndefined();
    expect(props.x).toBe(1_000);
  });

  it("draws a label, when given one, inside the plot rather than above its top edge", () => {
    const { label } = propsOf(
      renderTimeMarkerLine("power", 2_000, { label: "START", color: "#6A1B9A" })
    );
    expect(label?.value).toBe("START");
    expect(label?.position).toBe("insideTop");
    expect(label?.fill).toBe("#6A1B9A");
  });

  it("puts the marker on the axis and time it was given", () => {
    const props = propsOf(renderTimeMarkerLine("power", 2_000, { label: "START", color: "#6A1B9A" }));
    expect(props.x).toBe(2_000);
    expect(props.yAxisId).toBe("power");
    expect(props.stroke).toBe("#6A1B9A");
  });
});
