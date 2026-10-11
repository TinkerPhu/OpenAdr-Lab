import type { AssetSnapshot } from "./types";

/**
 * One of an asset's own state values by key (`soc`, `temp_c`, `max_charge_kw`, ...): the number
 * when the asset reports it, `undefined` otherwise.
 *
 * On the wire these sit flattened beside `AssetSnapshot`'s typed fields (`AssetSnapshot.values`
 * in Rust). The generated type leaves them out — an index signature next to `asset_type: string`
 * would make the typed fields unusable — so this is the one place the UI reads them (R-133).
 */
export function assetValue(snap: AssetSnapshot | undefined, key: string): number | undefined {
  const value = (snap as Record<string, unknown> | undefined)?.[key];
  return typeof value === "number" ? value : undefined;
}
