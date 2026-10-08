//! Pure translation from the loaded `Profile` into the domain-layer param
//! structs (`SimulatorParams`, `PlannerParams`, `Vec<AssetParams>`) consumed
//! by `main.rs` at startup. Split out of `main.rs` (which stays orchestration
//! only) to keep that file under the `VEN/src/` file-size cap.

use crate::entities::asset_params::AssetParams;
use crate::entities::planner_params::{PlannerParams, SimulatorParams};
use crate::profile::Profile;

pub fn build_domain_params(
    profile: &Profile,
) -> (SimulatorParams, PlannerParams, Vec<AssetParams>) {
    let sim_params = SimulatorParams {
        tick_s: profile.simulator.tick_s,
        persist_every_s: profile.simulator.persist_every_s,
    };
    let planner_params = PlannerParams::from(&profile.planner);
    let asset_params = profile.asset_params();
    (sim_params, planner_params, asset_params)
}
