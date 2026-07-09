use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum DomainError {
    #[error("session conflict: {0}")]
    SessionConflict(String),

    #[error("not found: {id}")]
    NotFound { id: Uuid },

    /// Reserved — not yet constructed at a real error boundary. Intended for the
    /// planner's solve failure path. See docs/BACKLOG.md BL-25.
    #[error("plan infeasible: {0}")]
    PlanInfeasible(String),

    /// Reserved — not yet constructed at a real error boundary. Intended for VTN-client
    /// repeated-timeout classification, distinct from a generic error. See
    /// docs/BACKLOG.md BL-25.
    #[error("VTN unreachable: {0}")]
    VtnUnreachable(String),

    /// Reserved — not yet constructed at a real error boundary. Intended for profile
    /// hot-reload validation, if that feature is ever built. See docs/BACKLOG.md BL-25.
    #[error("profile invalid: {0}")]
    ProfileInvalid(String),

    /// History store (SQLite) I/O or migration failure. Phase 1 (A-1).
    #[error("storage error: {0}")]
    StorageError(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_variants_have_non_empty_display() {
        let id = Uuid::nil();
        let cases: &[DomainError] = &[
            DomainError::SessionConflict("already active".into()),
            DomainError::NotFound { id },
            DomainError::PlanInfeasible("infeasible".into()),
            DomainError::VtnUnreachable("timeout".into()),
            DomainError::ProfileInvalid("bad value".into()),
            DomainError::StorageError("disk full".into()),
        ];
        for e in cases {
            assert!(!e.to_string().is_empty(), "variant {e:?} has empty Display");
        }
    }
}
