use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum DomainError {
    #[error("session conflict: {0}")]
    SessionConflict(String),

    #[error("not found: {id}")]
    NotFound { id: Uuid },

    #[error("plan infeasible: {0}")]
    PlanInfeasible(String),

    #[error("VTN unreachable: {0}")]
    VtnUnreachable(String),

    #[error("profile invalid: {0}")]
    ProfileInvalid(String),
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
        ];
        for e in cases {
            assert!(!e.to_string().is_empty(), "variant {e:?} has empty Display");
        }
    }
}
