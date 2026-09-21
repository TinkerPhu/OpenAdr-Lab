//! Getting a database connection, for tasks that must not fail without one.
//!
//! Both background writers — the VTN recorder and the fleet telemetry store —
//! need the same thing: connect, create their tables, and if either step fails,
//! keep trying rather than disabling themselves for the life of the process.
//! That second half is not a generic nicety, it is the 2026-08-10 incident: a
//! single failed startup connection left the recorder silently dead for nine
//! days.
//!
//! One implementation so the next writer inherits the lesson instead of
//! re-learning it (`one-concept-one-function`). What differs between callers —
//! which tables to create, where a failure is reported — arrives as a
//! parameter.

use std::future::Future;
use std::time::Duration;

use anyhow::Result;
use sqlx::PgPool;
use tracing::error;

const INITIAL_BACKOFF_S: u64 = 5;
const MAX_BACKOFF_S: u64 = 300;

/// Double until the ceiling: quick enough for a database that is merely
/// starting, patient enough not to hammer one that is down.
pub fn next_backoff(current: Duration) -> Duration {
    (current * 2).min(Duration::from_secs(MAX_BACKOFF_S))
}

/// Connect, initialise, and do not give up.
///
/// `init` is retried along with the connection, because a pool that connects
/// to a database whose tables were never created is not usable either — the
/// two succeed together or the attempt failed.
///
/// `on_error` is async so a caller can record the failure wherever its health
/// surface lives; it is called once per failed attempt, not once overall.
pub async fn connect_and_init_with_retry<Init, InitFut, OnErr, OnErrFut>(
    database_url: &str,
    label: &str,
    init: Init,
    mut on_error: OnErr,
) -> PgPool
where
    Init: Fn(PgPool) -> InitFut,
    InitFut: Future<Output = Result<PgPool>>,
    OnErr: FnMut(String) -> OnErrFut,
    OnErrFut: Future<Output = ()>,
{
    let mut backoff = Duration::from_secs(INITIAL_BACKOFF_S);
    loop {
        let attempt = async {
            let pool = PgPool::connect(database_url).await?;
            init(pool).await
        };
        match attempt.await {
            Ok(pool) => return pool,
            Err(e) => {
                let msg = format!("{e:#}");
                error!("{label}: connect/init failed, retrying in {backoff:?}: {msg}");
                on_error(msg).await;
                tokio::time::sleep(backoff).await;
                backoff = next_backoff(backoff);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_up_to_the_ceiling() {
        assert_eq!(
            next_backoff(Duration::from_secs(5)),
            Duration::from_secs(10)
        );
        assert_eq!(
            next_backoff(Duration::from_secs(200)),
            Duration::from_secs(MAX_BACKOFF_S)
        );
        // Already at the ceiling: stays there rather than overflowing past it.
        assert_eq!(
            next_backoff(Duration::from_secs(MAX_BACKOFF_S)),
            Duration::from_secs(MAX_BACKOFF_S)
        );
    }
}
