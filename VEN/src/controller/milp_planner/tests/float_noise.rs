//! Comparing recorded text across CPU architectures.
//!
//! The planner goldens (`plans_golden`, `model_fingerprint`) were recorded on x86. On aarch64
//! the same code, same inputs and same HiGHS answer differ in the last digits of some floats
//! (relative 1e-12: the sums are formed in a different order), so a byte comparison fails there
//! for no reason a reader cares about. Two tools make the comparison about the numbers:
//!
//! - `same_within_float_noise`: two recorded lines agree when their text and integers are equal
//!   and every float matches to 1e-9 relative;
//! - `quantise_floats`: where a line is a digest and cannot be compared numerically, floats are
//!   rounded to 6 significant digits before hashing, so the digest is the same on both.
//!
//! A float is a number with a decimal point or an exponent; integers, hex digests and words are
//! compared exactly.

use crate::controller::milp_planner::model_probe::recorder::{quantise_f64, FLOAT_NOISE};

#[derive(Debug, PartialEq)]
enum Tok<'a> {
    Text(&'a str),
    Int(&'a str),
    Float(f64),
}

fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn skip_digits(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    i
}

/// Split into text, integers and floats. A number glued to letters (`18fc44…`, `x2`) is part of
/// a word, not a number.
fn tokens(s: &str) -> Vec<Tok<'_>> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let (mut i, mut text_from) = (0, 0);
    while i < b.len() {
        let negative = b[i] == b'-' && i + 1 < b.len() && b[i + 1].is_ascii_digit();
        let starts = (b[i].is_ascii_digit() || negative) && (i == 0 || !is_word(b[i - 1]));
        if !starts {
            i += 1;
            continue;
        }
        let mut j = skip_digits(b, i + usize::from(negative));
        let mut is_float = false;
        if j + 1 < b.len() && b[j] == b'.' && b[j + 1].is_ascii_digit() {
            j = skip_digits(b, j + 1);
            is_float = true;
        }
        if j < b.len() && (b[j] == b'e' || b[j] == b'E') {
            let sign = usize::from(j + 1 < b.len() && (b[j + 1] == b'+' || b[j + 1] == b'-'));
            let k = skip_digits(b, j + 1 + sign);
            if k > j + 1 + sign {
                j = k;
                is_float = true;
            }
        }
        if j < b.len() && is_word(b[j]) {
            while j < b.len() && is_word(b[j]) {
                j += 1;
            }
            i = j;
            continue;
        }
        if text_from < i {
            out.push(Tok::Text(&s[text_from..i]));
        }
        let number = &s[i..j];
        out.push(match number.parse::<f64>() {
            Ok(v) if is_float => Tok::Float(v),
            _ => Tok::Int(number),
        });
        i = j;
        text_from = j;
    }
    if text_from < b.len() {
        out.push(Tok::Text(&s[text_from..]));
    }
    out
}

/// Whether two recorded lines say the same thing, floats compared to `FLOAT_NOISE`.
pub(super) fn same_within_float_noise(got: &str, want: &str) -> bool {
    if got == want {
        return true;
    }
    let (a, b) = (tokens(got), tokens(want));
    a.len() == b.len()
        && a.iter().zip(&b).all(|pair| match pair {
            (Tok::Float(x), Tok::Float(y)) => {
                (x - y).abs() <= FLOAT_NOISE * x.abs().max(y.abs()) + FLOAT_NOISE
            }
            (x, y) => x == y,
        })
}

/// The text with every float rounded as `quantise_f64` does (6 significant digits, dust as 0),
/// for hashing a result that must agree across architectures.
pub(super) fn quantise_floats(text: &str) -> String {
    tokens(text)
        .iter()
        .map(|t| match t {
            Tok::Text(s) | Tok::Int(s) => (*s).to_string(),
            Tok::Float(v) => quantise_f64(*v),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_within_float_noise_accepts_the_last_digits_differing() {
        assert!(same_within_float_noise(
            r#"cost_breakdown {"c_energy_eur":2.7550990207796175,"c_wear_eur":0.1517437379580934}"#,
            r#"cost_breakdown {"c_energy_eur":2.7550990207796886,"c_wear_eur":0.15174373795807775}"#,
        ));
    }

    #[test]
    fn same_within_float_noise_rejects_a_visible_change() {
        assert!(!same_within_float_noise("cost 2.75", "cost 2.76"));
    }

    #[test]
    fn same_within_float_noise_compares_integers_exactly() {
        assert!(!same_within_float_noise("slot 3", "slot 4"));
        assert!(!same_within_float_noise("terms=299", "terms=300"));
    }

    #[test]
    fn same_within_float_noise_compares_text_and_words_exactly() {
        assert!(!same_within_float_noise("asset ev 1.0", "asset pv 1.0"));
        // A digest that happens to look like an exponent is a word, not a number.
        assert!(!same_within_float_noise("sorted=3e5a", "sorted=3e5b"));
        assert!(!same_within_float_noise("a 1.0", "a 1.0 b"));
    }

    #[test]
    fn same_within_float_noise_treats_signed_zero_and_dust_as_zero() {
        assert!(same_within_float_noise("peak -0.0", "peak 0.0"));
        assert!(same_within_float_noise("violation 1.2e-15", "violation 0.0"));
    }

    #[test]
    fn same_within_float_noise_reads_exponents() {
        assert!(same_within_float_noise("x 1.5e3", "x 1500.0"));
        assert!(!same_within_float_noise("x 1.5e3", "x 1600.0"));
    }

    #[test]
    fn quantise_floats_agrees_for_noise_and_differs_for_a_change() {
        let a = quantise_floats("[2.7550990207796175, -0.0, 3]");
        assert_eq!(a, quantise_floats("[2.7550990207796886, 0.0, 3]"));
        assert_ne!(a, quantise_floats("[2.76, 0.0, 3]"));
        assert_ne!(a, quantise_floats("[2.7550990207796175, 0.0, 4]"));
    }

    #[test]
    fn quantise_floats_leaves_words_and_integers_alone() {
        assert_eq!(quantise_floats("Optimal|x2|18fc44"), "Optimal|x2|18fc44");
    }
}
