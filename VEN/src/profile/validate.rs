use super::schema::{AssetProfile, Profile};
use std::path::Path;

impl Profile {
    pub async fn try_load(path: &str) -> anyhow::Result<Self> {
        let contents = tokio::fs::read_to_string(Path::new(path)).await?;
        let profile: Profile = serde_yaml::from_str(&contents)?;
        if profile.assets.is_empty() {
            anyhow::bail!(
                "profile at '{}' has no assets — check the YAML 'assets:' list",
                path
            );
        }
        Ok(profile)
    }

    /// Validate profile invariants. Returns all violations at once so the user
    /// can fix all problems in a single startup attempt.
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors: Vec<String> = Vec::new();

        // At least one asset declared.
        if self.assets.is_empty() {
            errors.push("profile must declare at least one asset".into());
        }

        // Planner numeric bounds.
        if self.planner.replan_interval_s == 0 {
            errors.push("planner.replan_interval_s must be > 0".into());
        }
        if self.planner.phase2_epsilon_eur < 0.0 {
            errors.push(format!(
                "planner.phase2_epsilon_eur must be ≥ 0.0, got {}",
                self.planner.phase2_epsilon_eur
            ));
        }

        // Per-asset numeric bounds.
        for asset in &self.assets {
            match asset {
                AssetProfile::Ev(c) => {
                    if !(0.0..=1.0).contains(&c.soc_target) {
                        errors.push(format!(
                            "ev.soc_target must be in [0.0, 1.0], got {}",
                            c.soc_target
                        ));
                    }
                    if c.max_discharge_kw < 0.0 {
                        errors.push(format!(
                            "ev.max_discharge_kw must be ≥ 0.0, got {}",
                            c.max_discharge_kw
                        ));
                    }
                }
                AssetProfile::Battery(c) => {
                    if !(0.0..1.0).contains(&c.min_soc) {
                        errors.push(format!(
                            "battery.min_soc must be in [0.0, 1.0), got {}",
                            c.min_soc
                        ));
                    }
                    if c.round_trip_efficiency <= 0.0 || c.round_trip_efficiency > 1.0 {
                        errors.push(format!(
                            "battery.round_trip_efficiency must be in (0.0, 1.0], got {}",
                            c.round_trip_efficiency
                        ));
                    }
                }
                AssetProfile::BaseLoad(c) => {
                    for (i, spike) in c.spikes.iter().enumerate() {
                        if !(0.0..=1.0).contains(&spike.probability) {
                            errors.push(format!(
                                "base_load.spikes[{i}].probability must be in [0.0, 1.0], got {}",
                                spike.probability
                            ));
                        }
                        if spike.duration_h <= 0.0 {
                            errors.push(format!(
                                "base_load.spikes[{i}].duration_h must be > 0.0, got {}",
                                spike.duration_h
                            ));
                        }
                        if spike.ramp_h < 0.0 || spike.ramp_h > spike.duration_h / 2.0 {
                            errors.push(format!(
                                "base_load.spikes[{i}].ramp_h must be in [0.0, duration_h/2], \
                                 got ramp_h={} duration_h={}",
                                spike.ramp_h, spike.duration_h
                            ));
                        }
                        if spike.amplitude_kw < 0.0 {
                            errors.push(format!(
                                "base_load.spikes[{i}].amplitude_kw must be ≥ 0.0, got {}",
                                spike.amplitude_kw
                            ));
                        }
                        if spike.weekdays.iter().any(|&d| d > 6) {
                            errors.push(format!(
                                "base_load.spikes[{i}].weekdays entries must be 0-6 (Mon-Sun), got {:?}",
                                spike.weekdays
                            ));
                        }
                    }
                }
                _ => {}
            }
        }

        // plan_zones constraints: every zone's step_s must be a multiple of zone[0].step_s;
        // no zone may have step_s == 0 or slots == 0.
        if let Some(zones) = &self.planner.plan_zones {
            let base = zones.first().map(|z| z.step_s).unwrap_or(0);
            if base == 0 {
                errors.push("plan_zones[0].step_s must be > 0".into());
            } else {
                for (i, z) in zones.iter().enumerate() {
                    if z.step_s == 0 {
                        errors.push(format!("plan_zones[{i}].step_s must be > 0"));
                    } else if z.step_s % base != 0 {
                        errors.push(format!(
                            "plan_zones[{i}].step_s ({}) is not a multiple of zone[0].step_s ({})",
                            z.step_s, base
                        ));
                    }
                    if z.slots == 0 {
                        errors.push(format!("plan_zones[{i}].slots must be > 0"));
                    }
                }
            }
        }

