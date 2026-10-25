// `SimSnapshot` fixtures for UI tests (R-133). An asset is written as the wire carries it: its
// typed fields and its own state values (`soc`, `temp_c`, ...) side by side, the values being
// flattened on the wire and read through `assetValue`. Typed fields a test leaves out get a
// neutral default, so each test states only what it is about.
import type { AssetKind } from "../../api/generated/AssetKind";
import type { AssetSnapshot, GridSnapshot, SimInjectState, SimSnapshot } from "../../api/types";

type AssetFields = Partial<AssetSnapshot> & Record<string, unknown>;

export function makeAsset(fields: AssetFields = {}): AssetSnapshot {
  return {
    power_kw: 0,
    asset_type: "unknown",
    cap_max_import_kw: 0,
    cap_max_export_kw: 0,
    default_setpoint_kw: 0,
    setpoint_kw: 0,
    embodied_co2_g_kwh: null,
    step_rule: "CONTINUOUS",
    snap_to_zero_below_kw: 0,
    roster_index: 0,
    ...fields,
  } as AssetSnapshot;
}

/** Fixture shorthand only: the default id a profile gives each kind when it omits `id:`. A test
 * asset that states no `asset_type` and has one of these ids is of that kind; any other id is a
 * request-created shiftable load (the kind with no default id). Production code never maps an id
 * to a kind. */
const KIND_OF_DEFAULT_ID: Record<string, AssetKind> = {
  ev: "ev",
  heater: "heater",
  pv: "pv",
  battery: "battery",
  base_load: "base_load",
};

/** Assets by id, in the order written (`roster_index`). */
export function makeAssets(assets: Record<string, AssetFields>): Record<string, AssetSnapshot> {
  return Object.fromEntries(
    Object.entries(assets).map(([id, a], i) => [
      id,
      makeAsset({
        asset_type: KIND_OF_DEFAULT_ID[id] ?? "shiftable_load",
        roster_index: i,
        ...a,
      }),
    ]),
  );
}

export function makeGrid(fields: Partial<GridSnapshot> = {}): GridSnapshot {
  return {
    net_power_kw: 0,
    voltage_v: 230,
    import_kwh: 0,
    export_kwh: 0,
    ...fields,
  };
}

export function makeSim(
  assets: Record<string, AssetFields> = {},
  grid: Partial<GridSnapshot> = {},
  ts = "2026-04-04T10:00:00Z",
): SimSnapshot {
  return { ts, grid: makeGrid(grid), assets: makeAssets(assets) };
}

/** No inject active: every override cleared, the blend rates at `SimInjectState::default()`'s. */
export function makeInject(fields: Partial<SimInjectState> = {}): SimInjectState {
  return {
    battery_soc: null,
    ev_soc: null,
    heater_temp_c: null,
    pv_irradiance: null,
    pv_tau_s: 2847.37, // DEFAULT_PV_TAU_S
    base_load_kw: null,
    base_load_alpha: 0.1,
    ev_plugged: null,
    ev_soc_target: null,
    heater_setpoint_c: null,
    heater_temp_min_c: null,
    heater_temp_max_c: null,
    heater_emergency_curtail: null,
    heater_emergency_absorb: null,
    ambient_temp_c: null,
    grid_import_limit_kw: null,
    grid_export_limit_kw: null,
    pv_generation_limit_kw: null,
    pv_plan_kw: null,
    ...fields,
  };
}
