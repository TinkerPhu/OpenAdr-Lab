//! The VEN's HTTP client for the VTN, behind `VtnPort`.
//!
//! Owns how the VTN's API is spoken: paths, pagination, and translating a non-2xx answer into
//! `DomainError::VtnRejected`. Reads the VTN only. The token it authenticates with is
//! `lab_core::vtn_auth`'s; this file must never fetch, cache or retry one itself (R-135).

use anyhow::{Context, Result};
use async_trait::async_trait;
use lab_core::vtn_auth::{ClientCredentials, VtnAuthError};
use reqwest::{Method, StatusCode};
use tracing::warn;

// WP2.2 (Phase 2): openleadr-rs collection GETs cap at 50/page regardless of
// a larger requested `limit` — match that cap so every page is full until
// the last one.
const PAGE_LIMIT: usize = 50;
// Runaway-poll guard: a well-behaved poll should never need this many pages
// for programs/events/reports on a single VEN; log if it does rather than
// looping forever on a misbehaving or huge-collection VTN.
const MAX_PAGES_WARNING: u32 = 20;
use serde::Deserialize;
use std::sync::{Arc, PoisonError};

use crate::controller::vtn_port::{OadrEvent, OadrProgram, OadrReport, OadrReportBody, VtnPort};
use crate::controller::wire_reject::{partition_valid, FetchOutcome};
use crate::entities::DomainError;

#[derive(Clone)]
pub struct VtnClient {
    http: reqwest::Client,
    base_url: String,
    ven_name: String,
    /// The token, and when it is fetched again.
    credentials: ClientCredentials,
    /// GB-49: set when the last token lacked VEN scopes. Read through
    /// `VtnPort::scope_warning` so the health route can surface it.
    scope_warning: Arc<std::sync::RwLock<Option<String>>>,
    /// F-6: `reportName` -> the VTN's id for it, learned from the first
    /// create. Steady-state submission is a PUT straight to that id instead of
    /// POST -> 409 -> paginated name lookup -> PUT. Purely an optimisation:
    /// every entry is re-derivable, and a wrong one self-heals (see
    /// `upsert_report`).
    pub(crate) report_ids: Arc<tokio::sync::RwLock<std::collections::HashMap<String, String>>>,
}

/// RFC 7807 "Problem Details for HTTP APIs" — openleadr-rs's error response
/// shape (WP2.3). All fields optional per the RFC; a body that doesn't parse
/// as this shape at all just falls back to the raw text.
#[derive(Debug, Deserialize)]
struct ProblemDetails {
    #[serde(rename = "type")]
    problem_type: Option<String>,
    title: Option<String>,
    status: Option<i64>,
    detail: Option<String>,
    instance: Option<String>,
}

/// Classify a failed HTTP `send()` as VTN-unreachable (BL-25) when it's a
/// connect- or timeout-class failure — distinct from an HTTP-level error
/// response, which always has a status code. Used only to log a typed,
/// structured `DomainError::VtnUnreachable` for fleet debugging; the
/// `anyhow::Result` propagated to callers is unchanged (`VtnPort`'s contract
/// stays a plain `Result<T, anyhow::Error>`).
fn classify_reqwest_error(e: &reqwest::Error) -> Option<crate::entities::DomainError> {
    if e.is_connect() || e.is_timeout() {
        Some(crate::entities::DomainError::VtnUnreachable(e.to_string()))
    } else {
        None
    }
}

/// Log a structured `DomainError::VtnUnreachable` line if `e` classifies as
/// one; a no-op for any other `send()` failure (DNS, TLS, etc. still surface
/// via the caller's `.context(...)` message as before).
fn log_if_vtn_unreachable(path: &str, e: &VtnAuthError) {
    // The request that failed is the token request when no token could be had for `path`.
    let path = if e.is_token_failure() {
        "/auth/token"
    } else {
        path
    };
    if let Some(domain_err) = e.transport().and_then(classify_reqwest_error) {
        tracing::error!(path, error = %domain_err, "VTN unreachable");
    }
}

