use rand::Rng;

/// Simple voltage model: 230V base + small random variance.
pub fn random_voltage() -> f64 {
    let mut rng = rand::thread_rng();
    230.0 + rng.gen_range(-2.0..2.0)
}
