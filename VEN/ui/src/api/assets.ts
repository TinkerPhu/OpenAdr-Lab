import type { AssetSnapshot, SimSnapshot } from "./types";

/**
 * The assets of one type (`asset_type`, which each asset declares on `/sim`), in roster order
 * (`roster_index`, then id): the one answer to "which EVs / heaters / batteries does this VEN
 * have" (W6). Every per-kind list in the UI calls this; no component keeps its own id list.
 */
export function assetsOfType(
  sim: SimSnapshot | undefined,
  assetType: string,
): Array<[string, AssetSnapshot]> {
  return assetsInRosterOrder(sim).filter(([, a]) => a.asset_type === assetType);
}

/** The type `assetId` declares on `/sim` (`asset_type`); its id when it is not in the snapshot
 * (an asset in the history that the roster no longer holds — on a default profile the id is
 * the type). */
export function assetTypeOf(sim: SimSnapshot | undefined, assetId: string): string {
  return sim?.assets?.[assetId]?.asset_type ?? assetId;
}

/** Every asset of the roster, in roster order (`roster_index`, then id). */
export function assetsInRosterOrder(sim: SimSnapshot | undefined): Array<[string, AssetSnapshot]> {
  const entries = Object.entries(sim?.assets ?? {}).filter(
    (e): e is [string, AssetSnapshot] => e[1] !== undefined,
  );
  return entries.sort(
    ([aId, a], [bId, b]) => a.roster_index - b.roster_index || aId.localeCompare(bId),
  );
}

/** The order power charts stack assets in, by type: generation nearest the axis, then the base
 * load, then the controllable loads; other types after, in the order given. */
const STACK_TYPE_ORDER = ["pv", "base_load", "ev", "shiftable_load", "heater", "battery"];

/** `assetIds` in stacking order: by type (`typeOf`), the given order kept within a type — the one
 * rule every stacked power chart uses (W6), so no chart decides by an asset's id. */
export function stackOrder(assetIds: string[], typeOf: (assetId: string) => string): string[] {
  const rank = (id: string) => {
    const i = STACK_TYPE_ORDER.indexOf(typeOf(id));
    return i < 0 ? STACK_TYPE_ORDER.length : i;
  };
  return assetIds
    .map((id, i) => ({ id, i }))
    .sort((a, b) => rank(a.id) - rank(b.id) || a.i - b.i)
    .map(({ id }) => id);
}
