use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::StatusCode;

const TOKEN_EXPIRY_MARGIN_S: u64 = 60;
// WP2.2 (Phase 2): openleadr-rs collection GETs cap at 50/page regardless of
// a larger requested `limit` — match that cap so every page is full until
// the last one.
const PAGE_LIMIT: usize = 50;
// Runaway-poll guard: a well-behaved poll should never need this many pages
// for programs/events/reports on a single VEN; log if it does rather than
// looping forever on a misbehaving or huge-collection VTN.
const MAX_PAGES_WARNING: u32 = 20;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Instant;

use crate::controller::vtn_port::{OadrEvent, OadrProgram, OadrReport, OadrReportBody, VtnPort};

#[derive(Clone)]
pub struct VtnClient {
    http: reqwest::Client,
    base_url: String,
    client_id: String,
    client_secret: String,
    ven_name: String,
    token: Arc<tokio::sync::RwLock<Option<Token>>>,
}

#[derive(Clone, Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[allow(dead_code)]
    token_type: Option<String>,
    expires_in: Option<u64>,
}

#[derive(Clone, Debug)]
struct Token {
    access_token: String,
    acquired_at: Instant,
    expires_in_secs: u64,
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
fn log_if_vtn_unreachable(path: &str, e: &reqwest::Error) {
    if let Some(domain_err) = classify_reqwest_error(e) {
        tracing::error!(path, error = %domain_err, "VTN unreachable");
    }
}

/// Build the error for a non-2xx VTN response, logging a structured line
/// either way: as parsed RFC 7807 fields if the body is problem+json shaped,
/// or the raw body otherwise. Called at every "not `is_success()`" branch in
/// this client so no call site has to duplicate the parse-or-fallback logic.
fn http_error(path: &str, status: StatusCode, body: &str) -> anyhow::Error {
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
            anyhow::anyhow!(
                "{path} returned {status}: {} — {}",
                problem.title.unwrap_or_default(),
                problem.detail.unwrap_or_default()
            )
        }
        _ => {
            tracing::error!(
                path,
                status = status.as_u16(),
                body,
                "VTN returned a non-2xx response"
            );
            anyhow::anyhow!("{path} returned {status}: {body}")
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
        Self {
            http: reqwest::Client::new(),
            base_url,
            client_id,
            client_secret,
            ven_name,
            token: Arc::new(tokio::sync::RwLock::new(None)),
        }
    }

    async fn ensure_token(&self) -> Result<String> {
        // Return cached token if still valid (with 60s safety margin)
        if let Some(t) = self.token.read().await.as_ref() {
            let elapsed = t.acquired_at.elapsed().as_secs();
            if elapsed + TOKEN_EXPIRY_MARGIN_S < t.expires_in_secs {
                return Ok(t.access_token.clone());
            }
        }

        self.fetch_new_token().await
    }

    async fn invalidate_token(&self) {
        *self.token.write().await = None;
    }

    /// WP-T1 (`docs/plans/ven-ui-transparency.md`): wall-clock expiry of the
    /// currently cached token, for `GET /vtn/status`. `Instant` is monotonic, so
    /// this derives an approximate `DateTime<Utc>` from elapsed time since
    /// acquisition — read-only observability, not used by `ensure_token`'s own
    /// (monotonic) refresh check.
    pub async fn token_expires_at(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        let guard = self.token.read().await;
        let t = guard.as_ref()?;
        let remaining = std::time::Duration::from_secs(t.expires_in_secs)
            .saturating_sub(t.acquired_at.elapsed());
        Some(chrono::Utc::now() + remaining)
    }

    async fn fetch_new_token(&self) -> Result<String> {
        let token_url = format!("{}/auth/token", self.base_url.trim_end_matches('/'));

        #[derive(Serialize)]
        struct Form<'a> {
            grant_type: &'a str,
            client_id: &'a str,
            client_secret: &'a str,
        }

        let resp = self
            .http
            .post(token_url)
            .form(&Form {
                grant_type: "client_credentials",
                client_id: &self.client_id,
                client_secret: &self.client_secret,
            })
            .send()
            .await
            .inspect_err(|e| log_if_vtn_unreachable("/auth/token", e))
            .context("token request failed")?;

        if resp.status() != StatusCode::OK {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("token endpoint returned {status}: {body}");
        }

