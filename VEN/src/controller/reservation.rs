use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::assets::AssetCapability;

/// Direction of flexibility constraint.
///
/// Up   = hold headroom for consumption reduction. Reduces max_import_kw.
/// Down = hold headroom for consumption increase.  Reduces max_export_kw toward zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlexDirection {
    Up,
    Down,
}

/// Source that created a reservation.
///
/// Note: OpenADR IMPORT/EXPORT_CAPACITY_LIMIT events do NOT produce Reservation
/// records. They are expressed through the Grid virtual asset's capability.
/// Only SIMPLE-type FIRM demand response events use VtnFirmEvent here.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum ReservationSource {
    /// VTN SIMPLE-type FIRM event: "reduce consumption by kw kW during window."
    VtnFirmEvent { event_id: String },
    /// FlexibilityPolicy scheduled window (Phase C).
    PolicySchedule { policy_id: String },
    /// FlexibilityPolicy default reserve (Phase C).
    PolicyDefault,
    /// User request (Phase F).
    UserRequest { request_id: Uuid },
}

/// A single capacity reservation. Time-windowed, asset-scoped or site-level.
///
/// `kw` is always a *reduction magnitude* (≥ 0) — how much headroom is held.
/// It is NOT a capacity ceiling. Direction determines which end of the capability
/// range is shrunk.
#[derive(Debug, Clone)]
pub struct Reservation {
    pub id: Uuid,
    pub window: (DateTime<Utc>, DateTime<Utc>),
    /// None = site-level (distributed proportionally across all assets).
    pub asset_id: Option<String>,
    /// Magnitude of reserved power. Always ≥ 0.
    pub kw: f64,
    pub direction: FlexDirection,
    pub source: ReservationSource,
    /// Lower = higher priority. 0 = hard constraint, 1 = FIRM event, 2+ = policy/user.
    pub priority: u8,
}

/// Per-asset reservation totals at a specific instant.
#[derive(Debug, Clone, Default)]
pub struct AssetReservation {
    /// Total kW locked for upward flexibility (consumption reduction). Always ≥ 0.
    pub reserved_up_kw: f64,
    /// Total kW locked for downward flexibility (consumption increase). Always ≥ 0.
    pub reserved_down_kw: f64,
}

pub struct ReservationLayer {
    reservations: Vec<Reservation>,
}

impl ReservationLayer {
    pub fn new() -> Self {
        Self {
            reservations: Vec::new(),
        }
    }

    /// Add a reservation.
    pub fn insert(&mut self, r: Reservation) {
        self.reservations.push(r);
    }

    /// Remove a reservation by id (e.g. when a VTN event is cancelled).
    pub fn remove(&mut self, id: Uuid) {
        self.reservations.retain(|r| r.id != id);
    }

    /// Sum of all reservations active at `t` for the given asset,
    /// including site-level reservations (asset_id == None).
    pub fn query_asset(&self, asset_id: &str, t: DateTime<Utc>) -> AssetReservation {
        let mut up = 0.0_f64;
        let mut down = 0.0_f64;
        for r in &self.reservations {
            let (ws, we) = r.window;
            if ws > t || t >= we {
                continue;
            }
            let applies =
                r.asset_id.is_none() || r.asset_id.as_deref() == Some(asset_id);
            if !applies {
                continue;
            }
            match r.direction {
                FlexDirection::Up => up += r.kw,
                FlexDirection::Down => down += r.kw,
            }
        }
        AssetReservation {
            reserved_up_kw: up,
            reserved_down_kw: down,
        }
    }

    /// Shrinks `phys_cap` by active reservations for `asset_id` at time `t`.
    ///
    /// Up   reservation: avail.max_import_kw = phys_cap.max_import_kw − reserved_up_kw
    ///                   (floored at phys_cap.max_export_kw — cannot go below export floor)
    /// Down reservation: avail.max_export_kw = phys_cap.max_export_kw + reserved_down_kw
    ///                   (capped at 0 — export floor stays ≤ 0)
    pub fn available_cap(
        &self,
        asset_id: &str,
        phys_cap: AssetCapability,
        t: DateTime<Utc>,
    ) -> AssetCapability {
        let res = self.query_asset(asset_id, t);
        AssetCapability {
            max_import_kw: (phys_cap.max_import_kw - res.reserved_up_kw)
                .max(phys_cap.max_export_kw),
            max_export_kw: (phys_cap.max_export_kw + res.reserved_down_kw).min(0.0),
        }
    }
}
