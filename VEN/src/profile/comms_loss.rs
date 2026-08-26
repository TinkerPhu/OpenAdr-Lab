//! YAML config for VTN-communication-loss power curtailment (R-59) — split
//! out of `schema.rs` to keep that file under the file-size cap. Opt-in:
//! an absent `comms_loss:` section means zero behavior change (existing
//! profiles/E2E scenarios unaffected), matching `weather_pv`/`measurements`.
//! One generic knob applied identically to every controllable asset (PV
//! export ceiling; EV/heater import ceiling; battery charge+discharge
//! ceiling) rather than one fail-safe per asset type.

use serde::Deserialize;

/// `comms_loss:` profile section. Presence of the section is itself the
/// enable flag — no separate `enabled` bool, matching the `weather_pv`/
/// `measurements` idiom.
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct CommsLossConfig {
    /// Fraction (0.0, 1.0] of each asset's own max power ceiling to curtail
    /// to once comms-loss is confirmed (debounced). E.g. 0.7 = 70%.
    #[serde(default = "super::defaults::default_comms_loss_max_power_pct")]
    pub max_power_pct: f64,
    /// Consecutive seconds the VTN must be unreachable before curtailment
    /// engages — avoids nuisance-tripping on transient poll blips already
    /// absorbed by `tasks/backoff.rs`'s exponential backoff.
    #[serde(default = "super::defaults::default_comms_loss_debounce_s")]
    pub debounce_s: u64,
}

#[cfg(test)]
mod tests {
    #[test]
    fn profile_without_comms_loss_parses_with_none() {
        let profile: crate::profile::Profile = serde_yaml::from_str("assets: []").unwrap();
        assert!(profile.comms_loss.is_none());
    }

    #[test]
    fn profile_with_comms_loss_parses_and_applies_defaults() {
        let yaml = "assets: []\ncomms_loss: {}\n";
        let profile: crate::profile::Profile = serde_yaml::from_str(yaml).unwrap();
        let cl = profile.comms_loss.expect("comms_loss must parse to Some");
        assert_eq!(cl.max_power_pct, 0.7);
        assert_eq!(cl.debounce_s, 60);
    }

    #[test]
    fn profile_with_comms_loss_overrides_pct_and_debounce() {
        let yaml = "assets: []\ncomms_loss:\n  max_power_pct: 0.5\n  debounce_s: 30\n";
        let profile: crate::profile::Profile = serde_yaml::from_str(yaml).unwrap();
        let cl = profile.comms_loss.expect("comms_loss must parse to Some");
        assert_eq!(cl.max_power_pct, 0.5);
        assert_eq!(cl.debounce_s, 30);
    }
}
