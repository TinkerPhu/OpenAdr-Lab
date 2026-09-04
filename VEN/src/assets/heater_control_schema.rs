//! `Heater::control_schema` — split into its own file (Spec A Phase 2a, R-file-size)
//! to keep `heater.rs` under the file-size cap after adding the new trait wiring;
//! a separate inherent `impl Heater` block, same as any other split-out method.

use super::{ControlDescriptor, ControlKind, Heater};

impl Heater {
    pub fn control_schema(&self) -> Vec<ControlDescriptor> {
        vec![
            ControlDescriptor {
                key: "heater_temp_c".into(),
                label: "T_tank".into(),
                kind: ControlKind::Slider,
                min: Some(18.0),
                max: Some(95.0),
                unit: "°C".into(),
                display_scale: None,
                nullable: false,
            },
            ControlDescriptor {
                key: "heater_setpoint_c".into(),
                label: "Comfort target".into(),
                kind: ControlKind::Slider,
                min: Some(18.0),
                max: Some(95.0),
                unit: "°C".into(),
                display_scale: None,
                // Compared against temp_c by the dispatcher (build_setpoints) to
                // derive ON/OFF — not a continuous value like T_tank, and no
                // override is active most of the time. nullable so "no override"
                // reads as "Off" (pinned to the top of the range) rather than a
                // numeric value indistinguishable from an active override.
                nullable: true,
            },
            ControlDescriptor {
                key: "heater_temp_min_c".into(),
                label: "T_tank_min".into(),
                kind: ControlKind::Slider,
                min: Some(18.0),
                max: Some(94.0),
                unit: "°C".into(),
                display_scale: None,
                nullable: false,
            },
            ControlDescriptor {
                key: "heater_temp_max_c".into(),
                label: "T_tank_max".into(),
                kind: ControlKind::Slider,
                min: Some(19.0),
                max: Some(95.0),
                unit: "°C".into(),
                display_scale: None,
                nullable: false,
            },
            ControlDescriptor {
                key: "heater_emergency_curtail".into(),
                label: "Emergency curtail".into(),
                kind: ControlKind::Switch,
                min: None,
                max: None,
                unit: "".into(),
                display_scale: None,
                nullable: false,
            },
            ControlDescriptor {
                key: "heater_emergency_absorb".into(),
                label: "Emergency absorb".into(),
                kind: ControlKind::Switch,
                min: None,
                max: None,
                unit: "".into(),
                display_scale: None,
                nullable: false,
            },
        ]
    }
}
