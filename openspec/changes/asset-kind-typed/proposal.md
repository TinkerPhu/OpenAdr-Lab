# Proposal

## Why

An asset's type is a free string today. `Asset::asset_type_str()` returns `&'static str`, it goes
onto `/sim` as `asset_type: String`, and it is compared as a string in Rust
(`simulator/inject.rs`, `simulator/capacity_headroom.rs`) and in the UI (TypeScript `string`), so a
misspelling compiles. Beside it sits a second vocabulary, the `AssetType` catalog enum, which does
not match the assets that exist (base load is `GenericConsumer`, a shiftable load is
`WashingMachine`) and has `unimplemented!()` defaults. Where neither is at hand, the id stands in
for the type: the SoC report looks up the ids `"ev"`/`"battery"` (R-143), and the UI falls back to
the id (`assetTypeOf`), labels by `assetId === asset_type`, and colours by id first.

The SoC report also ignores the VTN's request: every OpenADR 3.1 `reportDescriptor` carries
`aggregate` (true: one result for all targeted resources; false, the default: one per resource),
and the VEN reads neither it nor more than one storage asset.

## What Changes

- **One closed type identifier, `AssetKind`** (`Pv`, `Battery`, `Ev`, `Heater`, `BaseLoad`,
  `ShiftableLoad`), declared by every asset (`Asset::kind()`, required, no default), carried on
  `AssetSnapshot.asset_type`, exported to the UI as a TypeScript union. Serialized as today's
  lowercase names, so profiles, history, BDD steps and the wire keep their spelling. The
  `AssetType` catalog enum, `asset_type()` and `asset_type_str()` are deleted; no string
  comparison of a type is left; the UI no longer substitutes an id for a type.
- **The SoC report follows the request (R-143).** `aggregate: false` gives one resource per asset
  that declares a state of charge (`<ven>-<asset_id>`); `aggregate: true` gives one resource with
  the energy-weighted SoC of the connected storage assets. Assets are selected by what they
  declare, not by id or kind.

## Capabilities

### New Capabilities
- `asset-kind`: every asset declares one closed kind; the SoC report honours `aggregate`.

## Impact

- VEN Rust: `entities/asset.rs`, `assets/*` (each `kind()`), `controller/sim_snapshot.rs`,
  `simulator/{snapshot,inject,capacity_headroom}.rs`, `entities/timeline.rs`,
  `controller/{reporter,report_intervals,openadr_interface}.rs`, `entities/capacity.rs`,
  `tasks/obligation.rs`; generated UI types.
- VEN UI: `api/assets.ts`, `components/controller/*` (summaries, colours, cells, chart specs),
  `components/devices/ComfortCurveCard`, `components/planner/PlanPowerStack`,
  `pages/{Dashboard,Devices}`.
- Wire: `/sim` unchanged in spelling. Reports: SoC resources named per asset when not aggregated.
