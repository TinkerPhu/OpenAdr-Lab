//! Test-only helper for inspecting what an asset declares: how far each variable can move.

use good_lp::solvers::highs::highs;
use good_lp::{Expression, ProblemVariables, Solution, SolverModel, Variable};

/// How far each of the variables `declare` returns can move on its own bounds:
/// `(lowest, highest)` per variable, found by minimising and maximising their sum.
/// A decision pinned to a value reads back as `(value, value)`.
///
/// `declare` is called twice (once per direction), each time on fresh variables.
pub fn bound_range(declare: impl Fn(&mut ProblemVariables) -> Vec<Variable>) -> Vec<(f64, f64)> {
    let extreme = |maximise: bool| -> Vec<f64> {
        let mut vars = good_lp::variables!();
        let watched = declare(&mut vars);
        let sum: Expression = watched.iter().copied().map(Expression::from).sum();
        let problem = if maximise {
            vars.maximise(sum)
        } else {
            vars.minimise(sum)
        };
        let solution = problem
            .using(highs)
            .solve()
            .expect("bounds-only problem solves");
        watched.iter().map(|&v| solution.value(v)).collect()
    };
    let (low, high) = (extreme(false), extreme(true));
    low.into_iter().zip(high).collect()
}