        // phase2_epsilon_eur sanity check: when a heater is present and the epsilon is
        // non-zero, it must not exceed 6× the effective per-switch cost
        // (switching_penalty_eur × step_s/3600). At 6× the effective cost the epsilon
        // already allows the Phase 2 solver to accept solutions with 6 extra relay
        // operations; values well above this override the Phase 1 cost objective.
        if self.planner.phase2_epsilon_eur > 0.0 {
            if let Some(AssetProfile::Heater(h)) = self
                .assets
                .iter()
                .find(|a| matches!(a, AssetProfile::Heater(_)))
            {
                // Use the longest zone step for the bound — that is the most expensive
                // switch in MILP terms, giving the most conservative (largest) ceiling.
                let longest_step_s =
                    self.planner
                        .plan_zones
                        .as_ref()
                        .and_then(|z| z.iter().map(|z| z.step_s).max())
                        .unwrap_or(self.planner.plan_step_s) as f64;
                let effective_switch_cost =
                    h.effective_switching_penalty() * (longest_step_s / 3600.0);
                let sanity_bound = effective_switch_cost * 6.0;
                if sanity_bound > 0.0 && self.planner.phase2_epsilon_eur > sanity_bound {
                    let ratio = self.planner.phase2_epsilon_eur / effective_switch_cost;
                    let target = effective_switch_cost * 2.0;
                    errors.push(format!(
                        "planner.phase2_epsilon_eur ({:.3}) is {:.1}× the effective per-switch \
                         cost ({:.3} EUR); expected ≤ {:.3}. Reduce to ~{:.2}.",
                        self.planner.phase2_epsilon_eur,
                        ratio,
                        effective_switch_cost,
                        sanity_bound,
                        target,
                    ));
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::plan::PlanZone;
    use crate::profile::schema::{HeaterConfig, PlannerConfig};

    #[test]
    fn heater_config_switching_penalty_default() {
        let cfg = HeaterConfig {
            id: "heater".into(),
            max_kw: 3.0,
            temp_initial_c: 20.0,
            temp_min_c: 18.0,
            temp_max_c: 23.0,
            mid_kw: None,
            volume_l: None,
            thermal_mass_kwh_per_c: None,
            k_loss_kw_per_c: None,
            draw_kw: None,
            switching_penalty_eur: None,
            c_terminal_eur_kwh: None,
        };
        assert!((cfg.effective_switching_penalty() - 0.01).abs() < 1e-9);
    }

    #[test]
    fn heater_config_switching_penalty_explicit() {
        let cfg = HeaterConfig {
            id: "heater".into(),
            max_kw: 3.0,
            temp_initial_c: 20.0,
            temp_min_c: 18.0,
            temp_max_c: 23.0,
            mid_kw: None,
            volume_l: None,
            thermal_mass_kwh_per_c: None,
            k_loss_kw_per_c: None,
            draw_kw: None,
            switching_penalty_eur: Some(0.05),
            c_terminal_eur_kwh: None,
        };
        assert!((cfg.effective_switching_penalty() - 0.05).abs() < 1e-9);
    }

    #[test]
    fn heater_config_yaml_without_penalty_field() {
        let yaml = r#"
type: heater
id: heater
max_kw: 3.0
temp_initial_c: 20.0
temp_min_c: 18.0
temp_max_c: 23.0
"#;
        let asset: AssetProfile = serde_yaml::from_str(yaml).expect("should parse heater yaml");
        if let AssetProfile::Heater(cfg) = asset {
            assert!(
                cfg.switching_penalty_eur.is_none(),
                "penalty should default to None"
            );
            assert!((cfg.effective_switching_penalty() - 0.01).abs() < 1e-9);
        } else {
            panic!("expected AssetProfile::Heater");
        }
    }

    #[test]
    fn base_load_yaml_round_trip_with_spikes() {
        let yaml = r#"
type: base_load
id: base_load
baseline_kw: 0.4
spikes:
  - center_hour: 8.0
    amplitude_kw: 1.2
    duration_h: 0.25
    ramp_h: 0.03
    probability: 1.0
    weekdays: [0, 1, 2, 3, 4]
  - center_hour: 18.0
    amplitude_kw: 2.5
    jitter_h: 0.3
"#;
        let asset: AssetProfile =
            serde_yaml::from_str(yaml).expect("should parse base_load yaml with spikes");
        let AssetProfile::BaseLoad(cfg) = asset else {
            panic!("expected AssetProfile::BaseLoad");
        };
        assert_eq!(cfg.spikes.len(), 2);
        assert!((cfg.spikes[0].center_hour - 8.0).abs() < 1e-9);
        assert!((cfg.spikes[0].amplitude_kw - 1.2).abs() < 1e-9);
        assert!((cfg.spikes[0].probability - 1.0).abs() < 1e-9);
        assert_eq!(cfg.spikes[0].weekdays, vec![0, 1, 2, 3, 4]);
        // Second spike sets jitter_h explicitly, omits duration_h/ramp_h/probability/weekdays.
        assert!((cfg.spikes[1].jitter_h - 0.3).abs() < 1e-9);
        assert!(
            (cfg.spikes[1].duration_h - 0.5).abs() < 1e-9,
            "duration_h should default to 0.5 when omitted"
        );
        assert!(
            (cfg.spikes[1].ramp_h - 0.05).abs() < 1e-9,
            "ramp_h should default to 0.05 when omitted"
        );
        assert!(
            (cfg.spikes[1].probability - 1.0).abs() < 1e-9,
            "probability should default to 1.0 when omitted"
        );
        assert!(
            cfg.spikes[1].weekdays.is_empty(),
            "weekdays should default to empty (every day) when omitted"
        );
    }

    #[test]
    fn base_load_no_spikes_key_defaults_to_empty() {
        let yaml = "type: base_load\nid: base_load\nbaseline_kw: 0.5\n";
        let asset: AssetProfile = serde_yaml::from_str(yaml).expect("should parse base_load yaml");
        let AssetProfile::BaseLoad(cfg) = asset else {
            panic!("expected AssetProfile::BaseLoad");
        };
        assert!(cfg.spikes.is_empty());
    }

    fn base_load_profile_with_spike(
        spike_overrides: crate::profile::schema::SpikeConfig,
    ) -> Profile {
        Profile {
            assets: vec![AssetProfile::BaseLoad(
                crate::profile::schema::BaseLoadConfig {
                    id: "base_load".into(),
                    baseline_kw: 0.5,
                    spikes: vec![spike_overrides],
                },
            )],
            ..Profile::default()
        }
    }

    fn valid_spike() -> crate::profile::schema::SpikeConfig {
        crate::profile::schema::SpikeConfig {
            center_hour: 8.0,
            jitter_h: 0.2,
            amplitude_kw: 1.2,
            duration_h: 0.25,
            ramp_h: 0.03,
            probability: 1.0,
            weekdays: vec![],
        }
    }

    #[test]
    fn validate_rejects_spike_probability_out_of_range() {
        let profile = base_load_profile_with_spike(crate::profile::schema::SpikeConfig {
            probability: 1.5,
            ..valid_spike()
        });
        let result = profile.validate();
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.iter().any(|e| e.contains("probability")));
    }

    #[test]
    fn validate_rejects_spike_non_positive_duration_h() {
        let profile = base_load_profile_with_spike(crate::profile::schema::SpikeConfig {
            duration_h: 0.0,
            ..valid_spike()
        });
        let result = profile.validate();
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.iter().any(|e| e.contains("duration_h")));
    }

    #[test]
    fn validate_rejects_ramp_h_exceeding_half_duration() {
        let profile = base_load_profile_with_spike(crate::profile::schema::SpikeConfig {
            duration_h: 0.5,
            ramp_h: 0.4, // > duration_h/2 = 0.25
            ..valid_spike()
        });
        let result = profile.validate();
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.iter().any(|e| e.contains("ramp_h")));
    }

