//! Turning the per-asset timeline arrays into stacked-area chart data.
//!
//! These two functions lived in `GridAccumulatedCell.tsx`, so
//! `PlanPowerStack.tsx` imported its data builders out of another
//! component's file — and the pair tripped `react-refresh`'s "a file should
//! export only components" rule, which is the lint noticing the same thing.

import type { AssetId, AssetTimelinePoint, StackedAreaPoint } from "./types";

/** Discover all asset IDs present in the timelines (everything except "grid"). */
function discoverAssetIds(allTimelines: Record<string, AssetTimelinePoint[]>): AssetId[] {
  return Object.keys(allTimelines).filter((id) => id !== "grid");
}

/**
 * Narrows a candidate asset-ID list (e.g. the currently-configured roster from
 * assetSummaries) down to the ones that actually have timeline entries — i.e. the
 * same set StackedTimeSeriesChart will actually draw a graph for. A candidate asset
 * with no timeline data yet would otherwise still get a legend entry with nothing
 * plotted next to it; this is the single filter both StackedTimeSeriesChart callers
 * (GridAccumulatedCell, PlanPowerStack) apply before passing `assetIds` to the chart,
 * so a legend entry and its graph can never drift apart.
 */
export function assetIdsWithTimelineData(
  candidateIds: AssetId[],
  allTimelines: Record<string, AssetTimelinePoint[]>
): AssetId[] {
  return candidateIds.filter((id) => (allTimelines[id]?.length ?? 0) > 0);
}

/** Build stacked-area data by positional zip across grid-aligned asset arrays. */
export function buildStackedFromAllTimelines(
  allTimelines: Record<string, AssetTimelinePoint[]>
): StackedAreaPoint[] {
  const assetIds = discoverAssetIds(allTimelines);
  // Use the first non-empty asset's array to determine length and timestamps.
  // RF-05c guarantees all assets share the same ts at each index.
  const refAsset = assetIds.find((id) => (allTimelines[id]?.length ?? 0) > 0);
  const refPoints = refAsset ? allTimelines[refAsset] : [];
  if (!refPoints || refPoints.length === 0) return [];

  return refPoints.map((ref, i) => {
    const pt: StackedAreaPoint = {
      ts: ref.ts,
      ev_pos: 0, ev_neg: 0,
      heater_pos: 0, heater_neg: 0,
      pv_pos: 0, pv_neg: 0,
      battery_pos: 0, battery_neg: 0,
      base_load_pos: 0, base_load_neg: 0,
      gridPowerKw: null,
    };
    for (const assetId of assetIds) {
      const kw = allTimelines[assetId]?.[i]?.values?.["power_kw"] ?? 0;
      pt[`${assetId}_pos`] = Math.max(0, kw);
      pt[`${assetId}_neg`] = Math.min(0, kw);
    }
    pt.gridPowerKw = allTimelines["grid"]?.[i]?.values?.["power_kw"] ?? null;
    return pt;
  });
}
