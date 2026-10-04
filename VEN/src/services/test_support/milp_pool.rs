//! Test-only helpers for declaring into, and inspecting, a MILP variable pool.

use good_lp::solvers::highs::highs;
use good_lp::{variable, Expression, ProblemVariables, Solution, SolverModel, Variable};

use crate::controller::milp_interactions::{GridMilpVars, MilpVarPool};

/// A pool holding only the grid variables, so an asset test can declare its own
/// variables into it. (Was copied verbatim into the battery, EV and heater test modules.)
pub fn empty_pool(vars: &mut ProblemVariables, n: usize) -> MilpVarPool {
    let grid = GridMilpVars {
        p_imp: (0..n).map(|_| vars.add(variable().min(0.0))).collect(),
        p_exp: (0..n).map(|_| vars.add(variable().min(0.0))).collect(),
        u_grid: (0..n).map(|_| vars.add(variable().binary())).collect(),
        s_imp_viol: (0..n).map(|_| vars.add(variable().min(0.0))).collect(),
        s_exp_viol: (0..n).map(|_| vars.add(variable().min(0.0))).collect(),
        p_pv_used: (0..n).map(|_| vars.add(variable().min(0.0))).collect(),
    };
    MilpVarPool {
        grid,
        bat: None,
        ev: None,
        heater: None,
        shiftable: vec![],
    }
}

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