        let tr: TokenResponse = resp.json().await.context("parse token response")?;
        let expires_in_secs = tr.expires_in.unwrap_or(3600);
        let token = Token {
            access_token: tr.access_token,
            acquired_at: Instant::now(),
            expires_in_secs,
        };
        let access = token.access_token.clone();
        *self.token.write().await = Some(token);
        Ok(access)
    }

    /// GET a VTN endpoint with automatic 401-retry (re-fetch token once).
    async fn get_json(&self, path: &str) -> Result<serde_json::Value> {
        let token = self.ensure_token().await?;
        let url = format!("{}{}", self.base_url.trim_end_matches('/'), path);

        let resp = self
            .http
            .get(&url)
            .bearer_auth(&token)
            .send()
            .await
            .inspect_err(|e| log_if_vtn_unreachable(path, e))
            .context(format!("GET {path} failed"))?;

        if resp.status() == StatusCode::UNAUTHORIZED || resp.status() == StatusCode::FORBIDDEN {
            // Token invalid or VTN restarted with new signing key; refresh once and retry
            self.invalidate_token().await;
            let new_token = self.ensure_token().await?;
            let resp = self
                .http
                .get(&url)
                .bearer_auth(&new_token)
                .send()
                .await
                .inspect_err(|e| log_if_vtn_unreachable(path, e))
                .context(format!("GET {path} retry failed"))?;

            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                return Err(http_error(path, status, &body));
            }
            return Ok(resp.json().await?);
        }

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(http_error(path, status, &body));
        }

        Ok(resp.json().await?)
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

    /// PUT JSON to a VTN endpoint with automatic 401-retry.
    async fn put_json(&self, path: &str, body: serde_json::Value) -> Result<serde_json::Value> {
        let token = self.ensure_token().await?;
        let url = format!("{}{}", self.base_url.trim_end_matches('/'), path);

        let resp = self
            .http
            .put(&url)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await
            .inspect_err(|e| log_if_vtn_unreachable(path, e))
            .context(format!("PUT {path} failed"))?;

        if resp.status() == StatusCode::UNAUTHORIZED || resp.status() == StatusCode::FORBIDDEN {
            self.invalidate_token().await;
            let new_token = self.ensure_token().await?;
            let resp = self
                .http
                .put(&url)
                .bearer_auth(&new_token)
                .json(&body)
                .send()
                .await
                .inspect_err(|e| log_if_vtn_unreachable(path, e))
                .context(format!("PUT {path} retry failed"))?;

            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                return Err(http_error(path, status, &body));
            }
            return Ok(resp.json().await?);
        }

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(http_error(path, status, &body));
        }

        Ok(resp.json().await?)
    }

    /// POST JSON, returning the raw response (status + body) without error-mapping.
    async fn post_json_raw(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<(StatusCode, String)> {
        let token = self.ensure_token().await?;
        let url = format!("{}{}", self.base_url.trim_end_matches('/'), path);

        let resp = self
            .http
            .post(&url)
            .bearer_auth(&token)
            .json(body)
            .send()
            .await
            .inspect_err(|e| log_if_vtn_unreachable(path, e))
            .context(format!("POST {path} failed"))?;

        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        Ok((status, text))
    }

    /// Submit a report with upsert semantics: on 409 Conflict, find the existing
    /// report by name and update it instead.
    pub(crate) async fn upsert_report(
        &self,
        body: crate::controller::vtn_port::OadrReportBody,
    ) -> Result<()> {
        let value = serde_json::to_value(&body).context("serialize report body")?;
        let (status, text) = self.post_json_raw("/reports", &value).await?;

        if status == StatusCode::CONFLICT {
            // A VTN 409 is NOT proof of a reportName duplicate: openleadr-rs
            // maps both unique violations and foreign-key violations (e.g. the
            // referenced event was cascade-deleted) to 409. Attempt the
            // name-based upsert when possible, but every failure on this path
            // must carry the VTN's problem body — it names the real cause.
            let vtn_problem = http_error("/reports", status, &text);
            if let Some(name) = body.reportName.as_deref() {
                match self.find_report_by_name(name).await {
                    Ok(id) => {
                        self.update_report(&id, value).await?;
                        return Ok(());
                    }
                    Err(lookup_err) => anyhow::bail!(
                        "409 on POST /reports and upsert of reportName '{name}' failed \
                         ({lookup_err:#}); VTN said: {vtn_problem:#}"
                    ),
                }
            }
            anyhow::bail!(
                "409 on POST /reports without reportName — cannot upsert by name; \
                 VTN said: {vtn_problem:#}"
            );
        }

        if !status.is_success() {
            return Err(http_error("/reports", status, &text));
        }

        Ok(())
    }

    pub(crate) async fn update_report(
        &self,
        id: &str,
        body: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let path = format!("/reports/{id}");
        self.put_json(&path, body).await
    }

    /// Search own reports (filtered by client_name) for a matching reportName.
    async fn find_report_by_name(&self, report_name: &str) -> Result<String> {
        let reports = VtnPort::fetch_reports(self).await?;
        for r in &reports {
            if r.reportName.as_deref() == Some(report_name) {
                return Ok(r.id.clone());
            }
        }
        anyhow::bail!("no report found with name '{report_name}'")
    }
}

