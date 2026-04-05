/// Stage 3 — HEMS Planning Algorithm (greedy per-step loop, Phase D).
///
/// Produces a Plan from TariffSnapshots + EnergyPackets + SimState.
/// Phase 6 (penalty check) is deferred to Stage 4.
use crate::assets::{AssetCapability, AssetConfig, AssetState};
use crate::controller::reservation::{AssetReservation, ReservationLayer};
use crate::entities::asset::{ComfortRate, PlanTrigger};
use crate::entities::capacity::OadrCapacityState;
use crate::entities::energy_packet::{DeadlineTier, EnergyPacket, PacketStatus, ValueCurve};
use crate::entities::plan::{
    FirmSummary, FlexibilityEnvelope, FlexibleSummary, LookaheadContext,
    PacketAllocation, Plan, PlanReason, PlanStep, PlanTimeSlot, PlanningHorizon,
    ReservationSource, SlotType,
};
use crate::simulator::SimState;

/// Running sum of already-committed setpoints at the current time step.
/// Built incrementally as each asset is resolved. Internal to CP2 loop.
#[derive(Debug, Clone, Default)]
pub struct SiteContext {
    pub planned_others_kw: f64,
    pub import_limit_kw: f64,
    pub export_limit_kw: f64,
    /// PV free-run forecast at this step (≤ 0, kW).
    pub pv_forecast_kw: f64,
}
use crate::entities::tariff_snapshot::TariffTimeSeries;
use crate::profile::{BatteryConfig, Profile};
use chrono::{DateTime, Duration, Utc};
use std::collections::HashMap;
use tracing::{debug, info};
use uuid::Uuid;

// ─── Constants ───────────────────────────────────────────────────────────────
const DEFAULT_IMPORT_PRICE: f64 = 0.20;
const DEFAULT_EXPORT_PRICE: f64 = 0.05;
const DEFAULT_CO2_G_KWH: f64 = 300.0;
const CO2_WEIGHT: f64 = 0.0001; // €/g ≈ €100/tonne

// ─── Public entry point ───────────────────────────────────────────────────────