/// The JSON body of a VTN answer, or the rejection it is when the status is not 2xx.
async fn json_or_rejection(path: &str, resp: reqwest::Response) -> Result<serde_json::Value> {
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(http_error(path, status, &body));
    }
    Ok(resp.json().await?)
}

/// Parse a non-2xx VTN response body into a human-readable problem message,
/// without logging: RFC 7807 title/detail if the body is problem+json shaped,
/// or the raw body otherwise. Used where the caller doesn't yet know whether
/// the response is a terminal failure (e.g. `upsert_report`'s speculative
/// 409, which may still resolve via name-based upsert) — logging here would
/// fire on every recoverable case, not just real failures.
pub(crate) fn describe_problem(body: &str) -> String {
    match serde_json::from_str::<ProblemDetails>(body) {
        Ok(problem) if problem.title.is_some() || problem.detail.is_some() => {
            format!(
                "{} — {}",
                problem.title.unwrap_or_default(),
                problem.detail.unwrap_or_default()
            )
        }
        _ => body.to_string(),
    }
}

/// Build the error for a non-2xx VTN response — translated here, at the HTTP boundary, to
/// `DomainError::VtnRejected` carrying the status (GB-23, R-131) — logging a structured ERROR
/// line either way: as parsed RFC 7807 fields if the body is problem+json
/// shaped, or the raw body otherwise. Called at every terminal
/// "not `is_success()`" branch in this client so no call site has to
/// duplicate the parse-or-fallback logic — but NOT for a status a caller may
/// still recover from (see `describe_problem` for that case).
pub(crate) fn http_error(path: &str, status: StatusCode, body: &str) -> anyhow::Error {
    match serde_json::from_str::<ProblemDetails>(body) {
        Ok(problem) if problem.title.is_some() || problem.detail.is_some() => {
            tracing::error!(
                path,
                status = status.as_u16(),
                problem_status = problem.status.unwrap_or(-1),
                problem_type = problem.problem_type.as_deref().unwrap_or(""),
                problem_title = problem.title.as_deref().unwrap_or(""),
                problem_detail = problem.detail.as_deref().unwrap_or(""),
                problem_instance = problem.instance.as_deref().unwrap_or(""),
                "VTN returned an RFC 7807 problem response"
            );
            anyhow::Error::new(DomainError::VtnRejected {
                status: status.as_u16(),
                message: format!(
                    "{path} returned {status}: {} — {}",
                    problem.title.unwrap_or_default(),
                    problem.detail.unwrap_or_default()
                ),
            })
        }
        _ => {
            tracing::error!(
                path,
                status = status.as_u16(),
                body,
                "VTN returned a non-2xx response"
            );
            anyhow::Error::new(DomainError::VtnRejected {
                status: status.as_u16(),
                message: format!("{path} returned {status}: {body}"),
            })
        }
    }
}

impl VtnClient {
    pub fn new(
        base_url: String,
        client_id: String,
        client_secret: String,
        ven_name: String,
    ) -> Self {
        let http = reqwest::Client::new();
        let scope_warning = Arc::new(std::sync::RwLock::new(None));
        let warning_sink = scope_warning.clone();
        // GB-49: a VEN granted no scopes authenticates fine and then polls an
        // empty world -- every request 200, every list empty, nothing saying
        // why. Read our own grant rather than infer it from silence.
        let credentials = ClientCredentials::new(http.clone(), &base_url, client_id, client_secret)
            .on_new_token(move |access_token| {
                let missing = crate::controller::token_scopes::missing_ven_scopes(access_token);
                let warning = (!missing.is_empty())
                    .then(|| crate::controller::token_scopes::describe_missing(&missing));
                if let Some(warning) = &warning {
                    warn!(missing = ?missing, "{warning}");
                }
                *warning_sink.write().unwrap_or_else(PoisonError::into_inner) = warning;
            });
        Self {
            http,
            base_url,
            ven_name,
            credentials,
            scope_warning,
            report_ids: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
        }
    }

