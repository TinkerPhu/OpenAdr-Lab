/// Stage 3 — HEMS Planning Algorithm (8-phase greedy scheduler).
///
/// Produces a Plan from TariffSnapshots + EnergyPackets + device profile.
/// Phase 6 (penalty check) is deferred to Stage 4.
use crate::common::{Interpolation, QuantityTimeline};
use crate::entities::asset::{ComfortRate, CompletionPolicy, PlanTrigger, UserRequestMode};
use crate::entities::capacity::OadrCapacityState;
use crate::entities::energy_packet::{DeadlineTier, EnergyPacket, PacketStatus, ValueCurve};
use crate::entities::plan::{
    FirmSummary, FlexibilityEnvelope, FlexibleSummary, PacketAllocation, Plan, PlanTimeSlot,
    PlanningHorizon, SlotType,
};
use crate::entities::tariff_snapshot::TariffSnapshot;
use crate::profile::{BatteryConfig, Profile};
use chrono::{DateTime, Duration, Utc};
use std::collections::HashMap;
use uuid::Uuid;

// ─── Constants ───────────────────────────────────────────────────────────────
const DEFAULT_IMPORT_PRICE: f64 = 0.20;
const DEFAULT_EXPORT_PRICE: f64 = 0.05;
const DEFAULT_CO2_G_KWH: f64 = 300.0;
const CO2_WEIGHT: f64 = 0.0001; // €/g ≈ €100/tonne

// ─── Public entry point ───────────────────────────────────────────────────────

/// Run the full 8-phase planning algorithm and return a new Plan.
pub fn run_planner(
    rates: &[TariffSnapshot],
    packets: &[EnergyPacket],
    capacity: &OadrCapacityState,
    profile: &Profile,
    now: DateTime<Utc>,
    trigger: PlanTrigger,
    asset_forecasts: &HashMap<String, QuantityTimeline>,
) -> Plan {
    let step_s = profile.planner.plan_step_s;
    let horizon_h = profile.planner.plan_horizon_h;
    let near_h = profile.planner.near_horizon_h;
    let slot_h = step_s as f64 / 3600.0;

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

    // Phase 1: Build planning grid
    let (mut firm_slots, flexible_slots) =
        build_grid(rates, capacity, profile, now, step_s, total_steps, firm_boundary, asset_forecasts);

    // Preserve terminal packets (cancelled/completed/failed) for history visibility
    let terminal_pkts: Vec<EnergyPacket> =
        packets.iter().filter(|p| p.is_terminal()).cloned().collect();

    // Work on non-terminal packets only
    let mut pkts: Vec<EnergyPacket> =
        packets.iter().filter(|p| !p.is_terminal()).cloned().collect();

    // Phase 2+3: Score + allocate consumption to FIRM slots
    allocate_consumption(&mut firm_slots, &mut pkts, slot_h, now);

    // Phase 4: Battery arbitrage
    if let Some(battery) = profile.battery_config() {
        allocate_battery(&mut firm_slots, battery, slot_h);
    }

    // Phase 5: Residual PV surplus already reflected in slot.net_export_kw

    // Phase 6: Penalty check — deferred to Stage 4

    // Phase 7: Flexibility envelopes
    let envelopes = build_envelopes(&pkts, &flexible_slots, &firm_slots, slot_h);

    // Phase 8: Finalize
    finalize_packets(&mut pkts, &firm_slots, slot_h, now);
    update_slot_flexibility(&mut firm_slots);

    let firm_summary = summarize_firm(&firm_slots, slot_h);
    let flexible_summary = FlexibleSummary {
        total_energy_kwh: envelopes.iter().map(|e| e.energy_needed_kwh).sum(),
        estimated_cost_eur: envelopes.iter().map(|e| e.estimated_cost_eur).sum(),
        estimated_co2_g: envelopes.iter().map(|e| e.estimated_co2_g).sum(),
    };

    // Re-append terminal packets so they remain visible in GET /packets
    pkts.extend(terminal_pkts);

    Plan {
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
    }
}

// ─── Phase 1: Build planning grid ────────────────────────────────────────────