/// Run the greedy per-step planning loop and return a new Plan + audit trail.
///
/// The caller assigns `plan.steps = steps` after the call.
pub fn run_planner(
    assets: &SimState,
    tariffs: &TariffTimeSeries,
    packets: &[EnergyPacket],
    capacity: &OadrCapacityState,
    reservations: &ReservationLayer,
    profile: &Profile,
    now: DateTime<Utc>,
    trigger: PlanTrigger,
) -> (Plan, Vec<PlanStep>) {
    let step_s = profile.planner.plan_step_s;
    let horizon_h = profile.planner.plan_horizon_h;
    let near_h = profile.planner.near_horizon_h;
    let lookahead_h = profile.planner.lookahead_h;
    let slot_h = step_s as f64 / 3600.0;
    let slot_dur = Duration::seconds(step_s as i64);

    let horizon_end = now + Duration::seconds((horizon_h * 3600) as i64);
    let firm_boundary = now + Duration::seconds((near_h * 3600) as i64);
    let total_steps = ((horizon_h * 3600) / step_s) as usize;

    let horizon = PlanningHorizon {
        start_time: now,
        end_time: horizon_end,
        step_size_s: step_s,
        num_steps: total_steps,
        near_horizon: firm_boundary,
        far_horizon: horizon_end,
    };

    // PV forecast for build_grid — built using the planner's own `now` so timestamps align
    // exactly with slot start times. forecast_kw_at returns positive generation magnitude.
    let pv_kw_map: HashMap<i64, f64> = assets
        .iter_assets()
        .find(|(e, _)| e.id == "pv")
        .and_then(|(_, cfg)| {
            if let AssetConfig::Pv(pv) = cfg {
                // Convert per-tick EMA alpha to continuous time constant in seconds.
                // tau_s = -step_s / ln(1 - alpha), so at each plan step boundary the
                // exponential gives the same value as (1-alpha)^step_index but in real time.
                let tau_s: f64 = if pv.pv_alpha >= 1.0 {
                    0.0           // instant decay
                } else if pv.pv_alpha <= 0.0 {
                    f64::INFINITY // no decay
                } else {
                    -(step_s as f64) / (1.0 - pv.pv_alpha).ln()
                };
                let map_ts0 = now.timestamp();
                let map_ts1 = (now + Duration::seconds(step_s as i64)).timestamp();
                info!(
                    irradiance_offset = pv.irradiance_offset,
                    pv_alpha = pv.pv_alpha,
                    tau_s,
                    rated_kw = pv.rated_kw,
                    map_ts0,
                    map_ts1,
                    "pv forecast build"
                );
                Some(
                    (0..total_steps)
                        .map(|i| {
                            let t = now + Duration::seconds((i as i64) * step_s as i64);
                            let seconds_ahead = (i as f64) * step_s as f64;
                            (t.timestamp(), pv.forecast_kw_at(t, seconds_ahead, tau_s))
                        })
                        .collect(),
                )
            } else {
                None
            }
        })
        .unwrap_or_default();

    // Phase 1: Build planning grid.
    // Read current effective base_load from the sim (reflects any inject override).
    let baseline_kw = assets
        .find_asset("base_load")
        .and_then(|(_, cfg)| {
            if let AssetConfig::BaseLoad(bl) = cfg {
                Some(bl.baseline_kw)
            } else {
                None
            }
        })
        .unwrap_or_else(|| profile.base_load_kw());
    let (mut firm_slots, flexible_slots) =
        build_grid(tariffs, capacity, baseline_kw, now, step_s, total_steps, firm_boundary, &pv_kw_map);

    // Preserve terminal packets; work on non-terminal only
    let terminal_pkts: Vec<EnergyPacket> =
        packets.iter().filter(|p| p.is_terminal()).cloned().collect();
    let mut pkts: Vec<EnergyPacket> =
        packets.iter().filter(|p| !p.is_terminal()).cloned().collect();

    // Median import tariff for battery arbitrage threshold.
    // Uses the full 24h horizon (firm + flexible) so the cheap/expensive thresholds
    // reflect the complete daily price cycle rather than the 4h near-horizon window alone.
    // This prevents the battery from charging to 100% on flat nighttime rates when
    // daytime prices (and free PV) are coming within the planning horizon.
    let median_tariff = {
        let mut prices: Vec<f64> = firm_slots.iter()
            .chain(flexible_slots.iter())
            .map(|s| s.import_tariff_eur_kwh)
            .collect();
        prices.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        prices.get(prices.len() / 2).copied().unwrap_or(DEFAULT_IMPORT_PRICE)
    };

    {
        // pv_kw_map covers the full horizon (24h); firm slots only cover near_horizon_h.
        // Report PV and surplus stats scoped to the firm slots for accurate diagnostics.
        let firm_pv_sum: f64 = firm_slots.iter()
            .map(|s| pv_kw_map.get(&s.start.timestamp()).copied().unwrap_or(0.0))
            .sum();
        let firm_pv_peak: f64 = firm_slots.iter()
            .map(|s| pv_kw_map.get(&s.start.timestamp()).copied().unwrap_or(0.0))
            .fold(0.0_f64, f64::max);
        let pv_map_hits = firm_slots.iter()
            .filter(|s| pv_kw_map.contains_key(&s.start.timestamp()))
            .count();
        let slot0_ts = firm_slots.first().map(|s| s.start.timestamp()).unwrap_or(0);
        info!(pv_map_hits, total_firm = firm_slots.len(), slot0_ts, pv_map_size = pv_kw_map.len(), "pv map lookup");
        let surplus_sum: f64 = firm_slots.iter().map(|s| s.surplus_available_kw).sum();
        let bat_cfg = profile.battery_config();
        let eff_sqrt = bat_cfg.map(|b| b.round_trip_efficiency.sqrt()).unwrap_or(1.0);
        info!(
            median_tariff,
            tariff_min = firm_slots.iter().map(|s| s.import_tariff_eur_kwh).fold(f64::MAX, f64::min),
            tariff_max = firm_slots.iter().map(|s| s.import_tariff_eur_kwh).fold(f64::MIN, f64::max),
            cheap_threshold = median_tariff * eff_sqrt,
            expensive_threshold = median_tariff / eff_sqrt,
            firm_pv_sum_kw = firm_pv_sum,
            firm_pv_peak_kw = firm_pv_peak,
            surplus_total_kw = surplus_sum,
            firm_slots = firm_slots.len(),
            active_packets = pkts.len(),
            "planner: cycle inputs"
        );
    }

    // Pre-loop: lookahead context per asset
    let lookahead_window = Duration::seconds((lookahead_h * 3600.0) as i64);
    let lookaheads = precompute_lookahead(assets, tariffs, now, lookahead_window, slot_dur);

    // Per-step mutable asset states (start from current SimState)
    let mut asset_states: HashMap<String, AssetState> =
        assets.iter_assets().map(|(e, _)| (e.id.clone(), e.state.clone())).collect();

    // Per-plan allocated energy per packet (tracks total across all slots)
    let mut allocated: HashMap<Uuid, f64> = pkts.iter().map(|p| (p.id, 0.0_f64)).collect();
    let mut plan_steps: Vec<PlanStep> = Vec::new();

    // Asset processing order per spec §3.4: uncontrollable first, controllable last
    let asset_order: &[&str] = &["pv", "base_load", "ev", "battery", "heater"];
    let uncontrollable: &[&str] = &["pv", "base_load"];

    // Pre-extract slot windows for deadline-pressure calculation in rules_choose().
    // Cannot pass &firm_slots into the iter_mut loop (simultaneous mut + immutable borrow).
    let firm_slot_windows: Vec<(DateTime<Utc>, DateTime<Utc>)> =
        firm_slots.iter().map(|s| (s.start, s.end)).collect();

    for slot in firm_slots.iter_mut() {
        let ts = slot.start;

        let mut site_ctx = SiteContext {
            planned_others_kw: 0.0,
            import_limit_kw: slot.import_cap_kw,
            export_limit_kw: slot.export_cap_kw,
            pv_forecast_kw: 0.0,
        };

        for &aid in asset_order {
            let (state, cfg) = match (asset_states.get(aid), assets.iter_assets().find(|(e, _)| e.id == aid)) {
                (Some(s), Some((_, c))) => (s.clone(), c),
                _ => continue,
            };

            let phys_cap = cfg.capability(&state);
            let avail_cap = reservations.available_cap(aid, phys_cap, ts);
            let res = reservations.query_asset(aid, ts);

            let (setpoint_kw, reason) = if uncontrollable.contains(&aid) {
                // Uncontrollable: free-run power, no decision
                let (_, power_kw) = cfg.step(&state, 0.0, slot_dur);
                (power_kw, PlanReason::Idle)
            } else {
                let la = match lookaheads.get(aid) {
                    Some(l) => l,
                    None => continue,
                };
                let soc_ceiling_pct = match (&state, cfg) {
                    (AssetState::Ev(_), AssetConfig::Ev(e)) => e.soc_target * 100.0,
                    _ => 100.0,
                };
                rules_choose(
                    aid, phys_cap, avail_cap, &res,
                    slot.import_tariff_eur_kwh, slot, &firm_slot_windows, &pkts,
                    &allocated, &site_ctx, la, reservations,
                    median_tariff, profile.battery_config(), slot_h, now,
                    soc_ceiling_pct,
                )
            };

            let (next_state, actual_kw) = cfg.step(&state, setpoint_kw, slot_dur);

            if aid == "pv" {
                // pv_forecast_kw is already set by build_grid from pv_kw_map (sin model).
                // Propagate to site_ctx so rules_choose sees it for surplus-aware decisions.
                site_ctx.pv_forecast_kw = actual_kw;
            }

            // Per-slot debug logging for battery (first slot only to avoid 48-line spam).
            if aid == "battery" && slot.slot_index == 0 {
                debug!(
                    slot = slot.slot_index,
                    reason = ?reason,
                    setpoint_kw,
                    phys_import = phys_cap.max_import_kw,
                    phys_export = phys_cap.max_export_kw,
                    avail_import = avail_cap.max_import_kw,
                    avail_export = avail_cap.max_export_kw,
                    surplus_kw = slot.surplus_available_kw,
                    tariff = slot.import_tariff_eur_kwh,
                    median_tariff,
                    reserved_up = res.reserved_up_kw,
                    reserved_down = res.reserved_down_kw,
                    planned_others_kw = site_ctx.planned_others_kw,
                    "battery[slot=0] decision"
                );
            }

            plan_steps.push(PlanStep {
                ts,
                asset_id: aid.to_string(),
                state_before: state,
                capability: phys_cap,
                reserved_up_kw: res.reserved_up_kw,
                reserved_down_kw: res.reserved_down_kw,
                avail_max_export_kw: avail_cap.max_export_kw,
                avail_max_import_kw: avail_cap.max_import_kw,
                setpoint_kw,
                actual_power_kw: actual_kw,
                reason,
            });

            // PV and base_load are non-controllable assets already accounted for in
            // build_grid (via pv_kw_map and baseline_kw). Calling update_slot_from_step
            // for either would double-subtract from net_import_kw / net_export_kw.
            // base_load allocations are added by build_grid for all slots (firm + flex)
            // so the asset timeline is populated without any dispatch-loop involvement.
            if aid != "pv" && aid != "base_load" {
                update_slot_from_step(slot, aid, actual_kw, &pkts, &mut allocated, slot_h);
            }
            site_ctx.planned_others_kw += actual_kw;
            asset_states.insert(aid.to_string(), next_state);
        }
    }

    // Post-loop: log battery reason distribution for diagnostics.
    {
        let bat_steps: Vec<_> = plan_steps.iter().filter(|s| s.asset_id == "battery").collect();
        if !bat_steps.is_empty() {
            let non_idle: Vec<_> = bat_steps.iter()
                .filter(|s| !matches!(s.reason, PlanReason::Idle))
                .collect();
            if non_idle.is_empty() {
                debug!(count = bat_steps.len(), "battery: all steps IDLE");
            } else {
                let reasons: Vec<String> = non_idle.iter()
                    .map(|s| format!("slot{}={:?}", s.ts.timestamp() % 10000, s.reason))
                    .take(5)
                    .collect();
                info!(
                    total = bat_steps.len(),
                    non_idle = non_idle.len(),
                    samples = %reasons.join(", "),
                    "battery: non-IDLE steps detected"
                );
            }
        }
    }

    // Transition Pending → Scheduled for any packet that got energy booked
    for p in pkts.iter_mut() {
        let booked = *allocated.get(&p.id).unwrap_or(&0.0);
        if booked > 0.0 && p.status == PacketStatus::Pending {
            p.status = PacketStatus::Scheduled;
        }
    }

    // Phase 7: Flexibility envelopes (unchanged)
    let envelopes = build_envelopes(&pkts, &flexible_slots, &firm_slots, slot_h);

    // Phase 8: Finalize (unchanged)
    finalize_packets(&mut pkts, &firm_slots, slot_h, now);
    update_slot_flexibility(&mut firm_slots);

    let firm_summary = summarize_firm(&firm_slots, slot_h);
    let flexible_summary = FlexibleSummary {
        total_energy_kwh: envelopes.iter().map(|e| e.energy_needed_kwh).sum(),
        estimated_cost_eur: envelopes.iter().map(|e| e.estimated_cost_eur).sum(),
        estimated_co2_g: envelopes.iter().map(|e| e.estimated_co2_g).sum(),
    };

    pkts.extend(terminal_pkts);

    let plan = Plan {
        id: Uuid::new_v4(),
        created_at: now,
        trigger,
        horizon,
        firm_boundary,
        firm_slots,
        firm_summary,
        flexible_slots,
        envelopes,
        flexible_summary,
        packets: pkts,
        warnings: vec![],
        steps: vec![], // caller assigns plan.steps = plan_steps
    };

    (plan, plan_steps)
}

