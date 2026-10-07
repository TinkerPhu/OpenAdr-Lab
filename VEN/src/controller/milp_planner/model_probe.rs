//! A record of the exact model each solve hands to HiGHS, for tests.
//!
//! The planner builds three models per cycle (phase 1, phase 2, the marginal-cost pass) that must
//! stay the same model while the code that builds them is restructured: the same variables in the
//! same order, the same objective, the same constraints. `good_lp` keeps coefficients in an
//! `FnvHashMap`, so what HiGHS receives is a pure function of the order operations were applied in,
//! and a refactor that is correct but reorders them can still change the solver's path.
//!
//! `probe!` marks the points where a model is complete. Outside tests it expands to nothing; inside
//! tests it appends a short canonical record to a thread-local sink, and
//! `tests/model_fingerprint.rs` compares those records with a committed golden. Each expression is
//! recorded twice: *sorted* (what it means) and *as sent* (the iteration order the solver sees), so
//! a difference that is only an ordering difference is named as one.

#[cfg(test)]
macro_rules! probe {
    ($kind:ident, $($arg:expr),+ $(,)?) => {
        $crate::controller::milp_planner::model_probe::recorder::$kind($($arg),+)
    };
}
#[cfg(not(test))]
macro_rules! probe {
    ($($t:tt)*) => {};
}

#[cfg(test)]
pub(crate) mod recorder {
    use std::cell::RefCell;
    use std::fmt::Write as _;

    use good_lp::{Constraint, Expression, IntoAffineExpression, ProblemVariables, Variable};

    thread_local! {
        static SINK: RefCell<Option<Vec<String>>> = const { RefCell::new(None) };
    }

    /// Run `f` and return its result with every record made on this thread meanwhile.
    pub(crate) fn capture<R>(f: impl FnOnce() -> R) -> (R, Vec<String>) {
        SINK.with(|s| *s.borrow_mut() = Some(Vec::new()));
        let result = f();
        let lines = SINK.with(|s| s.borrow_mut().take()).unwrap_or_default();
        (result, lines)
    }

    fn push(line: String) {
        SINK.with(|s| {
            if let Some(lines) = s.borrow_mut().as_mut() {
                lines.push(line);
            }
        });
    }

    /// FNV-1a, 64 bit: stable across Rust releases, unlike `std`'s `DefaultHasher`.
    pub(crate) fn digest(text: &str) -> String {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in text.bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        format!("{h:016x}")
    }

    /// `Variable`'s index is private; its `Debug` output is `Variable { index: N }`.
    fn index_of(v: Variable) -> usize {
        format!("{v:?}")
            .chars()
            .filter(char::is_ascii_digit)
            .collect::<String>()
            .parse()
            .expect("a good_lp Variable's Debug output carries its index")
    }

    /// `(sorted digest, as-sent digest)` of `(variable, bits)` pairs plus a constant.
    fn digests(terms: Vec<(usize, u64)>, constant_bits: u64) -> (String, String) {
        let text = |t: &[(usize, u64)]| {
            let mut s = format!("c{constant_bits:x}");
            for (i, bits) in t {
                let _ = write!(s, ";{i}:{bits:x}");
            }
            s
        };
        let mut sorted = terms.clone();
        sorted.sort_unstable();
        (digest(&text(&sorted)), digest(&text(&terms)))
    }

    fn expression_digests(e: &Expression) -> (usize, String, String) {
        let terms: Vec<(usize, u64)> = e
            .linear_coefficients()
            .map(|(v, c)| (index_of(v), c.to_bits()))
            .collect();
        let n = terms.len();
        let (sorted, sent) = digests(terms, IntoAffineExpression::constant(e).to_bits());
        (n, sorted, sent)
    }

    /// Mark the start of one solve.
    pub(crate) fn begin(label: &str) {
        push(format!("== {label}"));
    }

    /// Every variable's definition (bounds, integrality), in declaration order.
    pub(crate) fn vars(v: &ProblemVariables) {
        let mut s = String::new();
        for (var, def) in v.iter_variables_with_def() {
            let _ = writeln!(s, "{} {def:?}", index_of(var));
        }
        push(format!("vars n={} {}", v.len(), digest(&s)));
    }

    pub(crate) fn expr(label: &str, e: &Expression) {
        let (n, sorted, sent) = expression_digests(e);
        push(format!(
            "expr {label} terms={n} sorted={sorted} sent={sent}"
        ));
    }

    pub(crate) fn constraint(c: &Constraint) {
        let (n, sorted, sent) = expression_digests(c.expression());
        push(format!(
            "con eq={} terms={n} sorted={sorted} sent={sent}",
            c.is_equality()
        ));
    }

    /// The initial MIP incumbent handed to HiGHS.
    pub(crate) fn warm_start(ws: &[(Variable, f64)]) {
        let terms: Vec<(usize, u64)> = ws
            .iter()
            .map(|(v, x)| (index_of(*v), x.to_bits()))
            .collect();
        let n = terms.len();
        let (sorted, sent) = digests(terms, 0);
        push(format!("warm terms={n} sorted={sorted} sent={sent}"));
    }
}
