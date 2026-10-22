import type { FleetServiceReport } from "../api/types";

/** The windows are cut on this grid so a card does not ask for new ones every second. */
const STEP_MS = 5 * 60_000;

export type ServiceWindow = { from: string; to: string };

/**
 * The two windows an objective switch is judged over: the span since the switch and the equal span
 * before it, so a 16 h run is read against the preceding 16 h. `undefined` until the switch is one
 * grid step old, when there is nothing to compare yet.
 */
export function serviceWindows(
  changedAt: string,
  nowMs: number,
): { before: ServiceWindow; after: ServiceWindow } | undefined {
  const changedMs = Date.parse(changedAt);
  const toMs = Math.floor(nowMs / STEP_MS) * STEP_MS;
  const lengthMs = toMs - changedMs;
  if (!(lengthMs >= STEP_MS)) return undefined;
  const iso = (ms: number) => new Date(ms).toISOString();
  return {
    before: { from: iso(changedMs - lengthMs), to: iso(changedMs) },
    after: { from: iso(changedMs), to: iso(toMs) },
  };
}

export type BeforeAfter = { before: number | null; after: number | null };
export type AssetServiceRow = {
  venName: string;
  assetId: string;
  unserved: BeforeAfter;
  carryOver: BeforeAfter;
};

/** Each VEN's assets of the two reports lined up, an asset on only one side kept with a null on the other. */
export function joinAssets(before: FleetServiceReport, after: FleetServiceReport): AssetServiceRow[] {
  const rows = new Map<string, AssetServiceRow>();
  const put = (side: "before" | "after", report: FleetServiceReport) => {
    for (const ven of report.vens) {
      for (const a of ven.assets) {
        const key = `${ven.venName}\u0000${a.assetId}`;
        const row = rows.get(key) ?? {
          venName: ven.venName,
          assetId: a.assetId,
          unserved: { before: null, after: null },
          carryOver: { before: null, after: null },
        };
        row.unserved[side] = a.unservedEnergyKwh;
        row.carryOver[side] = a.carryOverKwh;
        rows.set(key, row);
      }
    }
  };
  put("before", before);
  put("after", after);
  return [...rows.values()].sort(
    (a, b) => a.venName.localeCompare(b.venName) || a.assetId.localeCompare(b.assetId),
  );
}
