import { render, screen } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import { Sparkline } from "../components/Sparkline";
import { sparklinePath, zeroLineY } from "../utils/sparkline";

const at = (i: number, value: number) => ({
  ts: `2026-09-22T10:0${i}:00Z`,
  value,
});

describe("sparklinePath", () => {
  it("is empty for no samples", () => {
    expect(sparklinePath([], 100, 10)).toBe("");
  });

  it("spans the box from first sample to last", () => {
    const d = sparklinePath([at(0, 0), at(1, 10)], 100, 10);
    expect(d.startsWith("M0.0,")).toBe(true);
    expect(d).toContain("L100.0,");
  });

  /* A site drawing a constant 2 kW has no range of its own. Scaling against
   * it would divide by zero; the line should simply be flat. */
  it("draws a flat series without dividing by its zero range", () => {
    const d = sparklinePath([at(0, 2000), at(1, 2000), at(2, 2000)], 100, 10);
    expect(d).not.toContain("NaN");
    const ys = [...d.matchAll(/,([\d.]+)/g)].map((m) => Number(m[1]));
    expect(new Set(ys).size).toBe(1);
  });

  /* Import above the line and export below is the shape of a site's day. A
   * scale that omitted zero would hide the moment it crosses. */
  it("keeps zero inside the scale when the series crosses it", () => {
    const samples = [at(0, -1000), at(1, 1000)];
    expect(zeroLineY(samples, 10)).toBeCloseTo(5);
    const d = sparklinePath(samples, 100, 10);
    const ys = [...d.matchAll(/,([\d.]+)/g)].map((m) => Number(m[1]));
    // Export is drawn below the zero line, import above it.
    expect(ys[0]).toBeGreaterThan(5);
    expect(ys[1]).toBeLessThan(5);
  });

  it("includes zero even for an all-positive series", () => {
    expect(zeroLineY([at(0, 1000), at(1, 2000)], 10)).toBeCloseTo(10);
  });
});

describe("Sparkline", () => {
  it("says so when the window holds no samples, instead of drawing nothing", () => {
    render(<Sparkline samples={[]} label="fleet power" />);
    expect(screen.getByTestId("sparkline-empty")).toBeVisible();
  });

  it("renders a labelled chart when it has samples", () => {
    render(<Sparkline samples={[at(0, 1), at(1, 2)]} label="fleet power" />);
    expect(screen.getByLabelText("fleet power")).toBeVisible();
  });
});