    /// Send one authorized request to `path` and return whatever the VTN answers. The token,
    /// and the one retry after a 401 or 403, are `ClientCredentials::send`'s.
    async fn send(
        &self,
        method: Method,
        path: &str,
        body: Option<&serde_json::Value>,
    ) -> Result<reqwest::Response> {
        let url = format!("{}{}", self.base_url.trim_end_matches('/'), path);
        self.credentials
            .send(|| {
                let request = self.http.request(method.clone(), &url);
                match body {
                    Some(body) => request.json(body),
                    None => request,
                }
            })
            .await
            .inspect_err(|e| log_if_vtn_unreachable(path, e))
            .with_context(|| format!("{method} {path} failed"))
    }

    async fn get_json(&self, path: &str) -> Result<serde_json::Value> {
        let resp = self.send(Method::GET, path, None).await?;
        json_or_rejection(path, resp).await
    }

    /// GET every page of a collection endpoint via `skip`/`limit`, stopping
    /// once a page returns fewer than `PAGE_LIMIT` items. `base_path` may
    /// already contain a query string (e.g. `/events?active=true`).
    async fn get_json_paginated(&self, base_path: &str) -> Result<Vec<serde_json::Value>> {
        let mut all = Vec::new();
        let mut skip = 0usize;
        let mut pages = 0u32;
        let sep = if base_path.contains('?') { '&' } else { '?' };
        loop {
            let page_path = format!("{base_path}{sep}skip={skip}&limit={PAGE_LIMIT}");
            let raw = self.get_json(&page_path).await?;
            let page = raw.as_array().cloned().unwrap_or_default();
            let n = page.len();
            all.extend(page);
            pages += 1;
            if pages == MAX_PAGES_WARNING {
                tracing::warn!(
                    base_path,
                    pages,
                    "paginated GET has fetched an unusually large number of pages"
                );
            }
            if n < PAGE_LIMIT {
                break;
            }
            skip += PAGE_LIMIT;
        }
        Ok(all)
    }

    pub(crate) async fn put_json(
        &self,
        path: &str,
        body: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let resp = self.send(Method::PUT, path, Some(&body)).await?;
        json_or_rejection(path, resp).await
    }

    /// POST JSON, returning the raw response (status + body) without error-mapping.
    pub(crate) async fn post_json_raw(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<(StatusCode, String)> {
        let resp = self.send(Method::POST, path, Some(body)).await?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        Ok((status, text))
    }
}

// ── VtnPort implementation ────────────────────────────────────────────────────

#[async_trait]
impl VtnPort for VtnClient {
    async fn fetch_programs(&self) -> Result<FetchOutcome<OadrProgram>> {
        let items = self.get_json_paginated("/programs").await?;
        Ok(partition_valid(&items))
    }

    async fn fetch_events(&self) -> Result<FetchOutcome<OadrEvent>> {
        let items = self.get_json_paginated("/events?active=true").await?;
        crate::controller::wire_reject::shadow_parse_events(&items);
        Ok(partition_valid(&items))
    }

    async fn fetch_reports(&self) -> Result<FetchOutcome<OadrReport>> {
        let path = format!("/reports?clientName={}", self.ven_name);
        let items = self.get_json_paginated(&path).await?;
        Ok(partition_valid(&items))
    }

    async fn upsert_report(&self, body: OadrReportBody) -> Result<()> {
        // Delegates to the inherent method which handles 409 upsert semantics.
        self.upsert_report(body).await
    }

    async fn update_report(&self, id: &str, body: OadrReportBody) -> Result<OadrReport> {
        let value = serde_json::to_value(&body).context("serialize report body")?;
        // The inherent method: the one PUT, shared with `upsert_report`.
        let answer = self.update_report(id, value).await?;
        serde_json::from_value(answer).context("parse the report the VTN returned")
    }