// ─── Phase 1: Build planning grid ────────────────────────────────────────────

fn build_grid(
    tariffs: &TariffTimeSeries,
    capacity: &OadrCapacityState,
    baseline_kw: f64,
    now: DateTime<Utc>,
    step_s: u64,
    total_steps: usize,
    firm_boundary: DateTime<Utc>,
    pv_kw_map: &HashMap<i64, f64>,
) -> (Vec<PlanTimeSlot>, Vec<PlanTimeSlot>) {
    let import_cap = capacity.import_limit_kw.unwrap_or(f64::MAX);
    let export_cap = capacity.export_limit_kw.unwrap_or(f64::MAX);
    let slot_h = step_s as f64 / 3600.0;
    let rates_empty = tariffs.is_empty();

    let mut firm = Vec::new();
    let mut flex = Vec::new();

    for i in 0..total_steps {
        let start = now + Duration::seconds((i as i64) * step_s as i64);
        let end = start + Duration::seconds(step_s as i64);
        let epoch = start.timestamp();

        // Use Step LOCF interpolation so tariff lookups work regardless of whether slot
        // timestamps align with the event intervalPeriod grid boundaries.
        let import_tariff = tariffs
            .import_eur_kwh
            .interpolate_at(start)
            .unwrap_or(DEFAULT_IMPORT_PRICE);
        let export_tariff = tariffs
            .export_eur_kwh
            .interpolate_at(start)
            .unwrap_or(DEFAULT_EXPORT_PRICE);
        let co2 = tariffs
            .co2_g_kwh
            .interpolate_at(start)
            .unwrap_or(DEFAULT_CO2_G_KWH);
        let grid_eff = import_tariff + co2 * CO2_WEIGHT;

        // PV forecast from capability_trajectory (already in generation-magnitude convention, ≥ 0)
        let pv_kw = pv_kw_map.get(&epoch).copied().unwrap_or(0.0);
        let net = baseline_kw - pv_kw; // positive = need to import, negative = surplus
        let surplus = (-net).max(0.0);
        let net_import = net.max(0.0);
        let net_export = surplus;

        let slot_type = if end <= firm_boundary {
            SlotType::Firm
        } else {
            SlotType::Flexible
        };

        // B1 fix: import_cap_kw is the raw OadrCapacityState value.
        // FIRM reservation effect lives in per-step available_cap() call in rules_choose().
        let slot = PlanTimeSlot {
            slot_index: i,
            start,
            end,
            slot_type,
            import_tariff_eur_kwh: import_tariff,
            export_tariff_eur_kwh: export_tariff,
            co2_g_kwh: co2,
            grid_effective_cost: grid_eff,
            rate_estimated: rates_empty,
            import_cap_kw: import_cap,
            export_cap_kw: export_cap,
            baseline_kw,
            pv_forecast_kw: pv_kw,
            surplus_available_kw: surplus,
            // base_load allocation: pre-populated here (not via update_slot_from_step)
            // so it appears in both firm and flexible slot timelines without double-
            // counting the net flow impact already captured in net_import_kw/net_export_kw.
            allocations: vec![PacketAllocation {
                packet_id: Uuid::nil(),
                asset_id: "base_load".to_string(),
                power_kw: baseline_kw,
                surplus_power_kw: 0.0,
                grid_power_kw: baseline_kw,
                marginal_value: 0.0,
                cost_eur: alloc_cost_eur(0.0, baseline_kw, export_tariff, import_tariff, slot_h),
                co2_g: baseline_kw * co2 * slot_h,
            }],
            net_import_kw: net_import,
            net_export_kw: net_export,
            import_flexibility_kw: 0.0,
            export_flexibility_kw: 0.0,
        };

        if slot.slot_type == SlotType::Firm {
            firm.push(slot);
        } else {
            flex.push(slot);
        }
    }

    (firm, flex)
}

// ─── Lookahead precomputation ─────────────────────────────────────────────────

fn precompute_lookahead(
    sim: &SimState,
    tariffs: &TariffTimeSeries,
    now: DateTime<Utc>,
    lookahead_window: Duration,
    resolution: Duration,
) -> HashMap<String, LookaheadContext> {
    let n = (lookahead_window.num_seconds() / resolution.num_seconds().max(1)) as usize;

    let mut result = HashMap::new();
    for (entry, cfg) in sim.iter_assets() {
        let traj = cfg.capability_trajectory(&entry.state, lookahead_window, resolution);

        let mut tariff_min = f64::MAX;
        let mut tariff_max = f64::MIN;
        for i in 0..n {
            let t = now + resolution * i as i32;
            let v = tariffs
                .import_eur_kwh
                .interpolate_at(t)
                .unwrap_or(DEFAULT_IMPORT_PRICE);
            tariff_min = tariff_min.min(v);
            tariff_max = tariff_max.max(v);
        }
        if tariff_min == f64::MAX {
            tariff_min = DEFAULT_IMPORT_PRICE;
        }
        if tariff_max == f64::MIN {
            tariff_max = DEFAULT_IMPORT_PRICE;
        }

        let ceiling_eta = traj.iter()
            .find(|(_, cap)| cap.max_import_kw < 1e-3)
            .map(|(ts, _)| *ts);
        let floor_eta = traj.iter()
            .find(|(_, cap)| cap.max_export_kw > -1e-3)
            .map(|(ts, _)| *ts);

        result.insert(entry.id.clone(), LookaheadContext {
            capability_trajectory: traj,
            tariff_min_ahead_eur_per_kwh: tariff_min,
            tariff_max_ahead_eur_per_kwh: tariff_max,
            ceiling_eta,
            floor_eta,
        });
    }
    result
}

