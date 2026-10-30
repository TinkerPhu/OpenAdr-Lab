//! Interval-schedule parsing for tariffs and the capacity-limit envelope.
//! Split out of `openadr_interface.rs` (2026-08-10) once that file crossed the
//! `VEN/src/` 500-production-line cap. Tests for both `parse_rate_snapshots`
//! and `parse_capacity_schedule` stay in `openadr_interface.rs`'s existing test
//! module (re-exported here via `pub use` at that file's top).
//!
//! Which event wins where events overlap is not decided here: `lab_core::event_priority::resolve`
//! is the one implementation (BL-02, GB-45), shared with the BFF's signal bands, so a band drawn
//! for an operator and the value applied here cannot disagree. This file only maps the winners of
//! each segment onto the VEN's snapshot types.
//!
//! GB-45: the result is non-overlapping and already priority-resolved, so every consumer —
//! tick-time cost, history sampler, planner tariff series, planned capacity limits — reads the
//! same value for the same instant without a resolution rule of its own.
//!
//! Every value is read through the `PayloadReader` (GB-50): a payload whose declared unit cannot
//! be honoured contributes nothing, exactly as if the event did not carry that type.

use crate::controller::vtn_port::OadrEvent;
use crate::entities::capacity::{CapacitySnapshot, CAPACITY_PAYLOAD_TYPES};
use crate::entities::tariff_snapshot::TariffSnapshot;
use lab_core::event_priority::{resolve, Segment};
use lab_core::wire_contract::PayloadReader;

fn winners_of(events: &[OadrEvent], reader: &PayloadReader, types: &[&str]) -> Vec<Segment> {
    resolve(events, reader, Some(types)).segments
}

/// Parse all rate snapshots from a slice of OpenADR events.
/// Handles PRICE, EXPORT_PRICE, GHG payload types per event interval.
/// Multiple payload types for the same interval are merged into one TariffSnapshot.
pub fn parse_rate_snapshots(events: &[OadrEvent], reader: &PayloadReader) -> Vec<TariffSnapshot> {
    winners_of(events, reader, &["PRICE", "EXPORT_PRICE", "GHG"])
        .into_iter()
        .filter_map(|segment| {
            let value = |t: &str| segment.winners.get(t).map(|w| w.value);
            let import_tariff_eur_kwh = value("PRICE");
            let export_tariff_eur_kwh = value("EXPORT_PRICE");
            let co2_g_kwh = value("GHG");
            if import_tariff_eur_kwh.is_none()
                && export_tariff_eur_kwh.is_none()
                && co2_g_kwh.is_none()
            {
                return None;
            }
            Some(TariffSnapshot {
                interval_start: segment.start,
                interval_end: segment.end,
                import_tariff_eur_kwh,
                export_tariff_eur_kwh,
                co2_g_kwh,
            })
        })
        .collect()
}

/// Parse the capacity schedule (Dynamic Operating Envelope and capacity
/// management, OpenADR 3.1 User Guide §8.10) from a slice of OpenADR events.
/// Handles the six `*_CAPACITY_LIMIT` / `*_SUBSCRIPTION` / `*_RESERVATION`
/// payload types (`entities::capacity::CAPACITY_PAYLOAD_TYPES`) per event interval,
/// each resolved by event priority on its own.
/// The single source for "which capacity value applies when" (GB-48): read it
/// through `entities::capacity::{tightest_capacity_limit, contracted_allowance_kw}`.
pub fn parse_capacity_schedule(
    events: &[OadrEvent],
    reader: &PayloadReader,
) -> Vec<CapacitySnapshot> {
    winners_of(events, reader, &CAPACITY_PAYLOAD_TYPES)
        .into_iter()
        .map(|segment| {
            let value = |t: &str| segment.winners.get(t).map(|w| w.value);
            let event_id = |t: &str| segment.winners.get(t).map(|w| w.event_id.clone());
            CapacitySnapshot {
                interval_start: segment.start,
                interval_end: segment.end,
                import_limit_kw: value("IMPORT_CAPACITY_LIMIT"),
                export_limit_kw: value("EXPORT_CAPACITY_LIMIT"),
                import_limit_event_id: event_id("IMPORT_CAPACITY_LIMIT"),
                export_limit_event_id: event_id("EXPORT_CAPACITY_LIMIT"),
                import_subscription_kw: value("IMPORT_CAPACITY_SUBSCRIPTION"),
                export_subscription_kw: value("EXPORT_CAPACITY_SUBSCRIPTION"),
                import_reservation_kw: value("IMPORT_CAPACITY_RESERVATION"),
                export_reservation_kw: value("EXPORT_CAPACITY_RESERVATION"),
            }
        })
        .collect()
}
