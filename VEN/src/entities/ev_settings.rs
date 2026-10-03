// ── EV settings: the user's standing preference for the charging overlay ──────
// A domain value, not a piece of application wiring, which is why it lives here
// rather than beside `AppState` (R-39): its two booleans mean the same thing to
// whoever reads them, whether they came from a route, a tick, or a restored
// snapshot. `HemsState` stayed in `state/` by the same test — its field list is
// defined by what must survive a restart, which is a persistence concern.

use serde::{Deserialize, Serialize};

/// User-controllable settings for the opportunistic EV charging overlay.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EvSettings {
    /// When true (default), the dispatcher routes live PV surplus to the EV when
    /// no EvSession is active. Automatically paused while an EvSession exists.
    #[serde(default = "opportunistic_charging_default")]
    pub opportunistic_charging_enabled: bool,
    /// Derived: true while any EvSession is active. Set by tick loop, not user-settable.
    #[serde(default)]
    pub paused_by_active_session: bool,
}

/// Opportunistic charging is on unless the user turns it off, so a snapshot
/// written before the field existed restores with the overlay enabled.
fn opportunistic_charging_default() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opportunistic_charging_defaults_on_when_the_field_is_absent() {
        let s: EvSettings = serde_json::from_str("{}").unwrap();
        assert!(s.opportunistic_charging_enabled);
        assert!(!s.paused_by_active_session);
    }

    #[test]
    fn an_explicit_false_survives_a_round_trip() {
        let s = EvSettings {
            opportunistic_charging_enabled: false,
            paused_by_active_session: true,
        };
        let back: EvSettings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert!(!back.opportunistic_charging_enabled);
        assert!(back.paused_by_active_session);
    }
}
