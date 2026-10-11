// `GET /capability/:asset_id` fixtures (R-133): a complete `AssetCapabilityView`, tests override
// what they are about.
import type { AssetCapabilityView } from "../../api/types";

export function makeCapability(overrides: Partial<AssetCapabilityView> = {}): AssetCapabilityView {
  return {
    max_import_kw: 0,
    max_export_kw: 0,
    min_import_kw: 0,
    min_export_kw: 0,
    is_fixed: false,
    adjustability: "STEPLESS",
    step_rule: "CONTINUOUS",
    snap_to_zero_below_kw: 0,
    key_features: [],
    default_target_temp_c: null,
    default_target_soc_frac: null,
    ...overrides,
  };
}
