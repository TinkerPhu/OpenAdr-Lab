# Tasks

Branch: `refactor/asset-kind`.

- [x] 1.1 Test first: every asset declares its kind; `/sim` serializes it lowercase; an EV `car-a` is kind `ev`.
- [x] 1.2 `AssetKind`, `Asset::kind()`; delete `AssetType`, `asset_type()`, `asset_type_str()`, `TimelineAssetData.asset_type`.
- [x] 1.3 `AssetSnapshot.asset_type: AssetKind`; inject table and capacity headroom typed; regenerate UI types.
- [x] 1.4 UI typed end to end: `assets.ts` without id fallback, `dataBuilders`, colours/labels by kind, shiftable-load display names separate; UI tests.
- [ ] 2.1 Test first: obligations carry `aggregate`; SoC report per asset / aggregated; an EV `car-a` reports.
- [ ] 2.2 `Asset::storage_capacity_kwh`; samples carry capacity and connection; `build_soc_intervals` over every SoC series.
- [ ] 2.3 BDD: the multi-asset VEN reports one SoC resource per storage asset.
- [ ] 3.1 Docs, R-143 row removed, journal, wiki; checks; Node2 suites.
- [ ] 3.2 Delete this change; merge.