// ─── Rules engine ─────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn rules_choose(
    asset_id: &str,
    phys_cap: AssetCapability,
    avail_cap: AssetCapability,
    res: &AssetReservation,
    tariff_t: f64,
    slot: &PlanTimeSlot,
    firm_slot_windows: &[(DateTime<Utc>, DateTime<Utc>)],
    packets: &[EnergyPacket],
    allocated: &HashMap<Uuid, f64>,
    site_ctx: &SiteContext,
    _lookahead: &LookaheadContext,
    reservations: &ReservationLayer,
    median_tariff: f64,
    battery_cfg: Option<&BatteryConfig>,
    slot_h: f64,
    now: DateTime<Utc>,
    soc_ceiling_pct: f64,
) -> (f64, PlanReason) {
    // Rule 4a: SoC ceiling — physics itself prevents import AND export is negligible.
    // Must come before Rule 1 so that EV-at-soc-target emits SocCeiling, not FirmObligation.
    // Condition: phys_cap (not avail_cap) prevents import, and the asset has no meaningful
    // export capability either. Reservations cannot grant capability that physics denies.
    if phys_cap.max_import_kw < 1e-6 && phys_cap.max_export_kw > -1e-3 {
        return (0.0, PlanReason::SocCeiling { soc_pct: soc_ceiling_pct });
    }

    // Rule 1: reservation blocks all headroom
    if avail_cap.max_import_kw <= 1e-6 && avail_cap.max_export_kw >= -1e-6 {
        let source = reservations.primary_source(asset_id, slot.start);
        let required_kw = res.reserved_up_kw.max(res.reserved_down_kw);
        return (0.0, PlanReason::FirmObligation { source, required_kw });
    }

    // Rule 4: SoC/comfort ceiling — no import headroom AND no discharge headroom either.
    // When export is still available (e.g. battery full but dischargeable), fall through
    // to arbitrage rules so Rule 10 can discharge at expensive tariff.
    if avail_cap.max_import_kw < 1e-6 && avail_cap.max_export_kw > -1e-6 {
        return (0.0, PlanReason::SocCeiling { soc_pct: soc_ceiling_pct });
    }

    // Rule 5: SoC/comfort floor (no export headroom, but asset can generate)
    if avail_cap.max_export_kw > -1e-6 && phys_cap.max_export_kw < -1e-3 {
        return (0.0, PlanReason::SocFloor { soc_pct: 0.0 });
    }

    // Rules 6+7: best active packet for this asset
    let far_future = now + Duration::days(365);
    let best = packets
        .iter()
        .filter(|p| p.asset_id == asset_id && !p.is_terminal())
        .filter_map(|p| {
            let already = *allocated.get(&p.id).unwrap_or(&0.0);
            let undelivered = (p.undelivered_energy_kwh() - already).max(0.0);
            if undelivered <= 1e-6 {
                return None;
            }
            // Budget gate (§8.4): skip packet if accumulated cost already reached the ceiling.
            if let Some(budget) = p.value_curve.deadline_tiers.first().and_then(|t| t.max_total_cost_eur) {
                if p.accumulated_cost_eur >= budget {
                    return None;
                }
            }
            let deadline_with_tolerance = p.latest_end().unwrap_or(far_future)
                + Duration::seconds(p.tolerance_min.unwrap_or(0) * 60);
            let slots_remaining = firm_slot_windows
                .iter()
                .filter(|(start, end)| *start >= p.earliest_start && *end <= deadline_with_tolerance)
                .count()
                .max(1);
            let slots_needed =
                (undelivered / p.desired_power_kw.max(1e-9) / slot_h).ceil() as usize;
            let time_pressure = (slots_needed as f64 / slots_remaining as f64).clamp(1.0, 3.0);

            let fill = p.fill() + already / p.target_energy_kwh.max(1e-9);
            let comfort_bid = p.value_curve.bid_at(fill.min(1.0));

            let surplus_frac =
                (slot.surplus_available_kw / p.desired_power_kw.max(1e-9)).min(1.0);
            let eff_cost = tariff_t * (1.0 - surplus_frac)
                + slot.export_tariff_eur_kwh * surplus_frac;

            let eligible = comfort_bid >= eff_cost || time_pressure >= 2.0;
            if !eligible {
                return None;
            }
            Some((p, comfort_bid, time_pressure))
        })
        .max_by(|a, b| {
            (a.1 * a.2)
                .partial_cmp(&(b.1 * b.2))
                .unwrap_or(std::cmp::Ordering::Equal)
        });

    if let Some((packet, comfort_bid, time_pressure)) = best {
        let import_head = (slot.import_cap_kw - slot.net_import_kw).max(0.0);
        let desired = packet
            .desired_power_kw
            .min(import_head + slot.surplus_available_kw);
        let desired = desired.clamp(avail_cap.max_export_kw, avail_cap.max_import_kw);
        // Rule 2: clamp by remaining site import headroom
        let site_head = (site_ctx.import_limit_kw - site_ctx.planned_others_kw).max(0.0);
        let setpoint = desired.min(site_head).min(avail_cap.max_import_kw);

        if setpoint > 1e-6 {
            if time_pressure >= 2.0 {
                // Rule 7: deadline pressure — treat as firm obligation
                return (
                    setpoint,
                    PlanReason::FirmObligation {
                        source: ReservationSource::UserRequest { request_id: packet.id },
                        required_kw: setpoint,
                    },
                );
            } else {
                // Rule 6: cheap enough / comfort-bid eligible
                return (
                    setpoint,
                    PlanReason::CheapTariff {
                        tariff_eur_per_kwh: tariff_t,
                        threshold_eur_per_kwh: comfort_bid,
                    },
                );
            }
        }
    }

    // Rule 8 (surplus opportunity) is intentionally absent from the planner.
    // Surplus EV charging is handled by the dispatcher as a live opportunistic
    // overlay (current tick only, using actual PV power). This prevents phantom
    // plan commitments based on model-forecast PV from polluting VTN reports.

    // Rule 8b: Battery absorbs PV surplus (self-consumption priority).
    // Fires for the battery whenever PV generates more than the other site loads,
    // regardless of tariff. Fills the battery with free solar rather than exporting
    // it at the low export tariff. Unlike Rule 8 (absent for EV), battery surplus
    // absorption IS planned because battery setpoints do not create VTN report entries.
    if asset_id == "battery" && slot.surplus_available_kw > 0.1 {
        let site_head = (site_ctx.import_limit_kw - site_ctx.planned_others_kw).max(0.0);
        let charge_kw = avail_cap
            .max_import_kw
            .min(slot.surplus_available_kw)
            .min(site_head)
            .max(0.0);
        if charge_kw > 0.01 {
            return (
                charge_kw,
                PlanReason::SurplusAbsorption { surplus_kw: slot.surplus_available_kw },
            );
        }
    }

    // Rules 9+10: battery arbitrage
    if asset_id == "battery" {
        if let Some(bat) = battery_cfg {
            let eff = bat.round_trip_efficiency.sqrt();
            if tariff_t < median_tariff * eff {
                // Rule 9: cheap tariff — charge at full available rate from grid.
                // PV-surplus case is handled by Rule 8b above; if we reach here
                // surplus_available_kw ≤ 0.1 kW (night arbitrage or flat tariff window).
                let site_head = (site_ctx.import_limit_kw - site_ctx.planned_others_kw).max(0.0);
                let charge_kw = avail_cap.max_import_kw.min(site_head).max(0.0);
                if charge_kw > 0.01 {
                    return (
                        charge_kw,
                        PlanReason::CheapTariff {
                            tariff_eur_per_kwh: tariff_t,
                            threshold_eur_per_kwh: median_tariff * eff,
                        },
                    );
                }
            } else if tariff_t > median_tariff / eff {
                // Rule 10: expensive tariff — discharge to offset site import only.
                // Guard: planned_others_kw is the net grid draw from all other assets
                // (PV, base_load, EV). If the site is already in surplus (PV covers
                // load), discharging would only add to grid export at the low export
                // tariff — depleting SoC for no gain.
                let site_import_kw = site_ctx.planned_others_kw.max(0.0);
                let discharge_kw = (-avail_cap.max_export_kw).max(0.0).min(site_import_kw);
                if discharge_kw > 0.01 {
                    return (
                        -discharge_kw,
                        PlanReason::ExpensiveTariff {
                            tariff_eur_per_kwh: tariff_t,
                            threshold_eur_per_kwh: median_tariff / eff,
                        },
                    );
                }
            }
        }
    }

    // Rule 12: idle
    (0.0, PlanReason::Idle)
}

