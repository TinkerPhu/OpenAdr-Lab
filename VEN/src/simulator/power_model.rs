use rand::Rng;

/// Net power computation result
#[derive(Debug, Clone)]
pub struct PowerResult {
    pub net_w: f64,       // positive = import, negative = export
    pub import_w: f64,    // max(0, net)
    pub export_w: f64,    // max(0, -net)
    pub voltage_v: f64,
}

/// Compute net power from device states.
/// `base_load_w`: fixed base load (always positive/import)
/// `ev_w`: EV power consumption (positive)
/// `heater_w`: heater power consumption (positive)
/// `pv_generation_w`: PV generation (positive value, subtracted from load)
pub fn compute_net_power(
    base_load_w: f64,
    ev_w: f64,
    heater_w: f64,
    pv_generation_w: f64,
) -> PowerResult {
    let net_w = base_load_w + ev_w + heater_w - pv_generation_w;
    let import_w = net_w.max(0.0);
    let export_w = (-net_w).max(0.0);

    // Simple voltage model: 230V base + small random variance
    let mut rng = rand::thread_rng();
    let voltage_v = 230.0 + rng.gen_range(-2.0..2.0);

    PowerResult {
        net_w,
        import_w,
        export_w,
        voltage_v,
    }
}
