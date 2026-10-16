//! plan_history persistence (GB-25), split from `mod.rs` for the 500-production-line
//! file-size cap. Called only via the `HistoryPort` impl on `SqliteHistoryStore`. See
//! `docs/architecture/VEN_ARCHITECTURE.md` §4.9a.

use std::str::FromStr;

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};

use super::{from_unix, to_unix};
use crate::entities::history::PlanHistorySample;
use crate::entities::plan::WarningKind;
use crate::entities::DomainError;

/// A stored enum value: the type's own serde spelling (SCREAMING_SNAKE_CASE for
/// `WarningKind` and `SolveStatus`), so the column holds exactly what JSON carries everywhere
/// else and a new variant needs no second spelling here.
fn stored<T: serde::Serialize>(value: T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(s)) => s,
        other => unreachable!("a unit enum serializes to a string, got {other:?}"),
    }
}

fn parse_stored<T: serde::de::DeserializeOwned>(s: &str, column: &str) -> Result<T, DomainError> {
    serde_json::from_value(serde_json::Value::String(s.to_string()))
        .map_err(|_| DomainError::StorageError(format!("invalid stored {column}: {s}")))
}

fn join_kinds(kinds: &[WarningKind]) -> String {
    kinds
        .iter()
        .map(|k| stored(*k))
        .collect::<Vec<_>>()
        .join(",")
}

fn split_kinds(s: &str) -> Result<Vec<WarningKind>, DomainError> {
    if s.is_empty() {
        return Ok(Vec::new());
    }
    s.split(',')
        .map(|k| parse_stored(k, "warning_kind"))
        .collect()
}

pub(super) fn append(conn: &mut Connection, rows: &[PlanHistorySample]) -> Result<(), DomainError> {
    let tx = conn
        .transaction()
        .map_err(|e| DomainError::StorageError(format!("begin tx: {e}")))?;
    {
        let mut stmt = tx
            .prepare(
                "INSERT INTO plan_history
                    (plan_id, created_at, trigger, solver_ms, solve_status, objective_eur,
                     friction_eur, mip_gap_target, warning_count, warning_kinds,
                     c_energy_eur, c_grid_eur, c_wear_eur, c_violations_eur, c_peak_penalty_eur)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            )
            .map_err(|e| DomainError::StorageError(format!("prepare insert: {e}")))?;
        for row in rows {
            stmt.execute(params![
                row.plan_id.to_string(),
                to_unix(row.created_at),
                row.trigger,
                row.solver_ms,
                stored(row.solve_status),
                row.objective_eur,
                row.friction_eur,
                row.mip_gap_target,
                row.warning_count,
                join_kinds(&row.warning_kinds),
                row.c_energy_eur,
                row.c_grid_eur,
                row.c_wear_eur,
                row.c_violations_eur,
                row.c_peak_penalty_eur,
            ])
            .map_err(|e| DomainError::StorageError(format!("insert plan history row: {e}")))?;
        }
    }
    tx.commit()
        .map_err(|e| DomainError::StorageError(format!("commit tx: {e}")))?;
    Ok(())
}

#[allow(clippy::type_complexity)]
type Row = (
    String,
    i64,
    String,
    Option<i64>,
    String,
    f64,
    f64,
    Option<f64>,
    u32,
    String,
    Option<f64>,
    Option<f64>,
    Option<f64>,
    Option<f64>,
    Option<f64>,
);

