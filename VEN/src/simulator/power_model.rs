use rand::Rng;

/// Simple voltage model: 230V base + small random variance.
/// Takes the RNG explicitly (R-24) so callers can seed it for deterministic
/// test sequences instead of an unseeded global `thread_rng()`.
pub fn random_voltage(rng: &mut impl Rng) -> f64 {
    230.0 + rng.gen_range(-2.0..2.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{rngs::StdRng, SeedableRng};

    #[test]
    fn same_seed_produces_identical_sequence() {
        let mut a = StdRng::seed_from_u64(42);
        let mut b = StdRng::seed_from_u64(42);
        let seq_a: Vec<f64> = (0..5).map(|_| random_voltage(&mut a)).collect();
        let seq_b: Vec<f64> = (0..5).map(|_| random_voltage(&mut b)).collect();
        assert_eq!(
            seq_a, seq_b,
            "identical seeds must produce identical sequences"
        );
    }

    #[test]
    fn different_seeds_produce_different_sequences() {
        let mut a = StdRng::seed_from_u64(1);
        let mut b = StdRng::seed_from_u64(2);
        let seq_a: Vec<f64> = (0..5).map(|_| random_voltage(&mut a)).collect();
        let seq_b: Vec<f64> = (0..5).map(|_| random_voltage(&mut b)).collect();
        assert_ne!(
            seq_a, seq_b,
            "different seeds should (overwhelmingly likely) diverge"
        );
    }
}