// ── VtnPort implementation ────────────────────────────────────────────────────

#[async_trait]
impl VtnPort for VtnClient {
    async fn fetch_programs(&self) -> Result<Vec<OadrProgram>> {
        let items = self.get_json_paginated("/programs").await?;
        items
            .iter()
            .map(|v| serde_json::from_value(v.clone()).map_err(anyhow::Error::from))
            .collect()
    }

    async fn fetch_events(&self) -> Result<Vec<OadrEvent>> {
        let items = self.get_json_paginated("/events?active=true").await?;
        items
            .iter()
            .map(|v| serde_json::from_value(v.clone()).map_err(anyhow::Error::from))
            .collect()
    }

    async fn fetch_reports(&self) -> Result<Vec<OadrReport>> {
        let path = format!("/reports?clientName={}", self.ven_name);
        let items = self.get_json_paginated(&path).await?;
        // Skip items without an `id` (can't deserialize) — everything else,
        // including absent reportName, is preserved via OadrReport's flatten.
        Ok(items
            .iter()
            .filter_map(|v| serde_json::from_value(v.clone()).ok())
            .collect())
    }

    async fn upsert_report(&self, body: OadrReportBody) -> Result<()> {
        // Delegates to the inherent method which handles 409 upsert semantics.
        self.upsert_report(body).await
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

        let programs = client.fetch_programs().await.unwrap();
        assert_eq!(programs.len(), 120);
        assert_eq!(programs[0].id, "p0");
        assert_eq!(programs[119].id, "p119");
    }

    #[tokio::test]
    async fn test_fetch_programs_empty_collection_returns_empty_vec() {
        let base_url = spawn_test_vtn(vec![]).await;
        let client = make_client(base_url);

        let programs = client.fetch_programs().await.unwrap();
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

        let programs = client.fetch_programs().await.unwrap();
        assert_eq!(programs.len(), 50);
    }

    #[tokio::test]
    async fn test_fetch_events_paginates_independently_of_programs() {
        let items: Vec<_> = (0..75)
            .map(|i| json!({"id": format!("e{i}"), "programID": "prog-a"}))
            .collect();
        let base_url = spawn_test_vtn(items).await;
        let client = make_client(base_url);

        let events = client.fetch_events().await.unwrap();
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
            programID: "prog-1".into(),
            eventID: Some("evt-1".into()),
            clientName: "test-ven".into(),
            reportName: report_name.map(str::to_string),
            resources: vec![],
        }
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

    // WP-T1 (docs/plans/ven-ui-transparency.md): token expiry observability.

    #[tokio::test]
    async fn token_expires_at_reflects_expires_in_from_acquisition() {
        let client = VtnClient::new(
            "http://example.invalid".to_string(),
            "id".to_string(),
            "secret".to_string(),
            "ven".to_string(),
        );
        *client.token.write().await = Some(Token {
            access_token: "t".to_string(),
            acquired_at: std::time::Instant::now(),
            expires_in_secs: 3600,
        });

        let expires_at = client.token_expires_at().await.expect("token was just set");
        let now = chrono::Utc::now();
        assert!(
            expires_at > now + chrono::Duration::seconds(3500),
            "expiry should be ~3600s out, got {expires_at} vs now {now}"
        );
        assert!(
            expires_at <= now + chrono::Duration::seconds(3600),
            "expiry should not exceed the full expires_in window"
        );
    }

    #[tokio::test]
    async fn token_expires_at_none_when_no_token_cached() {
        let client = VtnClient::new(
            "http://example.invalid".to_string(),
            "id".to_string(),
            "secret".to_string(),
            "ven".to_string(),
        );
        assert_eq!(client.token_expires_at().await, None);
    }
}