pub(super) fn query(
    conn: &Connection,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> Result<Vec<PlanHistorySample>, DomainError> {
    let mut stmt = conn
        .prepare(
            "SELECT plan_id, created_at, trigger, solver_ms, solve_status, objective_eur,
                    friction_eur, mip_gap_target, warning_count, warning_kinds,
                    c_energy_eur, c_grid_eur, c_wear_eur, c_violations_eur, c_peak_penalty_eur
             FROM plan_history
             WHERE created_at >= ?1 AND created_at < ?2
             ORDER BY created_at ASC",
        )
        .map_err(|e| DomainError::StorageError(format!("prepare query_plan_history: {e}")))?;
    let raw: Vec<Row> = stmt
        .query_map(params![to_unix(from), to_unix(to)], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
                row.get(8)?,
                row.get(9)?,
                row.get(10)?,
                row.get(11)?,
                row.get(12)?,
                row.get(13)?,
                row.get(14)?,
            ))
        })
        .map_err(|e| DomainError::StorageError(format!("query_plan_history: {e}")))?
        .collect::<Result<_, _>>()
        .map_err(|e| DomainError::StorageError(format!("read query_plan_history rows: {e}")))?;

    raw.into_iter()
        .map(
            |(
                plan_id,
                created_at,
                trigger,
                solver_ms,
                solve_status,
                objective_eur,
                friction_eur,
                mip_gap_target,
                warning_count,
                warning_kinds,
                c_energy_eur,
                c_grid_eur,
                c_wear_eur,
                c_violations_eur,
                c_peak_penalty_eur,
            )| {
                Ok(PlanHistorySample {
                    plan_id: uuid::Uuid::from_str(&plan_id).map_err(|e| {
                        DomainError::StorageError(format!("invalid stored plan_id: {e}"))
                    })?,
                    created_at: from_unix(created_at)?,
                    trigger,
                    solver_ms: solver_ms.map(|v| v as u64),
                    solve_status: parse_stored(&solve_status, "solve_status")?,
                    objective_eur,
                    friction_eur,
                    mip_gap_target,
                    warning_count,
                    warning_kinds: split_kinds(&warning_kinds)?,
                    c_energy_eur,
                    c_grid_eur,
                    c_wear_eur,
                    c_violations_eur,
                    c_peak_penalty_eur,
                })
            },
        )
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{parse_stored, stored};
    use crate::entities::plan::{SolveStatus, WarningKind};

    /// The column keeps the spelling it had when each value was written by hand, so rows
    /// already on disk still read back.
    #[test]
    fn stored_spellings_are_the_ones_already_on_disk() {
        let statuses = [
            (SolveStatus::Optimal, "OPTIMAL"),
            (SolveStatus::TimeLimit, "TIME_LIMIT"),
            (SolveStatus::GapLimit, "GAP_LIMIT"),
            (SolveStatus::Infeasible, "INFEASIBLE"),
            (SolveStatus::Simulated, "SIMULATED"),
        ];
        for (status, text) in statuses {
            assert_eq!(stored(status), text);
            assert_eq!(
                parse_stored::<SolveStatus>(text, "solve_status").unwrap(),
                status
            );
        }
        let kinds = [
            (WarningKind::SolverInfeasible, "SOLVER_INFEASIBLE"),
            (WarningKind::StaleRateEstimate, "STALE_RATE_ESTIMATE"),
            (WarningKind::BudgetShortfall, "BUDGET_SHORTFALL"),
            (WarningKind::CapacityViolation, "CAPACITY_VIOLATION"),
            (WarningKind::PeakPenaltyExceeded, "PEAK_PENALTY_EXCEEDED"),
            (WarningKind::EvCoreEnergyUnmet, "EV_CORE_ENERGY_UNMET"),
            (WarningKind::TargetAboveLimit, "TARGET_ABOVE_LIMIT"),
            (
                WarningKind::OpportunisticNoForecast,
                "OPPORTUNISTIC_NO_FORECAST",
            ),
            (WarningKind::Other, "OTHER"),
        ];
        for (kind, text) in kinds {
            assert_eq!(stored(kind), text);
            assert_eq!(
                parse_stored::<WarningKind>(text, "warning_kind").unwrap(),
                kind
            );
        }
        assert!(parse_stored::<SolveStatus>("NOPE", "solve_status").is_err());
    }
}