// ─── Cost helpers ────────────────────────────────────────────────────────────

/// EUR cost for one plan-slot allocation, split between PV-surplus and grid portions.
/// `surplus_kw` is the PV-covered share (opportunity cost at export tariff).
/// `grid_kw`    is the grid-import share (actual cost at import tariff).
/// Pass `grid_kw < 0` for discharge/export to get negative cost (revenue).
fn alloc_cost_eur(
    surplus_kw: f64,
    grid_kw: f64,
    export_tariff_eur_kwh: f64,
    import_tariff_eur_kwh: f64,
    slot_h: f64,
) -> f64 {
    surplus_kw * export_tariff_eur_kwh * slot_h + grid_kw * import_tariff_eur_kwh * slot_h
}

// ─── Slot bookkeeping helper ──────────────────────────────────────────────────

fn update_slot_from_step(
    slot: &mut PlanTimeSlot,
    asset_id: &str,
    actual_kw: f64,
    packets: &[EnergyPacket],
    allocated: &mut HashMap<Uuid, f64>,
    slot_h: f64,
) {
    if actual_kw > 1e-6 {
        // Import: find the active packet for this asset
        if let Some(packet) =
            packets.iter().find(|p| p.asset_id == asset_id && !p.is_terminal())
        {
            let surplus_used = slot.surplus_available_kw.min(actual_kw);
            let grid_used = (actual_kw - surplus_used).max(0.0);
            let energy_kwh = actual_kw * slot_h;
            let cost = alloc_cost_eur(surplus_used, grid_used, slot.export_tariff_eur_kwh, slot.import_tariff_eur_kwh, slot_h);
            let co2 = grid_used * slot.co2_g_kwh * slot_h;

            slot.surplus_available_kw -= surplus_used;
            slot.net_import_kw += grid_used;
            slot.net_export_kw = (slot.net_export_kw - surplus_used).max(0.0);
            *allocated.entry(packet.id).or_insert(0.0) += energy_kwh;

            slot.allocations.push(PacketAllocation {
                packet_id: packet.id,
                asset_id: asset_id.to_string(),
                power_kw: actual_kw,
                surplus_power_kw: surplus_used,
                grid_power_kw: grid_used,
                marginal_value: 0.0,
                cost_eur: cost,
                co2_g: co2,
            });
        } else {
            // Battery arbitrage (no packet)
            let surplus_used = slot.surplus_available_kw.min(actual_kw);
            let grid_used = (actual_kw - surplus_used).max(0.0);
            slot.surplus_available_kw -= surplus_used;
            slot.net_import_kw += grid_used;
            slot.net_export_kw = (slot.net_export_kw - surplus_used).max(0.0);

            slot.allocations.push(PacketAllocation {
                packet_id: Uuid::nil(),
                asset_id: asset_id.to_string(),
                power_kw: actual_kw,
                surplus_power_kw: surplus_used,
                grid_power_kw: grid_used,
                marginal_value: 0.0,
                cost_eur: alloc_cost_eur(0.0, grid_used, slot.export_tariff_eur_kwh, slot.import_tariff_eur_kwh, slot_h),
                co2_g: grid_used * slot.co2_g_kwh * slot_h,
            });
        }
    } else if actual_kw < -1e-6 {
        // Export / discharge: first offset any remaining import, then overflow into export.
        let discharge_kw = -actual_kw;
        let import_offset = discharge_kw.min(slot.net_import_kw);
        let export_surplus = discharge_kw - import_offset;
        slot.net_import_kw = (slot.net_import_kw - import_offset).max(0.0);
        slot.net_export_kw += export_surplus;

        slot.allocations.push(PacketAllocation {
            packet_id: Uuid::nil(),
            asset_id: asset_id.to_string(),
            power_kw: actual_kw,
            surplus_power_kw: 0.0,
            grid_power_kw: actual_kw,
            marginal_value: 0.0,
            cost_eur: -discharge_kw * slot.import_tariff_eur_kwh * slot_h,
            co2_g: 0.0,
        });
    }
}

// ─── Phase 7: Flexibility Envelopes ──────────────────────────────────────────

fn build_envelopes(
    packets: &[EnergyPacket],
    flexible_slots: &[PlanTimeSlot],
    firm_slots: &[PlanTimeSlot],
    slot_h: f64,
) -> Vec<FlexibilityEnvelope> {
    let far_future = Utc::now() + Duration::days(365);
    let mut envs = Vec::new();

    for packet in packets {
        if packet.is_terminal() {
            continue;
        }

        // Energy already allocated to this packet in FIRM slots
        let firm_kwh: f64 = firm_slots
            .iter()
            .flat_map(|s| s.allocations.iter())
            .filter(|a| a.packet_id == packet.id)
            .map(|a| a.power_kw * slot_h)
            .sum();

        let energy_for_flex = (packet.undelivered_energy_kwh() - firm_kwh).max(0.0);
        if energy_for_flex < 1e-3 {
            continue;
        }

        let latest_end = packet.latest_end().unwrap_or(far_future);

        let eligible: Vec<&PlanTimeSlot> = flexible_slots
            .iter()
            .filter(|s| s.start < latest_end)
            .collect();

        if eligible.is_empty() {
            continue;
        }

        let n = eligible.len();
        let window_start = eligible[0].start;
        let window_end = eligible[n - 1].end;

        let avg_import = eligible
            .iter()
            .map(|s| s.import_tariff_eur_kwh)
            .sum::<f64>()
            / n as f64;
        let avg_co2 = eligible.iter().map(|s| s.co2_g_kwh).sum::<f64>() / n as f64;

        let fill_now = packet.fill();
        let fill_after = (fill_now + energy_for_flex / packet.target_energy_kwh.max(1e-9)).min(1.0);

        envs.push(FlexibilityEnvelope {
            packet_id: packet.id,
            asset_id: packet.asset_id.clone(),
            energy_needed_kwh: energy_for_flex,
            power_min_kw: packet.desired_power_kw * 0.5,
            power_max_kw: packet.desired_power_kw,
            window_start,
            window_end,
            slots_available: n,
            max_acceptable_rate: packet.value_curve.bid_at(fill_now),
            min_acceptable_rate: packet.value_curve.bid_at(fill_after),
            budget_remaining_eur: packet.value_curve
                .deadline_tiers
                .first()
                .and_then(|t| t.max_total_cost_eur)
                .map(|b| (b - packet.accumulated_cost_eur).max(0.0))
                .unwrap_or(f64::MAX),
            estimated_cost_eur: energy_for_flex * avg_import,
            estimated_co2_g: energy_for_flex * avg_co2,
        });
    }

    envs
}

// ─── Phase 8: Finalize ────────────────────────────────────────────────────────

fn finalize_packets(
    packets: &mut Vec<EnergyPacket>,
    firm_slots: &[PlanTimeSlot],
    slot_h: f64,
    now: DateTime<Utc>,
) {
    for packet in packets.iter_mut() {
        let mut allocated_kwh = 0.0_f64;
        let mut cost = 0.0_f64;
        let mut co2 = 0.0_f64;

        for alloc in firm_slots.iter().flat_map(|s| s.allocations.iter()) {
            if alloc.packet_id == packet.id {
                allocated_kwh += alloc.power_kw * slot_h;
                cost += alloc.cost_eur;
                co2 += alloc.co2_g;
            }
        }

        packet.estimated_cost_eur = cost;
        packet.estimated_co2_g = co2;
        packet.estimated_completion = ((packet.past_energy_kwh() + allocated_kwh)
            / packet.target_energy_kwh.max(1e-9))
        .min(1.0);
        packet.last_estimate_at = Some(now);
    }
}

