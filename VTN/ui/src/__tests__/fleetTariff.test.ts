import { describe, it, expect } from "vitest";
import {
  tariffGroups,
  shortNames,
  tariffRows,
  tariffLineRows,
  priceBands,
  IMPORT_KEY,
  EXPORT_KEY,
  DRAFT_IMPORT_KEY,
  DRAFT_EXPORT_KEY,
} from "../utils/fleetTariff";
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

const NOW = T0 + HOUR / 2;

/** A tariff of one PRICE and one EXPORT_PRICE band, carried by event `name`. */
function tariffOf(name: string | null, price: number, eventID = name ?? "ev"): SignalBand[] {
  return [
    band({ payloadType: "PRICE", value: price, eventName: name, eventID }),
    band({ payloadType: "EXPORT_PRICE", value: price * 0.75, eventName: name, eventID }),
  ];
}

describe("tariffGroups", () => {
  it("folds twenty identical VENs into one tariff", () => {
    const { groups, knownVens, unpricedVens } = tariffGroups(fleetOf(20), NOW);
    expect(groups).toHaveLength(1);
    expect(groups[0].vens).toHaveLength(20);
    expect(groups[0].bands).toHaveLength(3);
    expect(knownVens).toBe(20);
    expect(unpricedVens).toEqual([]);
  });

  it("keeps every distinct tariff, largest first, and none is a dissenter", () => {
    const fleet = signals([
      { venName: "ven-1", bands: tariffOf("exp-PRICE-g0", 0.1) },
      { venName: "ven-2", bands: tariffOf("exp-PRICE-g1", 0.2) },
      { venName: "ven-3", bands: tariffOf("exp-PRICE-g1", 0.2) },
      { venName: "ven-10", bands: tariffOf("exp-PRICE-g1", 0.2) },
      { venName: "ven-4", bands: tariffOf("exp-PRICE-g2", 0.3) },
    ]);
    const { groups } = tariffGroups(fleet, NOW);
    expect(groups.map((g) => g.vens)).toEqual([
      ["ven-2", "ven-3", "ven-10"],
      ["ven-1"],
      ["ven-4"],
    ]);
  });

  it("is one tariff whichever event ids carry it", () => {
    const { groups } = tariffGroups(
      signals([
        { venName: "ven-1", bands: tariffOf("a", 0.1, "id-a") },
        { venName: "ven-2", bands: tariffOf("a", 0.1, "id-b") },
      ]),
      NOW,
    );
    expect(groups).toHaveLength(1);
  });

  it("counts a VEN with no price as unpriced, not as a tariff", () => {
    const { groups, unpricedVens, knownVens } = tariffGroups(
      signals([
        { venName: "ven-1", bands: LAB_BANDS },
        { venName: "ven-2", bands: [band({ payloadType: "IMPORT_CAPACITY_LIMIT", value: 3 })] },
      ]),
      NOW,
    );
    expect(groups).toHaveLength(1);
    expect(unpricedVens).toEqual(["ven-2"]);
    expect(knownVens).toBe(2);
  });

  it("labels each group by the short event name and its VEN count, with the VENs on hover", () => {
    const { groups } = tariffGroups(
      signals([
        { venName: "ven-1", bands: tariffOf("exp-flat-PRICE-g0", 0.1) },
        { venName: "ven-2", bands: tariffOf("exp-flat-PRICE-g1", 0.2) },
        { venName: "ven-3", bands: tariffOf("exp-flat-PRICE-g1", 0.2) },
      ]),
      NOW,
    );
    expect(groups.map((g) => g.label)).toEqual(["g1 · 2 VENs", "g0 · 1 VEN"]);
    expect(groups[0].hover).toBe("exp-flat-PRICE-g1: ven-2, ven-3");
  });

  it("labels a group with no event name by its VEN count alone", () => {
    const { groups } = tariffGroups(signals([{ venName: "ven-1", bands: tariffOf(null, 0.1) }]), NOW);
    expect(groups[0].label).toBe("tariff · 1 VEN");
  });

  it("keeps labels unique when two tariffs would read alike", () => {
    const { groups } = tariffGroups(
      signals([
        { venName: "ven-1", bands: tariffOf("same", 0.1) },
        { venName: "ven-2", bands: tariffOf("same", 0.2) },
      ]),
      NOW,
    );
    expect(new Set(groups.map((g) => g.label)).size).toBe(2);
  });

  it("is empty when nothing was priced", () => {
    expect(tariffGroups(undefined, NOW).groups).toEqual([]);
    expect(tariffGroups(signals([]), NOW).knownVens).toBe(0);
  });
});

describe("shortNames", () => {
  it("leaves out the part every name shares, cut at a separator", () => {
    expect(shortNames(["exp-flat-PRICE-g0", "exp-flat-PRICE-g1"])).toEqual(["g0", "g1"]);
  });

  it("keeps a single name, or names that are all alike, whole", () => {
    expect(shortNames(["exp-flat-PRICE-g0"])).toEqual(["exp-flat-PRICE-g0"]);
    expect(shortNames(["same", "same"])).toEqual(["same", "same"]);
  });

  it("does not cut inside a word", () => {
    expect(shortNames(["tariff-night", "tariff-nightly"])).toEqual(["night", "nightly"]);
  });

  it("passes a missing name through", () => {
    expect(shortNames(["a-x", null, "a-y"])).toEqual(["x", null, "y"]);
  });
});

describe("tariffLineRows", () => {
  it("carries any number of lines in one row array, each pinned at the left edge", () => {
    const rows = tariffLineRows(
      [
        { key: "g0", bands: [band({ value: 0.1 })] },
        { key: "g1", bands: [band({ value: 0.2 })] },
      ],
      T0 + HOUR / 2,
      T0 + 2 * HOUR,
    );
    expect(rows[0].values?.g0).toBe(0.1);
    expect(rows[0].values?.g1).toBe(0.2);
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

  it("carries a draft beside the live tariff in the same rows, under its own keys", () => {
    const draft = [
      band({ payloadType: "PRICE", value: 0.55, eventID: "draft" }),
      band({
        from: new Date(T0 + HOUR).toISOString(),
        to: new Date(T0 + 2 * HOUR).toISOString(),
        payloadType: "PRICE",
        value: 0.05,
        eventID: "draft",
      }),
    ];
    const rows = tariffRows(LAB_BANDS, T0, tMax, draft);
    const atStart = rows.find((r) => r.ts === T0);
    expect(atStart?.values?.[IMPORT_KEY]).toBe(0.4);
    expect(atStart?.values?.[DRAFT_IMPORT_KEY]).toBe(0.55);
    const atNext = rows.find((r) => r.ts === T0 + HOUR);
    expect(atNext?.values?.[DRAFT_IMPORT_KEY]).toBe(0.05);
    // No draft export was given, so there is nothing under that key.
    expect(atStart?.values?.[DRAFT_EXPORT_KEY]).toBeUndefined();
  });

  it("draws a draft even when there is no live tariff", () => {
    const rows = tariffRows([], T0, tMax, [band({ value: 0.3 })]);
    expect(rows.length).toBeGreaterThan(0);
    expect(rows[0].values?.[DRAFT_IMPORT_KEY]).toBe(0.3);
    expect(rows[0].values?.[IMPORT_KEY]).toBeUndefined();
  });
});
