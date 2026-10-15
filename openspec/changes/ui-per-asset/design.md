# Design

## Inventory: where the UI picks an asset by a fixed id

| Place | Today | After |
|---|---|---|
| Devices page | `EV_ASSET_ID = "ev"`, heater card for `"heater"`, `useAssetCapabilities(["heater", "ev"])` | one card per asset of type `ev` / `heater` (`assetsOfType`), hooks per asset id |
| EV / heater card | `asset_id: "ev"` / `"heater"` in the request body; requests filtered by session type | the card's `assetId` prop; requests filtered by `asset_id` |
| Controller summaries | `dataBuilders.ts`: `sim.assets["ev"|"heater"|"pv"|"battery"|"base_load"]`, `HARDCODED_IDS` | one summary per asset, built by type from its own snapshot |
| Controller right section | `AssetRightSection.tsx`: values from `sim.assets["pv"|"base_load"|"heater"|"ev"]` | the section's own asset snapshot |
| Tariff builders | `ASSET_IDS` constant | the roster's ids |
| Comfort curves | fixed `{ev, heater, battery}` list, default `"ev"` | assets of those types in roster order |
| Plan stack / stacked chart | `RENDER_ORDER` by id, `"pv"` first | order by asset type (type precedence table), then roster |
| Dashboard | `"pv" in sim.assets` | any asset of type `pv` |
| Controller chart spec | `key: "ev"` | per type |

Reused: `asset_type` on `/sim` (each asset's own declaration); `roster_index` (W5a, now on the
wire); the existing per-asset hooks (`useEvSettings(assetId)`, `useEvUsageSim(assetId)`,
`useAssetCapabilities(ids)`).

## Decisions (user, 2026-10-15)

- **U1** One card / cell / chart per asset, roster order.
- **U2** Titled with the asset id; no display-name field.
- **U3** A card submits for its own asset. Requests are only created from cards, so the card is
  the picker; no separate selector is needed while that holds.
- **U4** W7 follows (lift one-per-kind); no deployment to Node1 production.

## One helper

`assetsOfType(sim, type)` in `VEN/ui/src/api/assets.ts` (new): `[id, snapshot][]` of that
`asset_type`, sorted by `roster_index` then id. Every per-kind loop calls it; no component keeps
its own id list.
