/**
 * How an asset is shown — colour, label, planning role — decided by its declared kind
 * (`asset_type`, an `AssetKind`), never by its id. The one place every chart, cell and list
 * asks; no component keeps its own id-keyed map.
 *
 * The one exception is a request-created shiftable load: its kind says only "shiftable load",
 * so a load the UI knows by name (`wm`, `dw`, offered by the Devices page) gets its display
 * name and colour from `SHIFTABLE_LOAD_DISPLAY`, keyed by the load's id.
 */
import type { AssetKind } from "../../api/generated/AssetKind";
import type { SimSnapshot } from "../../api/types";
import { assetTypeOf, assetsOfType } from "../../api/assets";

/** Fallback colour for an asset the roster does not (or no longer) holds. */
export const COLOR_ASSET_FALLBACK = "#888";

export const KIND_COLORS: Record<AssetKind, string> = {
  ev: "#2196F3",
  heater: "#FF5722",
  pv: "#FFC107",
  battery: "#9C27B0",
  base_load: "#607D8B",
  shiftable_load: "#FF9800",
};

export const KIND_LABELS: Record<AssetKind, string> = {
  ev: "EV",
  heater: "Heater",
  pv: "PV",
  battery: "Battery",
  base_load: "Base Load",
  shiftable_load: "Shiftable Load",
};

/** "forecast" = predicted, not controllable; "planned" = scheduled by the planner. */
export const KIND_PLANNING_ROLE: Record<AssetKind, "forecast" | "planned"> = {
  ev: "planned",
  heater: "planned",
  pv: "forecast",
  battery: "planned",
  base_load: "forecast",
  shiftable_load: "planned",
};

/** Shiftable loads the UI offers by name (`ShiftableLoadsCard`), by the load's id. */
export const SHIFTABLE_LOAD_DISPLAY: Record<string, { label: string; color: string }> = {
  wm: { label: "Washing Machine", color: "#FF9800" },
  dw: { label: "Dishwasher", color: "#795548" },
};

type Roster = Pick<SimSnapshot, "assets"> | undefined;

/** The asset's label: its kind's, with its id when the VEN holds more than one of that kind
 * ("EV · car-b"); a named shiftable load's name; the id of an asset the roster does not hold. */
export function assetLabel(sim: Roster, assetId: string): string {
  const kind = assetTypeOf(sim, assetId);
  if (kind === undefined || kind === "shiftable_load") {
    return SHIFTABLE_LOAD_DISPLAY[assetId]?.label ?? assetId;
  }
  return assetsOfType(sim, kind).length > 1
    ? `${KIND_LABELS[kind]} · ${assetId}`
    : KIND_LABELS[kind];
}

/** The asset's colour: its kind's; a named shiftable load's own. */
export function assetColor(sim: Roster, assetId: string): string {
  const kind = assetTypeOf(sim, assetId);
  if (kind === undefined || kind === "shiftable_load") {
    return SHIFTABLE_LOAD_DISPLAY[assetId]?.color ?? (kind ? KIND_COLORS[kind] : COLOR_ASSET_FALLBACK);
  }
  return KIND_COLORS[kind];
}

/** A legend entry: the label and the asset's planning role ("PV (forecast)"). */
export function assetLegendLabel(label: string, kind: AssetKind | undefined): string {
  return `${label} (${kind ? KIND_PLANNING_ROLE[kind] : "planned"})`;
}

/** Colour and legend label of each of `assetIds`, for a stacked chart. */
export function assetLegend(
  sim: Roster,
  assetIds: string[],
): { colorMap: Record<string, string>; labelMap: Record<string, string> } {
  return {
    colorMap: Object.fromEntries(assetIds.map((id) => [id, assetColor(sim, id)])),
    labelMap: Object.fromEntries(
      assetIds.map((id) => [id, assetLegendLabel(assetLabel(sim, id), assetTypeOf(sim, id))]),
    ),
  };
}
