import { describe, it, expect } from "vitest";
import { sharedTariff, tariffRows, priceBands, IMPORT_KEY, EXPORT_KEY } from "../utils/fleetTariff";
import type { FleetSignals, SignalBand } from "../api/types";

const T0 = Date.parse("2026-09-23T09:00:00Z");
const HOUR = 3_600_000;

function band(over: Partial<SignalBand> = {}): SignalBand {
  return {
    from: new Date(T0).toISOString(),
    to: new Date(T0 + HOUR).toISOString(),
    eventID: "ev-1",
    eventName: "tou-pricing-day-ahead",
    payloadType: "PRICE",
    value: 0.4,
    ...over,
  };
}

/** The lab's real shape: hourly import + export, identical for every VEN. */
const LAB_BANDS: SignalBand[] = [
  band({ payloadType: "PRICE", value: 0.4 }),
  band({ payloadType: "EXPORT_PRICE", value: 0.3, eventID: "ev-2" }),
  band({
    from: new Date(T0 + HOUR).toISOString(),
    to: new Date(T0 + 2 * HOUR).toISOString(),
    payloadType: "PRICE",
    value: 0.2,
  }),
];

function signals(vens: { venName: string; bands: SignalBand[] }[]): FleetSignals {
  return {
    from: new Date(T0).toISOString(),
    to: new Date(T0 + 2 * HOUR).toISOString(),
    vens,
    rejectedEvents: 0,
  };
}

const fleetOf = (n: number, bands = LAB_BANDS) =>
  signals(Array.from({ length: n }, (_, i) => ({ venName: `ven-${i + 1}`, bands })));

describe("priceBands", () => {
  it("keeps only priced signals", () => {
    const mixed = [
      band({ payloadType: "IMPORT_CAPACITY_LIMIT", value: 3 }),
      band({ payloadType: "GHG", value: 300 }),
      band({ payloadType: "PRICE", value: null }),
      band({ payloadType: "PRICE", value: 0.4 }),
      band({ payloadType: "EXPORT_PRICE", value: 0.3 }),
    ];
    expect(priceBands(mixed).map((b) => b.payloadType)).toEqual(["PRICE", "EXPORT_PRICE"]);
  });
});

describe("sharedTariff", () => {
  it("folds twenty identical VENs into one tariff", () => {
    const shared = sharedTariff(fleetOf(20));
    expect(shared.agreeingVens).toBe(20);
    expect(shared.dissentingVens).toEqual([]);
    expect(shared.bands).toHaveLength(3);
  });

  it("names a VEN priced differently and keeps the majority's tariff", () => {
    const odd = LAB_BANDS.map((b) =>
      b.payloadType === "PRICE" && b.value === 0.4 ? { ...b, value: 0.99 } : b,
    );
    const shared = sharedTariff(
      signals([
        { venName: "ven-1", bands: LAB_BANDS },
        { venName: "ven-2", bands: LAB_BANDS },
        { venName: "ven-10", bands: odd },
      ]),
    );
    expect(shared.dissentingVens).toEqual(["ven-10"]);
    expect(shared.agreeingVens).toBe(2);
    expect(shared.bands.find((b) => b.payloadType === "PRICE")?.value).toBe(0.4);
  });

  it("does not call an untargeted VEN a dissenter", () => {
    const shared = sharedTariff(
      signals([
        { venName: "ven-1", bands: LAB_BANDS },
        // Sent a limit but no price — not disagreement about price.
        { venName: "ven-2", bands: [band({ payloadType: "IMPORT_CAPACITY_LIMIT", value: 3 })] },
      ]),
    );
    expect(shared.dissentingVens).toEqual([]);
    expect(shared.agreeingVens).toBe(1);
    expect(shared.knownVens).toBe(2);
  });

  it("reports dissenters in counting order, not byte order", () => {
    const other = (v: number) => LAB_BANDS.map((b) => ({ ...b, value: v }));
    const shared = sharedTariff(
      signals([
        { venName: "ven-1", bands: LAB_BANDS },
        { venName: "ven-2", bands: LAB_BANDS },
        { venName: "ven-10", bands: other(0.7) },
        { venName: "ven-3", bands: other(0.8) },
      ]),
    );
    expect(shared.dissentingVens).toEqual(["ven-3", "ven-10"]);
  });

  it("is empty when nothing was priced", () => {
    expect(sharedTariff(undefined).bands).toEqual([]);
    expect(sharedTariff(signals([])).agreeingVens).toBe(0);
  });
});

describe("tariffRows", () => {
  const tMin = T0 + HOUR / 2;
  const tMax = T0 + 2 * HOUR;

  it("starts at the price in force at the left edge, not at the next step", () => {
    const rows = tariffRows(LAB_BANDS, tMin, tMax);
    // The 09:00 band is still in force at 09:30; without the left anchor the
    // line would begin an hour into the window.
    expect(rows[0].ts).toBe(tMin);
    expect(rows[0].values?.[IMPORT_KEY]).toBe(0.4);
    expect(rows[0].values?.[EXPORT_KEY]).toBe(0.3);
  });

  it("reaches the right edge carrying the last announced price", () => {
    const rows = tariffRows(LAB_BANDS, tMin, tMax);
    const last = rows[rows.length - 1];
    expect(last.ts).toBe(tMax);
    expect(last.values?.[IMPORT_KEY]).toBe(0.2);
  });

  it("puts import and export in the same row, not two arrays", () => {
    const rows = tariffRows(LAB_BANDS, T0, tMax);
    const atStart = rows.find((r) => r.ts === T0);
    // recharts resolves tooltips by array index; two arrays would mislabel.
    expect(atStart?.values?.[IMPORT_KEY]).toBe(0.4);
    expect(atStart?.values?.[EXPORT_KEY]).toBe(0.3);
  });

  it("never leaves rows past the right edge", () => {
    const rows = tariffRows(LAB_BANDS, T0, T0 + HOUR / 2);
    expect(rows.every((r) => r.ts <= T0 + HOUR / 2)).toBe(true);
  });

  it("has nothing to draw with no bands", () => {
    expect(tariffRows([], tMin, tMax)).toEqual([]);
  });
});