    async fn token_expires_at(
        &self,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Option<chrono::DateTime<chrono::Utc>> {
        self.credentials.expires_at(now).await
    }

    async fn scope_warning(&self) -> Option<String> {
        self.scope_warning
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

#[cfg(test)]
mod tests {
    //! WP2.2 (Phase 2) adapter-contract tests: a tiny in-process axum server
    //! (no new test-only HTTP-mock dependency — axum/tokio are already
    //! production deps) stands in for the VTN so `get_json_paginated`'s real
    //! skip/limit loop is exercised end-to-end, not just its arithmetic.
    use super::*;
    use axum::extract::{Query, State};
    use axum::routing::{get, post};
    use axum::{Json, Router};
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::Mutex;
    use tokio::net::TcpListener;

    async fn token_handler() -> Json<serde_json::Value> {
        Json(json!({"access_token": "test-token", "expires_in": 3600}))
    }

    async fn paginated_handler(
        State(items): State<Arc<Vec<serde_json::Value>>>,
        Query(params): Query<HashMap<String, String>>,
    ) -> Json<serde_json::Value> {
        let skip: usize = params.get("skip").and_then(|s| s.parse().ok()).unwrap_or(0);
        let limit: usize = params
            .get("limit")
            .and_then(|s| s.parse().ok())
            .unwrap_or(50);
        let page: Vec<_> = items.iter().skip(skip).take(limit).cloned().collect();
        Json(json!(page))
    }

    /// Spawn a throwaway VTN stand-in serving `items` (paginated) from every
    /// collection route this test module needs, and return its base URL.
    async fn spawn_test_vtn(items: Vec<serde_json::Value>) -> String {
        let state = Arc::new(items);
        let app = Router::new()
            .route("/auth/token", post(token_handler))
            .route("/programs", get(paginated_handler))
            .route("/events", get(paginated_handler))
            .with_state(state);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        format!("http://{addr}")
    }

    fn make_client(base_url: String) -> VtnClient {
        VtnClient::new(base_url, "id".into(), "secret".into(), "test-ven".into())
    }

    #[tokio::test]
    async fn test_fetch_programs_accumulates_across_multiple_full_pages() {
        // 120 items over a 50/page cap = pages of 50, 50, 20 (3 requests).
        let items: Vec<_> = (0..120)
            .map(|i| json!({"id": format!("p{i}"), "programName": format!("prog-{i}")}))
            .collect();
        let base_url = spawn_test_vtn(items).await;
        let client = make_client(base_url);

        let programs = client.fetch_programs().await.unwrap().items;
        assert_eq!(programs.len(), 120);
        assert_eq!(programs[0].id, "p0");
        assert_eq!(programs[119].id, "p119");
    }

    #[tokio::test]
    async fn test_fetch_programs_empty_collection_returns_empty_vec() {
        let base_url = spawn_test_vtn(vec![]).await;
        let client = make_client(base_url);

        let programs = client.fetch_programs().await.unwrap().items;
        assert!(programs.is_empty());
    }

    #[tokio::test]
    async fn test_fetch_programs_exact_page_boundary_still_terminates() {
        // Exactly 50 items (== PAGE_LIMIT): the loop must still issue a
        // second, empty-page request to confirm there's nothing more, then stop.
        let items: Vec<_> = (0..50)
            .map(|i| json!({"id": format!("p{i}"), "programName": format!("prog-{i}")}))
            .collect();
        let base_url = spawn_test_vtn(items).await;
        let client = make_client(base_url);

        let programs = client.fetch_programs().await.unwrap().items;
        assert_eq!(programs.len(), 50);
    }

    #[tokio::test]
    async fn test_fetch_events_paginates_independently_of_programs() {
        let items: Vec<_> = (0..75)
            .map(|i| {
                json!({
                    "id": format!("e{i}"),
                    "programID": "prog-a",
                    // Required by 3.1; a VTN that omitted them would have its
                    // events refused, which is what `partition_valid` reports.
                    "createdDateTime": "2026-01-01T00:00:00Z",
                    "modificationDateTime": "2026-01-01T00:00:00Z"
                })
            })
            .collect();
        let base_url = spawn_test_vtn(items).await;
        let client = make_client(base_url);

        let events = client.fetch_events().await.unwrap().items;
        assert_eq!(events.len(), 75);
    }

    // ── WP2.3: RFC 7807 problem parsing ─────────────────────────────────────

    #[test]
    fn test_http_error_parses_rfc7807_problem_body() {
        let body = json!({
            "type": "https://example.com/probs/out-of-range",
            "title": "Program not found",
            "status": 404,
            "detail": "No program with that id exists",
            "instance": "/programs/abc"
        })
        .to_string();

        let err = http_error("/programs/abc", StatusCode::NOT_FOUND, &body);

        let msg = err.to_string();
        assert!(msg.contains("Program not found"), "message was: {msg}");
        assert!(
            msg.contains("No program with that id exists"),
            "message was: {msg}"
        );
    }

    #[test]
    fn test_http_error_falls_back_to_raw_body_when_not_problem_json() {
        let body = "internal server error, no JSON here";

        let err = http_error("/events", StatusCode::INTERNAL_SERVER_ERROR, body);

        let msg = err.to_string();
        assert!(msg.contains(body), "message was: {msg}");
        assert!(msg.contains("500"), "message was: {msg}");
    }

    #[test]
    fn test_http_error_falls_back_when_json_but_not_problem_shaped() {
        // Valid JSON, but has neither `title` nor `detail` — not an RFC 7807
        // body, so this must fall back to the raw-body path rather than
        // silently producing an error message with no useful content.
        let body = json!({"error": "unexpected"}).to_string();

        let err = http_error("/events", StatusCode::BAD_REQUEST, &body);

        let msg = err.to_string();
        assert!(msg.contains(&body), "message was: {msg}");
    }

    // ── GB-23 (design.md D1), R-131: a non-2xx answer is a DomainError with its status ──

    fn rejected_status(err: &anyhow::Error) -> u16 {
        match err.downcast_ref::<DomainError>() {
            Some(DomainError::VtnRejected { status, .. }) => *status,
            other => panic!("http_error must produce DomainError::VtnRejected, got {other:?}"),
        }
    }

    #[test]
    fn http_error_is_a_domain_rejection_with_404_status() {
        let err = http_error("/reports/abc", StatusCode::NOT_FOUND, "not found");
        assert_eq!(rejected_status(&err), 404);
    }

    #[test]
    fn http_error_is_a_domain_rejection_with_409_status() {
        let err = http_error("/reports", StatusCode::CONFLICT, "conflict");
        assert_eq!(rejected_status(&err), 409);
    }

    // ── WP2.3: BL-25 VtnUnreachable classification ──────────────────────────

    #[tokio::test]
    async fn test_classify_reqwest_error_detects_connection_refused() {
        // Port 1 has no listener bound in this sandbox — a real, fast,
        // network-free connection-refused error, no mock server needed.
        let client = reqwest::Client::new();
        let err = client
            .get("http://127.0.0.1:1/nope")
            .send()
            .await
            .expect_err("connecting to a closed local port must fail");

        let classified = classify_reqwest_error(&err);
        assert!(
            matches!(
                classified,
                Some(crate::entities::DomainError::VtnUnreachable(_))
            ),
            "expected VtnUnreachable, got {classified:?}"
        );
    }

    // ── upsert_report: 409 handling must surface the VTN problem detail ─────
    //
    // The VTN maps BOTH unique violations AND foreign-key violations to 409
    // (openleadr-rs error.rs), so a 409 is not proof of a reportName duplicate.
    // Errors on this path must carry the VTN's problem body so the true cause
    // (e.g. "A foreign key constraint is violated") is visible to operators.

    async fn conflict_post_handler() -> (StatusCode, Json<serde_json::Value>) {
        (
            StatusCode::CONFLICT,
            Json(json!({
                "title": "409 Conflict",
                "status": 409,
                "detail": "A foreign key constraint is violated"
            })),
        )
    }

    /// VTN stand-in whose POST /reports always 409s with an FK-violation
    /// problem body; GET /reports serves `own_reports` (paginated).
    async fn spawn_conflict_vtn(own_reports: Vec<serde_json::Value>) -> String {
        let state = Arc::new(own_reports);
        let app = Router::new()
            .route("/auth/token", post(token_handler))
            .route(
                "/reports",
                get(paginated_handler).post(conflict_post_handler),
            )
            .with_state(state);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        format!("http://{addr}")
    }

    fn report_body(report_name: Option<&str>) -> crate::controller::vtn_port::OadrReportBody {
        crate::controller::vtn_port::OadrReportBody {
            eventID: Some("evt-1".into()),
            clientName: "test-ven".into(),
            reportName: report_name.map(str::to_string),
            resources: vec![],
            payloadDescriptors: Vec::new(),
        }
    }

    /// A VTN that counts what it is asked to do, so a test can assert the
    /// *shape* of the traffic rather than just its outcome.
    #[derive(Default)]
    struct CallLog {
        posts: std::sync::atomic::AtomicUsize,
        puts: std::sync::atomic::AtomicUsize,
        lists: std::sync::atomic::AtomicUsize,
    }

    impl CallLog {
        fn counts(&self) -> (usize, usize, usize) {
            use std::sync::atomic::Ordering::Relaxed;
            (
                self.posts.load(Relaxed),
                self.puts.load(Relaxed),
                self.lists.load(Relaxed),
            )
        }
    }

    /// A VTN that creates a report once and then 409s, like the real one.
    /// `put_ok` false makes it reject the id, standing in for a report that
    /// has been deleted since we learned it.
    async fn spawn_counting_vtn(log: Arc<CallLog>, put_ok: bool) -> String {
        use axum::extract::State;
        use axum::http::StatusCode;

        async fn token(State(_): State<(Arc<CallLog>, bool)>) -> Json<serde_json::Value> {
            Json(json!({"access_token": "test-token", "expires_in": 3600}))
        }
        async fn post_report(
            State((log, _)): State<(Arc<CallLog>, bool)>,
            Json(_): Json<serde_json::Value>,
        ) -> (StatusCode, Json<serde_json::Value>) {
            use std::sync::atomic::Ordering::Relaxed;
            let n = log.posts.fetch_add(1, Relaxed);
            if n == 0 {
                (StatusCode::CREATED, Json(json!({"id": "rep-1"})))
            } else {
                (
                    StatusCode::CONFLICT,
                    Json(json!({"detail": "duplicate reportName"})),
                )
            }
        }
        async fn put_report(
            State((log, put_ok)): State<(Arc<CallLog>, bool)>,
            Json(_): Json<serde_json::Value>,
        ) -> (StatusCode, Json<serde_json::Value>) {
            use std::sync::atomic::Ordering::Relaxed;
            log.puts.fetch_add(1, Relaxed);
            if put_ok {
                (StatusCode::OK, Json(json!({"id": "rep-1"})))
            } else {
                (StatusCode::NOT_FOUND, Json(json!({"detail": "gone"})))
            }
        }
        async fn list_reports(
            State((log, _)): State<(Arc<CallLog>, bool)>,
        ) -> Json<serde_json::Value> {
            use std::sync::atomic::Ordering::Relaxed;
            log.lists.fetch_add(1, Relaxed);
            Json(json!([]))
        }

        let app = Router::new()
            .route("/auth/token", post(token))
            .route("/reports", get(list_reports).post(post_report))
            .route("/reports/:id", axum::routing::put(put_report))
            .with_state((log, put_ok));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        format!("http://{addr}")
    }

    /// F-6: after the first create, a submission is one PUT -- not POST, 409,
    /// a paginated lookup of our own reports, then PUT.
    #[tokio::test]
    async fn upsert_report_reuses_the_id_it_learned() {
        let log = Arc::new(CallLog::default());
        let base_url = spawn_counting_vtn(log.clone(), true).await;
        let client = make_client(base_url);

        client
            .upsert_report(report_body(Some("TELEMETRY")))
            .await
            .unwrap();
        assert_eq!(log.counts(), (1, 0, 0), "first submission creates");

        for _ in 0..3 {
            client
                .upsert_report(report_body(Some("TELEMETRY")))
                .await
                .unwrap();
        }
        let (posts, puts, lists) = log.counts();
        assert_eq!(posts, 1, "steady state must not POST again");
        assert_eq!(puts, 3, "each later submission is exactly one PUT");
        assert_eq!(lists, 0, "and never a name lookup");
    }

    /// The cache is an optimisation, so a wrong entry must cost speed and not
    /// correctness: if the VTN no longer has that id, the report still lands.
    #[tokio::test]
    async fn upsert_report_recovers_when_the_cached_id_is_gone() {
        let log = Arc::new(CallLog::default());
        let base_url = spawn_counting_vtn(log.clone(), false).await;
        let client = make_client(base_url);

        // First submission creates and caches `rep-1`.
        client
            .upsert_report(report_body(Some("TELEMETRY")))
            .await
            .unwrap();

        // Second finds the PUT rejected, forgets the id, and re-creates. This
        // VTN 409s every POST after the first, so recovery ends in the
        // name-lookup path -- what matters is that the stale id was dropped.
        let _ = client.upsert_report(report_body(Some("TELEMETRY"))).await;
        let (posts, puts, _) = log.counts();
        assert_eq!(puts, 1, "the stale id was tried once");
        assert_eq!(posts, 2, "and then a create was attempted");
    }

    #[tokio::test]
    async fn upsert_report_409_without_name_surfaces_vtn_detail() {
        let base_url = spawn_conflict_vtn(vec![]).await;
        let client = make_client(base_url);

        let err = client
            .upsert_report(report_body(None))
            .await
            .expect_err("nameless 409 cannot be upserted");

        let msg = format!("{err:#}");
        assert!(
            msg.contains("foreign key constraint"),
            "error must carry the VTN problem detail, got: {msg}"
        );
    }

    #[tokio::test]
    async fn upsert_report_409_name_lookup_miss_surfaces_vtn_detail() {
        // Own reports contain no TELEMETRY_USAGE (e.g. the conflicting row is
        // another client's — report_name is globally unique on the VTN).
        let base_url = spawn_conflict_vtn(vec![]).await;
        let client = make_client(base_url);

        let err = client
            .upsert_report(report_body(Some("TELEMETRY_USAGE")))
            .await
            .expect_err("upsert recovery must fail when the name is not visible");

        let msg = format!("{err:#}");
        assert!(
            msg.contains("TELEMETRY_USAGE"),
            "error must name the report, got: {msg}"
        );
        assert!(
            msg.contains("foreign key constraint"),
            "error must carry the VTN problem detail, got: {msg}"
        );
    }

    async fn update_ok_handler() -> Json<serde_json::Value> {
        Json(json!({"status": "ok"}))
    }

    /// Like `spawn_conflict_vtn`, but also serves `PUT /reports/:id` so a
    /// 409-then-upsert can actually complete — steady-state periodic
    /// reporting on a real VTN (every submission after the first for a given
    /// report name).
    async fn spawn_conflict_then_upsertable_vtn(own_reports: Vec<serde_json::Value>) -> String {
        let state = Arc::new(own_reports);
        let app = Router::new()
            .route("/auth/token", post(token_handler))
            .route(
                "/reports",
                get(paginated_handler).post(conflict_post_handler),
            )
            .route("/reports/:id", axum::routing::put(update_ok_handler))
            .with_state(state);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        format!("http://{addr}")
    }

    #[derive(Clone, Default)]
    struct CapturedLogs(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for CapturedLogs {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn upsert_report_409_recovered_by_name_does_not_log_at_error_level() {
        // Steady-state case: the report already exists under this name (a
        // prior submission created it), so this 409 is fully expected and
        // must resolve silently via the name-based PUT — no ERROR noise.
        let existing = json!({"id": "rep-1", "reportName": "auto-test-ven-evt-1"});
        let base_url = spawn_conflict_then_upsertable_vtn(vec![existing]).await;
        let client = make_client(base_url);

        let captured = CapturedLogs::default();
        let writer = captured.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(move || writer.clone())
            .with_max_level(tracing::Level::TRACE)
            .without_time()
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);

        client
            .upsert_report(report_body(Some("auto-test-ven-evt-1")))
            .await
            .expect("409 with a matching existing report must recover via upsert");

        drop(_guard);
        let log = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
        assert!(
            !log.contains("ERROR"),
            "a recoverable 409 must not log at error level, got:\n{log}"
        );
    }

    // The token itself (cache, expiry, re-authentication) is `lab_core::vtn_auth`'s and is
    // tested there. These pin what this client adds around it.

    /// POST `/reports` on a VTN that refuses the first token it issued, as after a restart with
    /// a new signing key. Returns the base URL; `posts` counts the POSTs it saw.
    async fn spawn_vtn_refusing_its_first_token(
        posts: Arc<std::sync::atomic::AtomicUsize>,
    ) -> String {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let issued = Arc::new(AtomicUsize::new(0));
        let app = Router::new()
            .route(
                "/auth/token",
                post(move || {
                    let n = issued.fetch_add(1, Ordering::SeqCst) + 1;
                    async move {
                        Json(json!({"access_token": format!("token-{n}"), "expires_in": 3600}))
                    }
                }),
            )
            .route(
                "/reports",
                post(move |headers: axum::http::HeaderMap| {
                    posts.fetch_add(1, Ordering::SeqCst);
                    let first = headers["authorization"] == "Bearer token-1";
                    async move {
                        if first {
                            StatusCode::UNAUTHORIZED
                        } else {
                            StatusCode::CREATED
                        }
                    }
                }),
            );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        format!("http://{addr}")
    }

    /// R-135: a report POST re-authenticates like every other request. It used to hand the 401
    /// back and leave the dead token cached for the next poll to find.
    #[tokio::test]
    async fn post_json_raw_retries_once_with_a_new_token_after_401() {
        let posts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let client = make_client(spawn_vtn_refusing_its_first_token(posts.clone()).await);

        let (status, _) = client.post_json_raw("/reports", &json!({})).await.unwrap();

        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(posts.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    /// GB-49: the scope warning is read from the token the VTN issued, when it is issued.
    #[tokio::test]
    async fn scope_warning_names_what_an_issued_token_lacks() {
        let client = make_client(spawn_test_vtn(vec![]).await);
        assert_eq!(VtnPort::scope_warning(&client).await, None, "no token yet");

        client.fetch_programs().await.unwrap();

        let warning = VtnPort::scope_warning(&client).await.expect("opaque token");
        assert!(warning.contains("read_ven_objects"), "{warning}");
    }

    // WP-T1 (docs/history/project_journal.md, search "WP-T"): token expiry observability.
    #[tokio::test]
    async fn token_expires_at_is_none_until_a_token_was_issued() {
        let client = make_client(spawn_test_vtn(vec![]).await);
        let now = chrono::Utc::now();
        assert_eq!(VtnPort::token_expires_at(&client, now).await, None);

        client.fetch_programs().await.unwrap();

        let expires_at = VtnPort::token_expires_at(&client, now).await.unwrap();
        assert!((3600 - (expires_at - now).num_seconds()).abs() <= 2);
    }

    /// R-129: the port's `update_report` is one PUT of the typed body to `/reports/{id}`, and
    /// what the VTN answers comes back typed, its other fields kept.
    #[tokio::test]
    async fn update_report_puts_the_typed_body_and_returns_the_typed_report() {
        async fn put_handler(
            axum::extract::Path(id): axum::extract::Path<String>,
            Json(mut body): Json<serde_json::Value>,
        ) -> Json<serde_json::Value> {
            body["id"] = json!(id);
            body["createdDateTime"] = json!("2026-01-01T00:00:00Z");
            Json(body)
        }
        let app = Router::new()
            .route("/auth/token", post(token_handler))
            .route("/reports/:id", axum::routing::put(put_handler));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let client = make_client(format!("http://{addr}"));
        let body = OadrReportBody {
            eventID: Some("evt-1".into()),
            clientName: "test-ven".into(),
            reportName: Some("r-1".into()),
            resources: vec![],
            payloadDescriptors: vec![],
        };

        let report = VtnPort::update_report(&client, "rep-7", body)
            .await
            .unwrap();

        assert_eq!(report.id, "rep-7");
        assert_eq!(report.reportName.as_deref(), Some("r-1"));
        assert_eq!(report.extra["eventID"], "evt-1");
        assert_eq!(report.extra["clientName"], "test-ven");
        assert_eq!(report.extra["createdDateTime"], "2026-01-01T00:00:00Z");
    }
}
