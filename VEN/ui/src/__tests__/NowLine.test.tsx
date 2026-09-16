/**
 * Time-marker lines are text-free: colour and position on the time axis identify them. A
 * label would also be clipped — recharts draws `position: "top"` above the plot area, where
 * the chart's SVG cuts it off (measured on the live page: 9 px above the surface).
 */
import { describe, it, expect } from "vitest";
import { renderNowLine, renderTimeMarkerLine } from "../components/charts/NowLine";

type MarkerProps = { x: number; yAxisId: string; stroke: string; label?: unknown };
const propsOf = (el: ReturnType<typeof renderNowLine>) => el.props as MarkerProps;

describe("time marker lines", () => {
  it("carries no label", () => {
    expect(propsOf(renderNowLine("power", 1_000)).label).toBeUndefined();
    expect(propsOf(renderTimeMarkerLine("power", 2_000, "#6A1B9A")).label).toBeUndefined();
  });

  it("puts the marker on the axis, time and colour it was given", () => {
    const props = propsOf(renderTimeMarkerLine("power", 2_000, "#6A1B9A"));
    expect(props.x).toBe(2_000);
    expect(props.yAxisId).toBe("power");
    expect(props.stroke).toBe("#6A1B9A");
  });
});
