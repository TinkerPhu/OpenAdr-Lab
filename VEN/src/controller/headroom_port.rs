//! The site headroom and capacity-curve computations as the application and adapter rings see
//! them. They read the whole live asset roster (each asset's own maximum-effort answer), which
//! the flattened `SimSnapshot` cannot carry correctly for PV - see `simulator_port.rs` - so they
//! stay on the simulator and are reached through this port; `simulator::SimHandle` implements it.

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::entities::capacity_curve::CapacityCurves;
use crate::entities::plan::{Plan, SiteFlexibilityEnvelope};

#[async_trait]
pub trait HeadroomPort: Send + Sync {
    /// The site's flexibility envelope right now, within the physical import/export ratings.
    async fn site_headroom(
        &self,
        now: DateTime<Utc>,
        phys_import_kw: f64,
        phys_export_kw: f64,
    ) -> SiteFlexibilityEnvelope;

    /// The capacity curves for a commitment starting at `start` (snapped to a plan slot
    /// boundary), or `None` when the plan has no slot left to anchor them.
    async fn capacity_curves_at(
        &self,
        plan: &Plan,
        start: DateTime<Utc>,
        now: DateTime<Utc>,
        phys_import_kw: f64,
        phys_export_kw: f64,
    ) -> Option<CapacityCurves>;
}
