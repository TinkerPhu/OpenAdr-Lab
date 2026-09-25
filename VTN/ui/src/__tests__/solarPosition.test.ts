/**
 * The port is checked against physics, not against itself.
 *
 * These are the instants `VEN/src/entities/solar.rs`'s own tests use, with the
 * same closed-form references: at solar noon the sun's elevation is
 * `90 - latitude ± declination`, and declination at the solstices is ±23.44°.
 * That makes the expectations independent of both implementations, which a
 * golden value copied from one of them would not be.
 */
import { describe, it, expect } from "vitest";
import { solarElevationDeg } from "@lab/charts/solarPosition";

// Zunzgen — the lab, and the site solar.rs is calibrated against.
const LAT = 47.4491;
const LON = 7.8081;

const at = (iso: string) => solarElevationDeg(Date.parse(iso), LAT, LON);

describe("solarElevationDeg", () => {
  it("puts the summer solstice sun where the geometry says", () => {
    // 90 - 47.4491 + 23.44 ≈ 66°, and 11:41 UTC is solar noon at this longitude.
    //
    // This assertion is also the UTC guard: on a UTC+2 machine a port using
    // local-time getters would compute 13:41 instead, ~6° lower, and fail here.
    // That is the most likely porting bug and this is what catches it.
    expect(at("2026-06-21T11:41:00Z")).toBeCloseTo(66, 0);
  });

  it("puts the winter solstice sun where the geometry says", () => {
    // 90 - 47.4491 - 23.44 ≈ 19°.
    expect(at("2026-12-21T11:41:00Z")).toBeCloseTo(19, 0);
  });

  it("puts the sun below the horizon at night", () => {
    expect(at("2026-06-21T23:00:00Z")).toBeLessThan(0);
  });

  it("peaks near local solar noon, not at midnight or an arbitrary hour", () => {
    const day = Date.parse("2026-09-24T00:00:00Z");
    let bestMs = day;
    let best = -Infinity;
    for (let m = 0; m < 24 * 60; m += 5) {
      const ts = day + m * 60_000;
      const e = solarElevationDeg(ts, LAT, LON);
      if (e > best) {
        best = e;
        bestMs = ts;
      }
    }
    // Solar noon at 7.8°E is ~11:29 UTC; allow half an hour either side.
    const peakHourUtc = new Date(bestMs).getUTCHours() + new Date(bestMs).getUTCMinutes() / 60;
    expect(peakHourUtc).toBeGreaterThan(11);
    expect(peakHourUtc).toBeLessThan(12);
  });

  it("is seasonal, which a clock-based approximation would not be", () => {
    // Same wall-clock instant, six months apart: the whole reason this is not
    // a fixed 06:00-18:00 curve.
    const summer = at("2026-06-21T17:00:00Z");
    const winter = at("2026-12-21T17:00:00Z");
    expect(summer).toBeGreaterThan(0);
    expect(winter).toBeLessThan(0);
  });

  it("is lower toward the pole at the same instant", () => {
    const here = at("2026-06-21T11:41:00Z");
    const north = solarElevationDeg(Date.parse("2026-06-21T11:41:00Z"), 67, LON);
    expect(north).toBeLessThan(here);
  });
});
