# Design

## Inventory (where the concept lives today)

| Place | Today | After |
|---|---|---|
| `entities/asset.rs::AssetType` | catalog enum, 9 variants; base load = `GenericConsumer`, shiftable = `WashingMachine` | deleted; `AssetKind` (6 variants) |
| `assets/asset_trait.rs::asset_type`, `asset_type_str` | two methods, `unimplemented!()` defaults | one required `kind()` |
| `assets/{pv,battery,ev,heater,base_load,shiftable_load}.rs` | implement both | implement `kind()` |
| `controller/sim_snapshot.rs::AssetSnapshot.asset_type` | `String` | `AssetKind` |
| `simulator/snapshot.rs` | `asset_type_str().to_string()`; timeline `asset_type()` | `kind()` |
| `entities/timeline.rs::TimelineAssetData.asset_type` | `AssetType`, read by no one | removed |
| `simulator/inject.rs::StateInjection.asset_type` | `&'static str` compared with `asset_type_str()` | `AssetKind` |
| `simulator/capacity_headroom.rs` | `asset_type_str() == "base_load"` | `kind() == AssetKind::BaseLoad` |
| `controller/report_intervals.rs::build_soc_intervals` | samples of ids `"ev"`, else `"battery"` | every sample series with a SoC |
| `controller/reporter.rs` | one resource `<ven>-meter` | SoC: per asset or aggregated; other payloads unchanged |
| `controller/openadr_interface.rs::extract_report_obligations` | drops `aggregate` | carries it on `OadrReportObligation` |
| `tasks/obligation.rs` | samples: ts, power, soc | plus the asset's capacity and connection |
| UI `api/assets.ts` | `assetsOfType(sim, string)`; `assetTypeOf` falls back to the id | typed `AssetKind`; no fallback |
| UI `dataBuilders.ts` | `SUMMARY_BY_TYPE: Record<string>`; label by `assetId === asset_type`; colour by id first | `Record<AssetKind>`; the label carries the id when its kind has more than one asset; colour by kind |
| UI `types.ts` | `ASSET_COLORS`/`ASSET_LABELS` keyed by id and type mixed | by kind; request-created shiftable loads (`wm`, `dw`) keep an id-keyed display-name map, named as such |
| UI `AssetCell`, `AssetRightSection`, `ComfortCurveCard`, `assetChartSpecs`, `PlanPowerStack`, `GridAccumulatedCell` | `string` types, `=== "heater"` | `AssetKind` |

Reused: `ui_types.rs` (generated TS), `report_payload::descriptors_for`, the `TimeSeries`
resampling in `build_soc_intervals`, `AssetHistoryView.plugged`, `AssetState::soc_frac`.

## Decisions

- **K1** Serialized lowercase (`"ev"`, `"base_load"`, `"shiftable_load"`): the spelling profiles
  (`type: ev`), history and BDD already use. The fixed meaning comes from the enum and the
  generated union, not from the spelling.
- **K2** `AssetKind` is for classification, routing and display. Behaviour stays a declaration
  asked of the asset (has a SoC, lever, request kind); the planner core stays kind-free (audit).
- **K3** Profile `type:` already maps to `AssetParams` variants; no second mapping is added unless
  a caller needs one.
- **S1** `aggregate` is honoured for `STORAGE_CHARGE_LEVEL`/`STORAGE_CHARGE_STATE`, the device
  payloads. Site payloads (usage, capacity limits, reservations, forecasts) are quantities of the
  site meter, the one resource they belong to, and keep `<ven>-meter` either way.
- **S2** Per-asset resource name `<ven>-<asset_id>`; aggregated resource `<ven>-meter`.
- **S3** Aggregated SoC = Σ soc·capacity / Σ capacity over connected storage assets (an unplugged
  EV's energy is not available to the site). Not aggregated: every storage asset reports its own
  SoC, plugged or not (the SoC is known; connection is not a SoC).
- **S4** Capacity is declared by the asset (`Asset::storage_capacity_kwh`), not read from a
  `values` key.
