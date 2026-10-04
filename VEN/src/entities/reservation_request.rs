//! Additional grid capacity the VEN is asking the VTN for (R-76).
//!
//! OpenADR 3.1 defines the two report payload types this feeds as
//! "Amount of additional import/export capacity **requested**"
//! (`docs/openadr_3_1_specs/2_OpenADR 3.1.0_Definition_20250801.md`, Table 2),
//! each with a companion `*_RESERVATION_FEE` saying what the VEN will pay for
//! it. So the quantity is a **VEN-initiated request to raise its contracted
//! allowance** — the outbound half of the mechanism whose inbound half this
//! VEN already honours: `IMPORT_CAPACITY_RESERVATION` / `*_SUBSCRIPTION`
//! events parse into `OadrCapacityState` and bind the planner's slot caps
//! (`controller::milp_planner::inputs`, WP3.3 §8.10).
//!
//! That makes it a different quantity from live site headroom, which is what
//! `controller::reporter` used to send here. Three things were wrong with
//! that, and this module exists to fix all three in one place rather than
//! leave the arithmetic inline in a report mapper:
//!
//! 1. **The directions were crossed.** `SiteFlexibilityEnvelope::up_kw` holds
//!    the *Export*-commitment power and `down_kw` the *Import*-commitment
//!    power; the `IMPORT_` payload read `up_kw` and the `EXPORT_` payload read
//!    `down_kw`. Reproduced before fixing, by
//!    `reporter::tests::import_reservation_capacity_reads_the_import_direction`.
//! 2. **Absolute where the spec says "additional".** Headroom is what the site
//!    could draw or inject in total; a reservation request is the part of that
//!    the current contract does not already cover.
//! 3. **`.abs()` masked a legitimate sign flip.** `up_kw` can go positive —
//!    net-importing even under a sustained Export commitment, when
//!    non-exportable draw like base load exceeds what is exportable (see
//!    `SiteFlexibilityEnvelope`'s own doc comment). Taking its magnitude
//!    reported that net *import* as export capacity. Here a direction the site
//!    cannot serve at all contributes zero, by construction.
//!
//! **Capability, not counterfactual desire.** "What the site wants" would
//! strictly mean re-solving the plan with the allowance lifted and reading how
//! much more it would then draw. This asks the cheaper question the existing
//! entities can answer: the site's own max-effort capability in that
//! direction, minus what it is already contracted for. That is an upper bound
//! on the useful request and never understates the binding constraint, which
//! is the property that matters for asking. The counterfactual refinement is a
//! deliberate non-goal — it needs a second solve per direction per report.

use serde::{Deserialize, Serialize};

use crate::entities::capacity::OadrCapacityState;
use crate::entities::plan::SiteFlexibilityEnvelope;

/// Additional capacity requested per direction (kW), both `>= 0`.
///
/// Zero means "nothing to ask for" — either the contract already covers what
/// the site can do, or no allowance was ever declared so nothing binds. Both
/// are honest zeros, and they are the normal case in this lab, where no VTN
/// program issues subscription or reservation events.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct ReservationRequest {
    /// Import capacity requested beyond the contracted import allowance (kW).
    pub import_kw: f64,
    /// Export capacity requested beyond the contracted export allowance (kW).
    pub export_kw: f64,
}

impl ReservationRequest {
    /// What to ask for, given what the site can do and what it is allowed.
    ///
    /// `headroom`'s fields are signed (positive = import) per
    /// `SiteFlexibilityEnvelope`'s convention, so each direction's capability
    /// is read in its own sign and floored at zero: a direction the site
    /// cannot serve is not a negative request, it is no request.
    pub fn from_headroom(headroom: &SiteFlexibilityEnvelope, capacity: &OadrCapacityState) -> Self {
        Self {
            import_kw: Self::beyond(headroom.down_kw, capacity.import_allowance_kw()),
            export_kw: Self::beyond(-headroom.up_kw, capacity.export_allowance_kw()),
        }
    }

    /// The part of `capability_kw` that `allowance_kw` does not already cover.
    ///
    /// An `INFINITY` allowance (nothing declared) yields zero through the same
    /// arithmetic, with no branch of its own.
    fn beyond(capability_kw: f64, allowance_kw: f64) -> f64 {
        (capability_kw.max(0.0) - allowance_kw).max(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn headroom(up_kw: f64, down_kw: f64) -> SiteFlexibilityEnvelope {
        SiteFlexibilityEnvelope {
            ts: Utc::now(),
            up_kw,
            down_kw,
            up_duration_s: None,
            down_duration_s: None,
        }
    }

    fn allowance(import_kw: Option<f64>, export_kw: Option<f64>) -> OadrCapacityState {
        OadrCapacityState {
            import_subscription_kw: import_kw,
            export_subscription_kw: export_kw,
            ..OadrCapacityState::default()
        }
    }

    /// The defect R-76 names: each direction must be read in its own sign.
    /// Distinct magnitudes, so a swap cannot pass by coincidence.
    #[test]
    fn from_headroom_reads_each_direction_in_its_own_sign() {
        // Can draw 11 kW, can inject 8 kW; contracted for 7 and 2.
        let req = ReservationRequest::from_headroom(
            &headroom(-8.0, 11.0),
            &allowance(Some(7.0), Some(2.0)),
        );
        assert_eq!(req.import_kw, 4.0, "11 kW capability less 7 kW contracted");
        assert_eq!(req.export_kw, 6.0, "8 kW capability less 2 kW contracted");
    }

    #[test]
    fn from_headroom_asks_for_nothing_when_no_allowance_is_declared() {
        let req = ReservationRequest::from_headroom(&headroom(-8.0, 11.0), &allowance(None, None));
        assert_eq!(
            (req.import_kw, req.export_kw),
            (0.0, 0.0),
            "nothing binds, so there is nothing to request"
        );
    }

    #[test]
    fn from_headroom_asks_for_nothing_when_the_contract_already_covers_capability() {
        let req = ReservationRequest::from_headroom(
            &headroom(-2.0, 3.0),
            &allowance(Some(10.0), Some(10.0)),
        );
        assert_eq!((req.import_kw, req.export_kw), (0.0, 0.0));
    }

    /// `up_kw` positive means net-importing even under a sustained Export
    /// commitment — the site cannot export at all. The old `.abs()` reported
    /// that as export capacity; a direction the site cannot serve must ask for
    /// nothing in it.
    #[test]
    fn from_headroom_treats_an_unexportable_site_as_asking_for_no_export() {
        let req = ReservationRequest::from_headroom(
            &headroom(1.5, 11.0),
            &allowance(Some(7.0), Some(2.0)),
        );
        assert_eq!(req.export_kw, 0.0, "cannot export, so asks for no export");
        assert_eq!(req.import_kw, 4.0, "the import side is unaffected");
    }

    /// A subscription and a reservation sum into one allowance (WP3.3 §8.10),
    /// through the entity that owns the rule.
    #[test]
    fn from_headroom_counts_subscription_and_reservation_together() {
        let capacity = OadrCapacityState {
            import_subscription_kw: Some(6.0),
            import_reservation_kw: Some(3.0),
            ..OadrCapacityState::default()
        };
        let req = ReservationRequest::from_headroom(&headroom(0.0, 11.0), &capacity);
        assert_eq!(req.import_kw, 2.0, "11 kW less a combined 9 kW allowance");
    }
}