fn update_slot_flexibility(slots: &mut Vec<PlanTimeSlot>) {
    for slot in slots.iter_mut() {
        slot.import_flexibility_kw = (slot.import_cap_kw - slot.net_import_kw).max(0.0);
        slot.export_flexibility_kw = (slot.export_cap_kw - slot.net_export_kw).max(0.0);
    }
}

fn summarize_firm(slots: &[PlanTimeSlot], slot_h: f64) -> FirmSummary {
    let mut s = FirmSummary::default();
    for slot in slots {
        s.total_import_kwh += slot.net_import_kw * slot_h;
        s.total_export_kwh += slot.net_export_kw * slot_h;
        for a in &slot.allocations {
            s.total_cost_eur += a.cost_eur;
            s.total_co2_g += a.co2_g;
        }
    }
    s
}

// ─── Packet seeding from profile ─────────────────────────────────────────────

/// Create EnergyPackets from profile seed entries at VEN startup.
pub fn seed_packets_from_profile(profile: &Profile, now: DateTime<Utc>) -> Vec<EnergyPacket> {
    profile
        .packets
        .iter()
        .map(|seed| seed_to_packet(seed, profile, now))
        .collect()
}

fn seed_to_packet(
    seed: &crate::profile::PacketSeed,
    profile: &Profile,
    now: DateTime<Utc>,
) -> EnergyPacket {
    let asset_id = seed.asset.clone();

    let (target_energy_kwh, desired_power_kw) = match asset_id.as_str() {
        "ev" => {
            if let Some(ev) = profile.ev_config() {
                let target_soc = seed.target_soc.unwrap_or(ev.soc_target);
                let energy = (target_soc - ev.initial_soc).max(0.0) * ev.battery_kwh;
                let power = seed.desired_power_kw.unwrap_or(ev.max_charge_kw);
                (energy, power)
            } else {
                (0.0, 0.0)
            }
        }
        "battery" => {
            if let Some(bat) = profile.battery_config() {
                let target_soc = seed.target_soc.unwrap_or(1.0);
                let energy = (target_soc - bat.initial_soc).max(0.0) * bat.capacity_kwh;
                let power = seed.desired_power_kw.unwrap_or(bat.max_charge_kw);
                (energy, power)
            } else {
                (0.0, 0.0)
            }
        }
        _ => {
            let power = seed.desired_power_kw.unwrap_or(1.0);
            (power * seed.latest_end_h * 0.5, power)
        }
    };

    let latest_end = now + Duration::seconds((seed.latest_end_h * 3600.0) as i64);

    let comfort_rates: Vec<ComfortRate> = if seed.comfort_rates.is_empty() {
        vec![
            ComfortRate {
                fill: 0.0,
                max_marginal_price: 0.35,
                max_marginal_co2: 0.0,
            },
            ComfortRate {
                fill: 1.0,
                max_marginal_price: 0.05,
                max_marginal_co2: 0.0,
            },
        ]
    } else {
        seed.comfort_rates
            .iter()
            .map(|r| ComfortRate {
                fill: r.fill,
                max_marginal_price: r.bid,
                max_marginal_co2: 0.0,
            })
            .collect()
    };

    let value_curve = ValueCurve {
        comfort_rates,
        deadline_tiers: vec![DeadlineTier {
            deadline: latest_end,
            max_total_cost_eur: None,
            max_marginal_rate_eur_kwh: None,
            min_completion: 0.8,
        }],
        active_tier_index: 0,
    };
    EnergyPacket {
        target_soc: seed.target_soc,
        ..EnergyPacket::new(
            asset_id,
            target_energy_kwh,
            desired_power_kw,
            value_curve,
            now,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::tariff_snapshot::TariffSnapshot;
    use chrono::TimeZone;

    // ── alloc_cost_eur ────────────────────────────────────────────────────────

    #[test]
    fn alloc_cost_eur_grid_only() {
        // No PV surplus: cost = grid_kw * import_tariff * slot_h
        let cost = alloc_cost_eur(0.0, 3.0, 0.05, 0.25, 1.0);
        assert!((cost - 0.75).abs() < 1e-9);
    }

    #[test]
    fn alloc_cost_eur_surplus_and_grid() {
        // 1 kW from PV (export tariff) + 2 kW from grid (import tariff), 1h slot
        let cost = alloc_cost_eur(1.0, 2.0, 0.05, 0.25, 1.0);
        assert!((cost - (1.0 * 0.05 + 2.0 * 0.25)).abs() < 1e-9);
    }

    #[test]
    fn alloc_cost_eur_discharge_is_negative() {
        // Discharge 2 kW saves grid import: cost = -2 * 0.25 * 1h = -0.5
        let cost = alloc_cost_eur(0.0, -2.0, 0.05, 0.25, 1.0);
        assert!((cost - (-0.50)).abs() < 1e-9);
    }

    #[test]
    fn alloc_cost_eur_zero_power_is_zero() {
        assert_eq!(alloc_cost_eur(0.0, 0.0, 0.05, 0.25, 1.0), 0.0);
    }

    fn ts(hour: u32, min: u32, sec: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 3, 21, hour, min, sec).unwrap()
    }

    fn snap(
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        imp: Option<f64>,
        exp: Option<f64>,
        co2: Option<f64>,
    ) -> TariffSnapshot {
        TariffSnapshot {
            interval_start: start,
            interval_end: end,
            import_tariff_eur_kwh: imp,
            export_tariff_eur_kwh: exp,
            co2_g_kwh: co2,
        }
    }

    fn empty_capacity() -> OadrCapacityState {
        OadrCapacityState {
            import_limit_kw: None,
            export_limit_kw: None,
            import_subscription_kw: None,
            import_reservation_kw: None,
            import_limit_event_id: None,
            export_limit_event_id: None,
            last_updated: None,
        }
    }

    fn test_profile(step_s: u64, horizon_h: u64) -> Profile {
        let mut p = Profile::default();
        p.planner.plan_step_s = step_s;
        p.planner.plan_horizon_h = horizon_h;
        p.planner.near_horizon_h = horizon_h; // all firm
        p
    }

    // ── Tariff resampling tests (T008) ──────────────────────────────────────

    #[test]
    fn boundary_aligned_tariffs_match_old_behavior() {
        // Tariffs aligned on 5-min boundaries: each slot gets its exact tariff
        let snaps = vec![
            snap(
                ts(10, 0, 0),
                ts(10, 5, 0),
                Some(0.20),
                Some(0.05),
                Some(300.0),
            ),
            snap(
                ts(10, 5, 0),
                ts(10, 10, 0),
                Some(0.25),
                Some(0.06),
                Some(350.0),
            ),
        ];
        let tariffs = TariffTimeSeries::from_snapshots(&snaps);
        let now = ts(10, 0, 0);
        let (firm, _) = build_grid(
            &tariffs,
            &empty_capacity(),
            test_profile(300, 1).base_load_kw(),
            now,
            300,
            2,
            now + Duration::hours(1),
            &HashMap::new(),
        );
        assert_eq!(firm.len(), 2);
        assert!((firm[0].import_tariff_eur_kwh - 0.20).abs() < 1e-9);
        assert!((firm[1].import_tariff_eur_kwh - 0.25).abs() < 1e-9);
        assert!((firm[0].export_tariff_eur_kwh - 0.05).abs() < 1e-9);
        assert!((firm[0].co2_g_kwh - 300.0).abs() < 1e-9);
    }

    #[test]
    fn mid_slot_tariff_change_uses_start_of_slot_value() {
        // Tariff changes at 10:03 inside a 5-min slot [10:00, 10:05).
        // interpolate_at(10:00) = 0.20 (Step LOCF: value at slot start).
        let snaps = vec![
            snap(ts(10, 0, 0), ts(10, 3, 0), Some(0.20), None, None),
            snap(ts(10, 3, 0), ts(10, 10, 0), Some(0.10), None, None),
        ];
        let tariffs = TariffTimeSeries::from_snapshots(&snaps);
        let now = ts(10, 0, 0);
        let (firm, _) = build_grid(
            &tariffs,
            &empty_capacity(),
            test_profile(300, 1).base_load_kw(),
            now,
            300,
            1,
            now + Duration::hours(1),
            &HashMap::new(),
        );
        assert_eq!(firm.len(), 1);
        assert!((firm[0].import_tariff_eur_kwh - 0.20).abs() < 1e-9);
    }

    #[test]
    fn empty_tariff_series_uses_defaults() {
        let tariffs = TariffTimeSeries::from_snapshots(&[]);
        let now = ts(10, 0, 0);
        let (firm, _) = build_grid(
            &tariffs,
            &empty_capacity(),
            test_profile(300, 1).base_load_kw(),
            now,
            300,
            2,
            now + Duration::hours(1),
            &HashMap::new(),
        );
        assert_eq!(firm.len(), 2);
        assert!((firm[0].import_tariff_eur_kwh - DEFAULT_IMPORT_PRICE).abs() < 1e-9);
        assert!((firm[0].export_tariff_eur_kwh - DEFAULT_EXPORT_PRICE).abs() < 1e-9);
        assert!((firm[0].co2_g_kwh - DEFAULT_CO2_G_KWH).abs() < 1e-9);
        assert!(firm[0].rate_estimated);
    }

    #[test]
    fn single_sample_tariff_locf_covers_all_slots() {
        // One tariff sample at 10:00 with interval ending 11:00.
        // interpolate_at uses Step LOCF: all slots at or after 10:00 get 0.30.
        let snaps = vec![snap(
            ts(10, 0, 0),
            ts(11, 0, 0),
            Some(0.30),
            Some(0.08),
            Some(400.0),
        )];
        let tariffs = TariffTimeSeries::from_snapshots(&snaps);
        let now = ts(10, 0, 0);
        let (firm, _) = build_grid(
            &tariffs,
            &empty_capacity(),
            test_profile(300, 1).base_load_kw(),
            now,
            300,
            4,
            now + Duration::hours(1),
            &HashMap::new(),
        );
        assert_eq!(firm.len(), 4);
        assert!((firm[0].import_tariff_eur_kwh - 0.30).abs() < 1e-9);
        assert!(!firm[0].rate_estimated);
        // Step LOCF carries value to all subsequent slots
        assert!((firm[1].import_tariff_eur_kwh - 0.30).abs() < 1e-9);
        assert!((firm[3].import_tariff_eur_kwh - 0.30).abs() < 1e-9);
    }

    // ── Asset forecast resampling tests (T013) ──────────────────────────────

    #[test]
    fn pv_linear_forecast_resampled() {
        // PV forecast at slot start (10:00): 7.5 kW generation magnitude
        let mut pv_kw_map = HashMap::new();
        pv_kw_map.insert(ts(10, 0, 0).timestamp(), 7.5_f64);

        let tariffs = TariffTimeSeries::from_snapshots(&[snap(
            ts(10, 0, 0),
            ts(11, 0, 0),
            Some(0.20),
            Some(0.05),
            Some(300.0),
        )]);
        let now = ts(10, 0, 0);
        let (firm, _) = build_grid(
            &tariffs,
            &empty_capacity(),
            test_profile(300, 1).base_load_kw(),
            now,
            300,
            1,
            now + Duration::hours(1),
            &pv_kw_map,
        );
        assert!((firm[0].pv_forecast_kw - 7.5).abs() < 0.1);
    }

    #[test]
    fn empty_forecast_defaults_to_zero() {
        let tariffs = TariffTimeSeries::from_snapshots(&[snap(
            ts(10, 0, 0),
            ts(11, 0, 0),
            Some(0.20),
            Some(0.05),
            Some(300.0),
        )]);
        let now = ts(10, 0, 0);
        let (firm, _) = build_grid(
            &tariffs,
            &empty_capacity(),
            test_profile(300, 1).base_load_kw(),
            now,
            300,
            1,
            now + Duration::hours(1),
            &HashMap::new(),
        );
        assert!((firm[0].pv_forecast_kw).abs() < 1e-9);
    }

    #[test]
    fn missing_asset_key_defaults_to_zero() {
        // Empty pv_kw_map — slot timestamp not present, defaults to 0.0
        let tariffs = TariffTimeSeries::from_snapshots(&[snap(
            ts(10, 0, 0),
            ts(11, 0, 0),
            Some(0.20),
            Some(0.05),
            Some(300.0),
        )]);
        let now = ts(10, 0, 0);
        let (firm, _) = build_grid(
            &tariffs,
            &empty_capacity(),
            test_profile(300, 1).base_load_kw(),
            now,
            300,
            1,
            now + Duration::hours(1),
            &HashMap::new(),
        );
        assert!((firm[0].pv_forecast_kw).abs() < 1e-9);
    }

    // ── finalize_packets single-pass test (RF-B07) ───────────────────────────

    fn make_slot(allocations: Vec<PacketAllocation>) -> PlanTimeSlot {
        PlanTimeSlot {
            slot_index: 0,
            start: ts(10, 0, 0),
            end: ts(10, 5, 0),
            slot_type: SlotType::Firm,
            import_tariff_eur_kwh: 0.20,
            export_tariff_eur_kwh: 0.05,
            co2_g_kwh: 300.0,
            grid_effective_cost: 0.20,
            rate_estimated: false,
            import_cap_kw: 10.0,
            export_cap_kw: 10.0,
            baseline_kw: 0.0,
            pv_forecast_kw: 0.0,
            surplus_available_kw: 0.0,
            allocations,
            net_import_kw: 0.0,
            net_export_kw: 0.0,
            import_flexibility_kw: 0.0,
            export_flexibility_kw: 0.0,
        }
    }

    fn make_alloc(packet_id: Uuid, power_kw: f64, cost_eur: f64, co2_g: f64) -> PacketAllocation {
        PacketAllocation {
            packet_id,
            asset_id: "ev".to_string(),
            power_kw,
            surplus_power_kw: 0.0,
            grid_power_kw: power_kw,
            marginal_value: 0.0,
            cost_eur,
            co2_g,
        }
    }

    #[test]
    // ── PV allocation via update_slot_from_step ──────────────────────────────

    /// Verifies the export branch of update_slot_from_step: a negative actual_kw
    /// (PV export) ends up in slot.allocations with the exact same power_kw.
    /// This is the mechanism through which the planner's pv_kw_map override
    /// (actual_kw = -pv_kw_map[ts]) reaches the timeline.
    #[test]
    fn pv_export_lands_in_slot_allocations() {
        let mut slot = make_slot(vec![]);
        // Simulate the planner override: pv_kw_map had 7.5 kW → actual_kw = -7.5
        let actual_kw = -7.5_f64;
        update_slot_from_step(&mut slot, "pv", actual_kw, &[], &mut HashMap::new(), 5.0 / 60.0);

        let pv = slot.allocations.iter().find(|a| a.asset_id == "pv");
        assert!(pv.is_some(), "PV export must produce an allocation entry");
        assert!(
            (pv.unwrap().power_kw - actual_kw).abs() < 1e-9,
            "allocation power_kw must equal actual_kw; expected {actual_kw}, got {}",
            pv.unwrap().power_kw
        );
    }

    /// When the planner overrides PV actual_kw from a map value, the resulting
    /// allocation must differ from what cfg.step() (flat irradiance) would have given.
    #[test]
    fn pv_allocation_uses_map_value_not_flat_irradiance() {
        let mut slot = make_slot(vec![]);

        let flat_irradiance = 0.5_f64;
        let rated_kw = 10.0_f64;
        // What cfg.step() with flat irradiance would produce:
        let flat_actual_kw = -(flat_irradiance * rated_kw); // -5.0

        // What pv_kw_map (sin model) would give for a daytime slot:
        let map_value_kw = 8.66_f64; // e.g. sin model at 9am for rated=10
        let map_actual_kw = -map_value_kw; // -8.66

        // Planner uses map_actual_kw (the override), NOT flat_actual_kw.
        update_slot_from_step(&mut slot, "pv", map_actual_kw, &[], &mut HashMap::new(), 5.0 / 60.0);

        let pv = slot.allocations.iter().find(|a| a.asset_id == "pv").unwrap();
        assert!(
            (pv.power_kw - map_actual_kw).abs() < 1e-9,
            "must use map-derived value {map_actual_kw:.3}, not flat {flat_actual_kw:.3}"
        );
        // Sanity: the flat value is different from the map value — confirms the test is non-trivial.
        assert!((map_actual_kw - flat_actual_kw).abs() > 1.0);
    }

    fn finalize_packets_single_pass_matches_three_pass() {
        let slot_h = 5.0 / 60.0; // 5-minute slots → 1/12 h
        let now = ts(10, 0, 0);

        let curve = ValueCurve {
            comfort_rates: vec![],
            deadline_tiers: vec![],
            active_tier_index: 0,
        };

        // Two packets across three slots
        let mut p1 = EnergyPacket::new("ev".to_string(), 10.0, 7.2, curve.clone(), now);
        let mut p2 = EnergyPacket::new("heater".to_string(), 5.0, 3.0, curve, now);

        // Slot 0: p1 gets 7.2 kW (cost 0.06 €, co2 18 g), p2 gets 3.0 kW (cost 0.025 €, co2 7.5 g)
        let slot0 = make_slot(vec![
            make_alloc(p1.id, 7.2, 0.06, 18.0),
            make_alloc(p2.id, 3.0, 0.025, 7.5),
        ]);
        // Slot 1: p1 gets 7.2 kW (cost 0.06 €, co2 18 g)
        let slot1 = make_slot(vec![make_alloc(p1.id, 7.2, 0.06, 18.0)]);
        // Slot 2: p2 gets 3.0 kW (cost 0.025 €, co2 7.5 g)
        let slot2 = make_slot(vec![make_alloc(p2.id, 3.0, 0.025, 7.5)]);

        let firm_slots = vec![slot0, slot1, slot2];
        let mut packets = vec![p1, p2];

        finalize_packets(&mut packets, &firm_slots, slot_h, now);

        let out_p1 = &packets[0];
        let out_p2 = &packets[1];

        // p1: 2 slots × 7.2 kW → allocated_kwh = 2 × 7.2 × slot_h = 1.2 kWh
        // target=10, past=0 → completion = 1.2/10 = 0.12
        let expected_p1_kwh = 2.0 * 7.2 * slot_h;
        assert!((out_p1.estimated_cost_eur - 0.12).abs() < 1e-9);
        assert!((out_p1.estimated_co2_g - 36.0).abs() < 1e-9);
        assert!((out_p1.estimated_completion - expected_p1_kwh / 10.0).abs() < 1e-9);
        assert_eq!(out_p1.last_estimate_at, Some(now));

        // p2: 2 slots × 3.0 kW → allocated_kwh = 2 × 3.0 × slot_h = 0.5 kWh
        // target=5, past=0 → completion = 0.5/5 = 0.1
        let expected_p2_kwh = 2.0 * 3.0 * slot_h;
        assert!((out_p2.estimated_cost_eur - 0.05).abs() < 1e-9);
        assert!((out_p2.estimated_co2_g - 15.0).abs() < 1e-9);
        assert!((out_p2.estimated_completion - expected_p2_kwh / 5.0).abs() < 1e-9);
        assert_eq!(out_p2.last_estimate_at, Some(now));
    }

    // ── pv_forecast_kw uses shared `now` → sin model, not flat irradiance ─────

    /// Verifies that pv_kw_map is built using the planner's `now` reference and
    /// forecast_kw_at, so slot.pv_forecast_kw reflects the sin-model curve rather
    /// than the flat current irradiance value.
    #[test]
    fn pv_forecast_kw_follows_sin_model() {
        use crate::assets::PvInverter;

        let rated_kw = 10.0_f64;
        let pv = PvInverter {
            rated_kw,
            irradiance: 0.5,       // current (flat) value — should NOT appear in forecast
            irradiance_offset: 0.0, // no perturbation
            pv_alpha: 0.1,
            export_limit_kw: None,
        };

        // UTC 12:00 — peak irradiance. sin(π*(12-6)/12) = sin(π/2) = 1.0
        let noon = chrono::Utc.with_ymd_and_hms(2025, 6, 21, 12, 0, 0).unwrap();
        let step_s: u64 = 300;
        let total_steps = 4;

        let tau_s: f64 = -(step_s as f64) / (1.0 - pv.pv_alpha).ln();
        let pv_kw_map: HashMap<i64, f64> = (0..total_steps)
            .map(|i| {
                let t = noon + Duration::seconds((i as i64) * step_s as i64);
                let seconds_ahead = (i as f64) * step_s as f64;
                (t.timestamp(), pv.forecast_kw_at(t, seconds_ahead, tau_s))
            })
            .collect();

        let tariffs = TariffTimeSeries::from_snapshots(&[snap(
            noon,
            noon + Duration::hours(4),
            Some(0.20),
            Some(0.05),
            Some(300.0),
        )]);
        let (firm, _) = build_grid(
            &tariffs,
            &empty_capacity(),
            test_profile(step_s, 1).base_load_kw(),
            noon,
            step_s,
            total_steps,
            noon + Duration::hours(4),
            &pv_kw_map,
        );

        // Slot 0 = noon: irradiance ≈ 1.0 → pv_forecast_kw ≈ 10.0 kW
        assert!(
            (firm[0].pv_forecast_kw - 10.0).abs() < 0.1,
            "noon slot: expected ≈10.0 kW, got {:.3}",
            firm[0].pv_forecast_kw
        );
        // Slot 3 = 12:15: irradiance still near peak, definitely >> 5.0 (the flat value)
        assert!(
            firm[3].pv_forecast_kw > 5.0,
            "12:15 slot: expected > 5.0 kW (near-peak sin), got {:.3}",
            firm[3].pv_forecast_kw
        );
        // Flat irradiance (0.5) would give 5.0 kW — confirm we're NOT using that
        assert!(
            (firm[0].pv_forecast_kw - 5.0).abs() > 1.0,
            "slot 0 must differ from flat irradiance value 5.0; got {:.3}",
            firm[0].pv_forecast_kw
        );
    }
}