fn build_grid(
    rates: &[TariffSnapshot],
    capacity: &OadrCapacityState,
    profile: &Profile,
    now: DateTime<Utc>,
    step_s: u64,
    total_steps: usize,
    firm_boundary: DateTime<Utc>,
    asset_forecasts: &HashMap<String, QuantityTimeline>,
) -> (Vec<PlanTimeSlot>, Vec<PlanTimeSlot>) {
    let import_cap = capacity.import_limit_kw.unwrap_or(f64::MAX);
    let export_cap = capacity.export_limit_kw.unwrap_or(f64::MAX);
    let baseline_kw = profile.base_load_kw();
    let rates_empty = rates.is_empty();

    let mut firm = Vec::new();
    let mut flex = Vec::new();

    for i in 0..total_steps {
        let start = now + Duration::seconds((i as i64) * step_s as i64);
        let end = start + Duration::seconds(step_s as i64);

        let import_tariff = tariff_import_at(rates, start).unwrap_or(DEFAULT_IMPORT_PRICE);
        let export_tariff = tariff_export_at(rates, start).unwrap_or(DEFAULT_EXPORT_PRICE);
        let co2 = tariff_co2_at(rates, start).unwrap_or(DEFAULT_CO2_G_KWH);
        let grid_eff = import_tariff + co2 * CO2_WEIGHT;

        // PV forecast: negative = export, so we negate to get positive generation magnitude.
        let pv_export_kw = nearest_value(asset_forecasts.get("pv"), start);
        let pv_kw = -pv_export_kw; // convert export (negative) to generation magnitude (positive)
        let net = baseline_kw - pv_kw; // positive = need to import, negative = surplus
        let surplus = (-net).max(0.0);
        let net_import = net.max(0.0);
        let net_export = surplus;

        let slot_type = if end <= firm_boundary {
            SlotType::Firm
        } else {
            SlotType::Flexible
        };

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
            allocations: vec![],
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

// ─── Phase 2+3: Score + Allocate Consumption (FIRM only) ─────────────────────

struct AllocEntry {
    slot_index: usize,
    packet_id: Uuid,
    marginal_value: f64,
    eligible: bool,
}

fn allocate_consumption(
    slots: &mut Vec<PlanTimeSlot>,
    packets: &mut Vec<EnergyPacket>,
    slot_h: f64,
    now: DateTime<Utc>,
) {
    // Track energy already allocated for each packet in this plan cycle
    let mut allocated: std::collections::HashMap<Uuid, f64> =
        packets.iter().map(|p| (p.id, 0.0_f64)).collect();

    // Build scoring entries
    let far_future = now + Duration::days(365);
    let mut entries: Vec<AllocEntry> = Vec::new();

    for packet in packets.iter() {
        if packet.is_terminal() || packet.target_energy_kwh <= 0.0 {
            continue;
        }
        let undelivered = packet.undelivered_energy_kwh();
        if undelivered <= 0.0 {
            continue;
        }

        let latest_end = packet.latest_end().unwrap_or(far_future);

        let slots_remaining = slots
            .iter()
            .filter(|s| s.start >= packet.earliest_start && s.end <= latest_end)
            .count()
            .max(1);

        let slots_needed =
            (undelivered / packet.desired_power_kw.max(1e-9) / slot_h).ceil() as usize;
        let time_pressure = (slots_needed as f64 / slots_remaining as f64)
            .max(1.0)
            .min(3.0);

        for (si, slot) in slots.iter().enumerate() {
            if slot.start < packet.earliest_start || slot.start >= latest_end {
                continue;
            }

            let import_head = (slot.import_cap_kw - slot.net_import_kw).max(0.0);
            if import_head <= 0.0 && slot.surplus_available_kw <= 0.0 {
                continue;
            }

            let fill = packet.fill()
                + allocated[&packet.id] / packet.target_energy_kwh.max(1e-9);
            let comfort_bid = packet.value_curve.bid_at(fill.min(1.0));

            let surplus_frac =
                (slot.surplus_available_kw / packet.desired_power_kw.max(1e-9)).min(1.0);
            let eff_cost = slot.import_tariff_eur_kwh * (1.0 - surplus_frac)
                + slot.export_tariff_eur_kwh * surplus_frac;

            let eligible = comfort_bid >= eff_cost || time_pressure >= 2.0;
            let marginal_value = comfort_bid * time_pressure;

            entries.push(AllocEntry {
                slot_index: si,
                packet_id: packet.id,
                marginal_value,
                eligible,
            });
        }
    }

    // Sort by MarginalValue DESC (greedy knapsack)
    entries.sort_by(|a, b| {
        b.marginal_value
            .partial_cmp(&a.marginal_value)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Greedy allocation pass
    for entry in &entries {
        if !entry.eligible {
            continue;
        }

        let pi = match packets.iter().position(|p| p.id == entry.packet_id) {
            Some(i) => i,
            None => continue,
        };

        let already = *allocated.get(&entry.packet_id).unwrap_or(&0.0);
        let undelivered = (packets[pi].undelivered_energy_kwh() - already).max(0.0);
        if undelivered <= 1e-6 {
            continue;
        }

        // Compute allocation (read slot values first, then mutate)
        let (power_kw, surplus_used, grid_used, cost, co2, energy_kwh) = {
            let slot = &slots[entry.slot_index];
            let import_head = (slot.import_cap_kw - slot.net_import_kw).max(0.0);
            let power = packets[pi]
                .desired_power_kw
                .min(import_head + slot.surplus_available_kw);
            if power <= 0.0 {
                continue;
            }
            let surplus = slot.surplus_available_kw.min(power);
            let grid = (power - surplus).max(0.0);
            let e = power * slot_h;
            let c = surplus * slot.export_tariff_eur_kwh * slot_h
                + grid * slot.import_tariff_eur_kwh * slot_h;
            let co2v = grid * slot.co2_g_kwh * slot_h;
            (power, surplus, grid, c, co2v, e)
        };

        if power_kw <= 0.0 {
            continue;
        }

        // No power clamping — dispatcher detects completion in real time.
        // Track allocated energy to avoid scheduling more slots than needed.

        let slot = &mut slots[entry.slot_index];
        slot.surplus_available_kw -= surplus_used;
        slot.net_import_kw += grid_used;
        slot.net_export_kw = (slot.net_export_kw - surplus_used).max(0.0);

        slot.allocations.push(PacketAllocation {
            packet_id: entry.packet_id,
            asset_id: packets[pi].asset_id.clone(),
            power_kw,
            surplus_power_kw: surplus_used,
            grid_power_kw: grid_used,
            marginal_value: entry.marginal_value,
            cost_eur: cost,
            co2_g: co2,
        });

        // Track energy booked for this packet so far (capped at undelivered to avoid over-scheduling)
        *allocated.entry(entry.packet_id).or_insert(0.0) += energy_kwh.min(undelivered);

        if packets[pi].status == PacketStatus::Pending {
            packets[pi].status = PacketStatus::Scheduled;
        }
    }
}

// ─── Phase 4: Battery Arbitrage ───────────────────────────────────────────────

fn allocate_battery(slots: &mut Vec<PlanTimeSlot>, battery: &BatteryConfig, slot_h: f64) {
    let n = slots.len();
    if n < 2 {
        return;
    }

    // Compute median tariff as arbitrage threshold
    let mut prices: Vec<f64> = slots.iter().map(|s| s.import_tariff_eur_kwh).collect();
    prices.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median = prices[n / 2];
    let eff = battery.round_trip_efficiency.sqrt();

    let mut soc = battery.initial_soc;
    let cap = battery.capacity_kwh;
    let min_soc = battery.min_soc;

    for slot in slots.iter_mut() {
        let price = slot.import_tariff_eur_kwh;

        if price < median * eff {
            // Cheap slot: charge from surplus or cheap grid
            let head_kwh = (cap - soc * cap).min(battery.max_charge_kw * slot_h);
            let surp_kwh = slot.surplus_available_kw * slot_h;
            let grid_head_kwh =
                (slot.import_cap_kw - slot.net_import_kw).max(0.0) * slot_h;
            let charge_kwh = (surp_kwh + grid_head_kwh).min(head_kwh).max(0.0);

            if charge_kwh > 0.01 {
                let surp_used = surp_kwh.min(charge_kwh);
                let grid_used = (charge_kwh - surp_used).max(0.0);
                let charge_kw = charge_kwh / slot_h;

                slot.surplus_available_kw -= surp_used / slot_h;
                slot.net_import_kw += grid_used / slot_h;
                soc = (soc + charge_kwh * eff / cap).min(1.0);

                slot.allocations.push(PacketAllocation {
                    packet_id: Uuid::nil(),
                    asset_id: "battery".to_string(),
                    power_kw: charge_kw,
                    surplus_power_kw: surp_used / slot_h,
                    grid_power_kw: grid_used / slot_h,
                    marginal_value: 0.0,
                    cost_eur: grid_used * price,
                    co2_g: grid_used * slot.co2_g_kwh,
                });
            }
        } else if price > median / eff {
            // Expensive slot: discharge battery
            let avail_kwh = ((soc - min_soc) * cap)
                .min(battery.max_discharge_kw * slot_h)
                .max(0.0);

            if avail_kwh > 0.01 {
                let discharge_kw = avail_kwh / slot_h;
                slot.net_import_kw = (slot.net_import_kw - discharge_kw).max(0.0);
                soc = (soc - avail_kwh / cap / eff).max(min_soc);

                slot.allocations.push(PacketAllocation {
                    packet_id: Uuid::nil(),
                    asset_id: "battery".to_string(),
                    power_kw: -discharge_kw,
                    surplus_power_kw: 0.0,
                    grid_power_kw: -discharge_kw,
                    marginal_value: 0.0,
                    cost_eur: -avail_kwh * price,
                    co2_g: 0.0,
                });
            }
        }
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

        let avg_import = eligible.iter().map(|s| s.import_tariff_eur_kwh).sum::<f64>() / n as f64;
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
            budget_remaining_eur: f64::MAX,
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
        let allocated_kwh: f64 = firm_slots
            .iter()
            .flat_map(|s| s.allocations.iter())
            .filter(|a| a.packet_id == packet.id)
            .map(|a| a.power_kw * slot_h)
            .sum();

        let cost: f64 = firm_slots
            .iter()
            .flat_map(|s| s.allocations.iter())
            .filter(|a| a.packet_id == packet.id)
            .map(|a| a.cost_eur)
            .sum();

        let co2: f64 = firm_slots
            .iter()
            .flat_map(|s| s.allocations.iter())
            .filter(|a| a.packet_id == packet.id)
            .map(|a| a.co2_g)
            .sum();

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

// ─── Tariff helpers ───────────────────────────────────────────────────────────

fn tariff_import_at(rates: &[TariffSnapshot], ts: DateTime<Utc>) -> Option<f64> {
    rates
        .iter()
        .find(|r| r.interval_start <= ts && ts < r.interval_end)
        .and_then(|r| r.import_tariff_eur_kwh)
}

fn tariff_export_at(rates: &[TariffSnapshot], ts: DateTime<Utc>) -> Option<f64> {
    rates
        .iter()
        .find(|r| r.interval_start <= ts && ts < r.interval_end)
        .and_then(|r| r.export_tariff_eur_kwh)
}

fn tariff_co2_at(rates: &[TariffSnapshot], ts: DateTime<Utc>) -> Option<f64> {
    rates
        .iter()
        .find(|r| r.interval_start <= ts && ts < r.interval_end)
        .and_then(|r| r.co2_g_kwh)
}

// ─── Forecast lookup helper ───────────────────────────────────────────────────

/// Return the forecast value for a given timestamp from a QuantityTimeline.
/// - If the series is empty or absent, returns 0.0.
/// - Step series: last sample at or before ts; falls back to first sample.
/// - Linear series: nearest sample by timestamp.
fn nearest_value(series: Option<&QuantityTimeline>, ts: DateTime<Utc>) -> f64 {
    let s = match series {
        Some(s) if !s.samples.is_empty() => s,
        _ => return 0.0,
    };
    match s.interpolation {
        Interpolation::Step => {
            // Last sample at or before ts.
            s.samples
                .iter()
                .rev()
                .find(|(t, _)| *t <= ts)
                .or_else(|| s.samples.first())
                .map(|(_, v)| *v)
                .unwrap_or(0.0)
        }
        Interpolation::Linear => {
            // Nearest sample by absolute time distance.
            s.samples
                .iter()
                .min_by_key(|(t, _)| {
                    let diff = (*t - ts).num_milliseconds().unsigned_abs();
                    diff
                })
                .map(|(_, v)| *v)
                .unwrap_or(0.0)
        }
    }
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
            ComfortRate { fill: 0.0, max_marginal_price: 0.35, max_marginal_co2: 0.0 },
            ComfortRate { fill: 1.0, max_marginal_price: 0.05, max_marginal_co2: 0.0 },
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

    EnergyPacket {
        id: Uuid::new_v4(),
        asset_id,
        status: PacketStatus::Pending,
        earliest_start: now,
        latest_start: None,
        target_energy_kwh,
        target_soc: seed.target_soc,
        desired_power_kw,
        value_curve: ValueCurve {
            comfort_rates,
            deadline_tiers: vec![DeadlineTier {
                deadline: latest_end,
                max_total_cost_eur: None,
                max_marginal_rate_eur_kwh: None,
                min_completion: 0.8,
            }],
            active_tier_index: 0,
        },
        request_mode: UserRequestMode::ByDeadline,
        completion_policy: CompletionPolicy::Stop,
        post_deadline_comfort_bid: None,
        planned_power_profile: vec![],
        past_power_profile: vec![],
        accumulated_cost_eur: 0.0,
        accumulated_co2_g: 0.0,
        estimated_cost_eur: 0.0,
        estimated_co2_g: 0.0,
        estimated_completion: 0.0,
        last_estimate_at: None,
        created_at: now,
        updated_at: now,
    }
}
