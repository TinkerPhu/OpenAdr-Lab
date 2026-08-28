//! plan_history persistence (GB-25), split from `mod.rs` for the 500-production-line
//! file-size cap. Called only via the `HistoryPort` impl on `SqliteHistoryStore`. See
//! `docs/architecture/VEN_ARCHITECTURE.md` §4.9a.

use std::str::FromStr;

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};

use super::{from_unix, to_unix};
use crate::entities::history::PlanHistorySample;
use crate::entities::plan::{SolveStatus, WarningKind};
use crate::entities::DomainError;

/// `WarningKind` values as stored in the comma-joined `warning_kinds` TEXT column —
/// SCREAMING_SNAKE_CASE, matching the enum's `#[serde(rename_all = ...)]` so the stored
/// text is consistent with anything serialized to JSON elsewhere.
fn warning_kind_str(kind: WarningKind) -> &'static str {
    match kind {
        WarningKind::SolverInfeasible => "SOLVER_INFEASIBLE",
        WarningKind::StaleRateEstimate => "STALE_RATE_ESTIMATE",
        WarningKind::BudgetShortfall => "BUDGET_SHORTFALL",
        WarningKind::CapacityViolation => "CAPACITY_VIOLATION",
        WarningKind::PeakPenaltyExceeded => "PEAK_PENALTY_EXCEEDED",
        WarningKind::EvCoreEnergyUnmet => "EV_CORE_ENERGY_UNMET",
        WarningKind::Other => "OTHER",
    }
}

fn parse_warning_kind(s: &str) -> Result<WarningKind, DomainError> {
    match s {
        "SOLVER_INFEASIBLE" => Ok(WarningKind::SolverInfeasible),
        "STALE_RATE_ESTIMATE" => Ok(WarningKind::StaleRateEstimate),
        "BUDGET_SHORTFALL" => Ok(WarningKind::BudgetShortfall),
        "CAPACITY_VIOLATION" => Ok(WarningKind::CapacityViolation),
        "PEAK_PENALTY_EXCEEDED" => Ok(WarningKind::PeakPenaltyExceeded),
        "EV_CORE_ENERGY_UNMET" => Ok(WarningKind::EvCoreEnergyUnmet),
        "OTHER" => Ok(WarningKind::Other),
        other => Err(DomainError::StorageError(format!(
            "invalid stored warning_kind: {other}"
        ))),
    }
}

fn join_kinds(kinds: &[WarningKind]) -> String {
    kinds
        .iter()
        .map(|k| warning_kind_str(*k))
        .collect::<Vec<_>>()
        .join(",")
}

fn split_kinds(s: &str) -> Result<Vec<WarningKind>, DomainError> {
    if s.is_empty() {
        return Ok(Vec::new());
    }
    s.split(',').map(parse_warning_kind).collect()
}

fn solve_status_str(status: SolveStatus) -> &'static str {
    match status {
        SolveStatus::Optimal => "OPTIMAL",
        SolveStatus::TimeLimit => "TIME_LIMIT",
        SolveStatus::GapLimit => "GAP_LIMIT",
        SolveStatus::Infeasible => "INFEASIBLE",
    }
}

fn parse_solve_status(s: &str) -> Result<SolveStatus, DomainError> {
    match s {
        "OPTIMAL" => Ok(SolveStatus::Optimal),
        "TIME_LIMIT" => Ok(SolveStatus::TimeLimit),
        "GAP_LIMIT" => Ok(SolveStatus::GapLimit),
        "INFEASIBLE" => Ok(SolveStatus::Infeasible),
        other => Err(DomainError::StorageError(format!(
            "invalid stored solve_status: {other}"
        ))),
    }
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
                solve_status_str(row.solve_status),
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
                    solve_status: parse_solve_status(&solve_status)?,
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