    #[test]
    fn validate_rejects_negative_spike_amplitude() {
        let profile = base_load_profile_with_spike(crate::profile::schema::SpikeConfig {
            amplitude_kw: -1.0,
            ..valid_spike()
        });
        let result = profile.validate();
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.iter().any(|e| e.contains("amplitude_kw")));
    }

    #[test]
    fn validate_rejects_invalid_weekday_entry() {
        let profile = base_load_profile_with_spike(crate::profile::schema::SpikeConfig {
            weekdays: vec![0, 7], // 7 is out of range (0-6)
            ..valid_spike()
        });
        let result = profile.validate();
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.iter().any(|e| e.contains("weekdays")));
    }

    #[test]
    fn validate_accepts_valid_spike_list() {
        let profile = base_load_profile_with_spike(crate::profile::schema::SpikeConfig {
            probability: 0.8,
            weekdays: vec![5, 6],
            ..valid_spike()
        });
        assert!(profile.validate().is_ok());
    }

    #[tokio::test]
    async fn profile_empty_assets_guard() {
        // try_load must reject a YAML that parses but has no assets.
        let dir = std::env::temp_dir();
        let path = dir.join("empty_assets_profile_test.yaml");
        tokio::fs::write(&path, "simulator:\n  tick_s: 1\n")
            .await
            .unwrap();
        let result = Profile::try_load(path.to_str().unwrap()).await;
        assert!(
            result.is_err(),
            "try_load must return Err for empty assets list"
        );
        let msg = format!("{}", result.unwrap_err());
        assert!(
            msg.contains("no assets"),
            "error message should mention 'no assets': {msg}"
        );
        let _ = tokio::fs::remove_file(path).await;
    }

    fn make_valid_profile() -> Profile {
        let yaml = r#"
assets:
  - type: battery
    id: battery
    capacity_kwh: 10.0
    min_soc: 0.10
    round_trip_efficiency: 0.92
  - type: ev
    id: ev
    soc_target: 0.80
    max_discharge_kw: 0.0
"#;
        serde_yaml::from_str(yaml).unwrap()
    }

    #[test]
    fn validate_passes_for_valid_profile() {
        let p = make_valid_profile();
        assert!(p.validate().is_ok(), "valid profile must pass validation");
    }

    #[test]
    fn validate_fails_for_empty_assets() {
        let mut p = make_valid_profile();
        p.assets.clear();
        let errs = p.validate().unwrap_err();
        assert!(errs.iter().any(|e| e.contains("at least one asset")));
    }

    #[test]
    fn validate_fails_for_soc_target_out_of_range() {
        let yaml = r#"
assets:
  - type: ev
    id: ev
    soc_target: 1.5
"#;
        let p: Profile = serde_yaml::from_str(yaml).unwrap();
        let errs = p.validate().unwrap_err();
        assert!(errs.iter().any(|e| e.contains("soc_target")));
    }

    #[test]
    fn validate_fails_for_round_trip_efficiency_zero() {
        let yaml = r#"
assets:
  - type: battery
    id: battery
    round_trip_efficiency: 0.0
"#;
        let p: Profile = serde_yaml::from_str(yaml).unwrap();
        let errs = p.validate().unwrap_err();
        assert!(errs.iter().any(|e| e.contains("round_trip_efficiency")));
    }

    #[test]
    fn validate_reports_multiple_violations_at_once() {
        let yaml = r#"
assets:
  - type: ev
    id: ev
    soc_target: 1.5
    max_discharge_kw: -1.0
"#;
        let p: Profile = serde_yaml::from_str(yaml).unwrap();
        let errs = p.validate().unwrap_err();
        assert!(
            errs.len() >= 2,
            "expected ≥ 2 errors, got {}: {:?}",
            errs.len(),
            errs
        );
    }

    fn make_heater_profile(switching_penalty_eur: f64, phase2_epsilon_eur: f64) -> Profile {
        let yaml = format!(
            r#"
assets:
  - type: heater
    id: heater
    max_kw: 6.0
    temp_initial_c: 50.0
    temp_min_c: 45.0
    temp_max_c: 60.0
    switching_penalty_eur: {switching_penalty_eur}
planner:
  phase2_epsilon_eur: {phase2_epsilon_eur}
"#
        );
        serde_yaml::from_str(&yaml).unwrap()
    }

    #[test]
    fn test_validate_phase2_epsilon_rejects_misconfiguration() {
        // switching_penalty=0.50, step=600s → effective=0.083 EUR/switch, bound=0.50
        // phase2_epsilon=5.0 is ~10× the bound → must be rejected
        let p = make_heater_profile(0.50, 5.0);
        let errs = p.validate().unwrap_err();
        assert!(
            errs.iter().any(|e| e.contains("phase2_epsilon_eur")),
            "expected phase2_epsilon_eur violation, got: {errs:?}"
        );
    }

    #[test]
    fn test_validate_phase2_epsilon_accepts_correct_value() {
        // 2× effective cost = 2 × 0.083 ≈ 0.17 EUR → well within bound
        let p = make_heater_profile(0.50, 0.17);
        assert!(
            p.validate().is_ok(),
            "phase2_epsilon_eur=0.17 should pass validation"
        );
    }

    #[test]
    fn test_validate_phase2_epsilon_skipped_without_heater() {
        // No heater → check is irrelevant regardless of value
        let yaml = r#"
assets:
  - type: battery
    id: battery
    capacity_kwh: 10.0
    min_soc: 0.10
    round_trip_efficiency: 0.92
planner:
  phase2_epsilon_eur: 99.0
"#;
        let p: Profile = serde_yaml::from_str(yaml).unwrap();
        assert!(
            p.validate().is_ok(),
            "no heater → phase2_epsilon check must be skipped"
        );
    }

    #[test]
    fn test_default_planner_config_has_correct_values() {
        let cfg = PlannerConfig::default();
        assert_eq!(cfg.c_ctrl_imp_malus_eur_kwh, 0.22);
        assert_eq!(cfg.plan_adoption_threshold_eur, 0.20);
        assert!((cfg.plan_adoption_decay_s - 1500.0).abs() < 1e-9);
        assert_eq!(cfg.plan_step_s, 600);
        assert_eq!(cfg.plan_horizon_h, 48);
    }

    #[test]
    fn test_plan_zones_derive_effective_step_and_horizon() {
        let cfg = PlannerConfig {
            plan_zones: Some(vec![
                PlanZone {
                    step_s: 300,
                    slots: 96,
                }, // 8 h
                PlanZone {
                    step_s: 600,
                    slots: 96,
                }, // 16 h
                PlanZone {
                    step_s: 900,
                    slots: 96,
                }, // 24 h
            ]),
            ..Default::default()
        };
        assert_eq!(cfg.effective_step_s(), 300);
        assert_eq!(cfg.effective_horizon_h(), 48);
    }

    #[test]
    fn test_plan_zones_single_zone_matches_test_profile_values() {
        let cfg = PlannerConfig {
            plan_zones: Some(vec![PlanZone {
                step_s: 3600,
                slots: 24,
            }]),
            ..Default::default()
        };
        assert_eq!(cfg.effective_step_s(), 3600);
        assert_eq!(cfg.effective_horizon_h(), 24);
    }

    #[test]
    fn test_plan_zones_no_zones_falls_back_to_scalar() {
        let cfg = PlannerConfig::default();
        // plan_zones absent → effective values come from plan_step_s / plan_horizon_h defaults
        assert_eq!(cfg.effective_step_s(), 600);
        assert_eq!(cfg.effective_horizon_h(), 48);
    }

    #[test]
    fn test_validate_plan_zones_rejects_non_multiple() {
        let yaml = r#"
assets:
  - type: battery
    id: battery
    capacity_kwh: 10.0
    min_soc: 0.10
    round_trip_efficiency: 0.92
planner:
  plan_zones:
    - step_s: 300
      slots: 96
    - step_s: 700
      slots: 96
"#;
        let p: Profile = serde_yaml::from_str(yaml).unwrap();
        let errs = p.validate().unwrap_err();
        assert!(
            errs.iter().any(|e| e.contains("plan_zones")),
            "expected plan_zones violation, got: {errs:?}"
        );
    }

    #[test]
    fn test_validate_plan_zones_accepts_multiples() {
        let yaml = r#"
assets:
  - type: battery
    id: battery
    capacity_kwh: 10.0
    min_soc: 0.10
    round_trip_efficiency: 0.92
planner:
  plan_zones:
    - step_s: 300
      slots: 96
    - step_s: 600
      slots: 96
    - step_s: 900
      slots: 96
"#;
        let p: Profile = serde_yaml::from_str(yaml).unwrap();
        assert!(
            p.validate().is_ok(),
            "300/600/900 are all multiples of 300 — should pass"
        );
    }

    #[test]
    fn test_validate_plan_zones_rejects_zero_step() {
        let yaml = r#"
assets:
  - type: battery
    id: battery
    capacity_kwh: 10.0
    min_soc: 0.10
    round_trip_efficiency: 0.92
planner:
  plan_zones:
    - step_s: 0
      slots: 96
"#;
        let p: Profile = serde_yaml::from_str(yaml).unwrap();
        let errs = p.validate().unwrap_err();
        assert!(
            errs.iter().any(|e| e.contains("plan_zones")),
            "zero step_s should be rejected: {errs:?}"
        );
    }

    #[tokio::test]
    async fn test_yaml_round_trip_plan_zones() {
        // Verify that plan_zones parses correctly from YAML.
        let yaml = r#"
assets:
  - type: battery
    id: battery
    capacity_kwh: 10.0
    min_soc: 0.10
    round_trip_efficiency: 0.92
planner:
  plan_zones:
    - step_s: 3600
      slots: 24
"#;
        let p: Profile = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(p.planner.effective_step_s(), 3600);
        assert_eq!(p.planner.effective_horizon_h(), 24);
    }
}
