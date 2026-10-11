//! The BFF's HTTP client for the VTN.
//!
//! Owns how the BFF speaks the VTN's API: paths, pagination, the request id it forwards, and
//! turning a non-2xx answer into `UpstreamStatusError`. The token it authenticates with is
//! `lab_core::vtn_auth`'s; this file must never fetch, cache or retry one itself (R-135).

use crate::error::UpstreamStatusError;
use anyhow::{Context, Result};
use lab_core::vtn_auth::ClientCredentials;
use reqwest::Method;

/// openleadr-rs caps every list endpoint at 50 rows per page, so this is the
/// page size for `get_all_pages` — asking for more is rejected by the VTN.
pub(crate) const PAGE_LIMIT: i64 = 50;

#[derive(Clone)]
pub struct VtnClient {
    http: reqwest::Client,
    base_url: String,
    /// The token, and when it is fetched again.
    credentials: ClientCredentials,
}

impl VtnClient {
    pub fn new(base_url: String, client_id: String, client_secret: String) -> Self {
        let http = reqwest::Client::new();
        let credentials = ClientCredentials::new(http.clone(), &base_url, client_id, client_secret);
        Self {
            http,
            base_url,
            credentials,
        }
    }

    /// Send one authorized request to `path` and return the VTN's answer if it is 2xx. The
    /// token, and the one retry after a 401 or 403, are `ClientCredentials::send`'s.
    async fn send(
        &self,
        method: Method,
        path: &str,
        body: Option<&serde_json::Value>,
        request_id: Option<&str>,
    ) -> Result<reqwest::Response> {
        let url = format!("{}{}", self.base_url.trim_end_matches('/'), path);
        let resp = self
            .credentials
            .send(|| {
                let mut request = self.http.request(method.clone(), &url);
                if let Some(rid) = request_id {
                    request = request.header("x-request-id", rid);
                }
                match body {
                    Some(body) => request.json(body),
                    None => request,
                }
            })
            .await
            .with_context(|| format!("{method} {path} failed"))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(UpstreamStatusError {
                status,
                message: format!("{path} returned {status}: {body}"),
            }
            .into());
        }
        Ok(resp)
    }

    pub async fn get_json(
        &self,
        path: &str,
        request_id: Option<&str>,
    ) -> Result<serde_json::Value> {
        let resp = self.send(Method::GET, path, None, request_id).await?;
        Ok(resp.json().await?)
    }

    /// GET every page of a VTN list endpoint via `skip`/`limit`, stopping when a
    /// page returns fewer than `PAGE_LIMIT` rows.
    ///
    /// The one place in the BFF that knows a VTN collection is paginated
    /// (openleadr-rs caps every list endpoint at 50 per page, silently
    /// truncating a plain `get_json`): both the list routes and the recorder
    /// read collections through here, so neither can grow its own loop (R-84).
    pub async fn get_all_pages(
        &self,
        path: &str,
        request_id: Option<&str>,
    ) -> Result<Vec<serde_json::Value>> {
        let mut all = Vec::new();
        let mut skip = 0i64;
        loop {
            let sep = if path.contains('?') { '&' } else { '?' };
            let page_path = format!("{path}{sep}skip={skip}&limit={PAGE_LIMIT}");
            let page: Vec<serde_json::Value> =
                serde_json::from_value(self.get_json(&page_path, request_id).await?)
                    .context(format!("{path} did not return a JSON array"))?;
            let n = page.len();
            all.extend(page);
            if (n as i64) < PAGE_LIMIT {
                break;
            }
            skip += PAGE_LIMIT;
        }
        Ok(all)
    }

    pub async fn post_json(
        &self,
        path: &str,
        body: serde_json::Value,
        request_id: Option<&str>,
    ) -> Result<serde_json::Value> {
        let resp = self
            .send(Method::POST, path, Some(&body), request_id)
            .await?;
        Ok(resp.json().await?)
    }

    pub async fn put_json(
        &self,
        path: &str,
        body: serde_json::Value,
        request_id: Option<&str>,
    ) -> Result<serde_json::Value> {
        let resp = self
            .send(Method::PUT, path, Some(&body), request_id)
            .await?;
        Ok(resp.json().await?)
    }

    pub async fn delete_json(&self, path: &str, request_id: Option<&str>) -> Result<()> {
        self.send(Method::DELETE, path, None, request_id).await?;
        Ok(())
    }

    /// Check if the VTN is reachable and auth works.
    pub async fn check_health(&self) -> (bool, bool) {
        let reachable = self
            .http
            .get(format!("{}/health", self.base_url.trim_end_matches('/')))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false);

        let auth_ok = if reachable {
            self.credentials.token().await.is_ok()
        } else {
            false
        };

        (reachable, auth_ok)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode as AxStatus;
    use axum::response::IntoResponse;
    use axum::routing::{get, post};
    use axum::{Json, Router};
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};

    async fn token_handler() -> Json<serde_json::Value> {
        Json(json!({"access_token": "test-token", "token_type": "bearer", "expires_in": 3600}))
    }

    /// Serve `app` on an ephemeral local port; returns the base URL.
    async fn spawn_stub(app: Router) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        format!("http://{addr}")
    }

    fn client_for(base_url: String) -> VtnClient {
        VtnClient::new(base_url, "id".into(), "secret".into())
    }

    #[tokio::test]
    async fn get_json_returns_body_on_200() {
        let app = Router::new()
            .route("/auth/token", post(token_handler))
            .route("/programs", get(|| async { Json(json!([{"id": "p1"}])) }));
        let client = client_for(spawn_stub(app).await);

        let body = client.get_json("/programs", None).await.unwrap();
        assert_eq!(body, json!([{"id": "p1"}]));
    }

    // ── get_all_pages (R-84) ────────────────────────────────────────────────

    /// Records every `skip`/`limit` query the stub was asked for, and serves
    /// `total` synthetic rows across pages of `PAGE_LIMIT`.
    fn paged_stub(total: usize, queries: std::sync::Arc<std::sync::Mutex<Vec<String>>>) -> Router {
        Router::new()
            .route("/auth/token", post(token_handler))
            .route(
                "/programs",
                get(move |axum::extract::RawQuery(q): axum::extract::RawQuery| {
                    let queries = queries.clone();
                    async move {
                        let q = q.unwrap_or_default();
                        queries.lock().unwrap().push(q.clone());
                        let skip: usize = q
                            .split('&')
                            .find_map(|kv| kv.strip_prefix("skip="))
                            .and_then(|v| v.parse().ok())
                            .unwrap_or(0);
                        let page: Vec<serde_json::Value> = (skip..(skip + PAGE_LIMIT as usize)
                            .min(total))
                            .map(|i| json!({"id": format!("p{i}")}))
                            .collect();
                        Json(serde_json::Value::Array(page))
                    }
                }),
            )
    }

    #[tokio::test]
    async fn get_all_pages_follows_skip_until_a_short_page() {
        let queries = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let client = client_for(spawn_stub(paged_stub(53, queries.clone())).await);

        let rows = client.get_all_pages("/programs", None).await.unwrap();

        assert_eq!(rows.len(), 53, "every page must be accumulated");
        assert_eq!(rows[0], json!({"id": "p0"}));
        assert_eq!(rows[52], json!({"id": "p52"}));
        assert_eq!(
            *queries.lock().unwrap(),
            vec!["skip=0&limit=50", "skip=50&limit=50"],
            "must page with skip/limit and stop after the short page"
        );
    }

    #[tokio::test]
    async fn get_all_pages_stops_after_one_request_when_the_first_page_is_short() {
        let queries = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let client = client_for(spawn_stub(paged_stub(3, queries.clone())).await);

        let rows = client.get_all_pages("/programs", None).await.unwrap();

        assert_eq!(rows.len(), 3);
        assert_eq!(queries.lock().unwrap().len(), 1, "no needless second page");
    }

    #[tokio::test]
    async fn get_all_pages_appends_to_an_existing_query_string() {
        let queries = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let client = client_for(spawn_stub(paged_stub(1, queries.clone())).await);

        client
            .get_all_pages("/programs?active=true", None)
            .await
            .unwrap();

        assert_eq!(
            *queries.lock().unwrap(),
            vec!["active=true&skip=0&limit=50"],
            "an existing query string must be kept, not replaced"
        );
    }

    #[tokio::test]
    async fn get_all_pages_errors_when_the_body_is_not_an_array() {
        let app = Router::new()
            .route("/auth/token", post(token_handler))
            .route("/programs", get(|| async { Json(json!({"id": "p1"})) }));
        let client = client_for(spawn_stub(app).await);

        let err = client.get_all_pages("/programs", None).await.unwrap_err();
        assert!(
            err.to_string().contains("did not return a JSON array"),
            "error must name the problem: {err}"
        );
    }

    #[tokio::test]
    async fn get_json_bails_with_status_on_500() {
        let app = Router::new()
            .route("/auth/token", post(token_handler))
            .route(
                "/programs",
                get(|| async { (AxStatus::INTERNAL_SERVER_ERROR, "boom") }),
            );
        let client = client_for(spawn_stub(app).await);

        let err = client.get_json("/programs", None).await.unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("500"), "error must carry the status: {msg}");
        assert!(msg.contains("boom"), "error must carry the body: {msg}");
    }

    #[tokio::test]
    async fn get_json_bails_with_downcastable_status_error_on_409() {
        let app = Router::new()
            .route("/auth/token", post(token_handler))
            .route(
                "/programs",
                get(|| async { (AxStatus::CONFLICT, "name already exists") }),
            );
        let client = client_for(spawn_stub(app).await);

        let err = client.get_json("/programs", None).await.unwrap_err();
        let upstream = err
            .downcast_ref::<crate::error::UpstreamStatusError>()
            .expect("error must downcast to UpstreamStatusError");
        assert_eq!(upstream.status, AxStatus::CONFLICT);
        assert!(upstream.message.contains("name already exists"));
    }

    #[tokio::test]
    async fn get_json_retries_once_after_401() {
        let calls = std::sync::Arc::new(AtomicUsize::new(0));
        let calls_handler = calls.clone();
        let app = Router::new()
            .route("/auth/token", post(token_handler))
            .route(
                "/events",
                get(move || {
                    let calls = calls_handler.clone();
                    async move {
                        if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                            AxStatus::UNAUTHORIZED.into_response()
                        } else {
                            Json(json!([{"id": "e1"}])).into_response()
                        }
                    }
                }),
            );
        let client = client_for(spawn_stub(app).await);

        let body = client.get_json("/events", None).await.unwrap();
        assert_eq!(body, json!([{"id": "e1"}]));
        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "must retry exactly once after a 401"
        );
    }

    #[tokio::test]
    async fn post_json_sends_body_and_returns_response() {
        let app =
            Router::new()
                .route("/auth/token", post(token_handler))
                .route(
                    "/reports",
                    post(|Json(body): Json<serde_json::Value>| async move {
                        Json(json!({"echo": body}))
                    }),
                );
        let client = client_for(spawn_stub(app).await);

        let body = client
            .post_json("/reports", json!({"reportName": "r1"}), None)
            .await
            .unwrap();
        assert_eq!(body, json!({"echo": {"reportName": "r1"}}));
    }

    #[tokio::test]
    async fn check_health_reports_unreachable_vtn() {
        // Nothing listens on this port (bound then dropped immediately).
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);

        let client = client_for(format!("http://{addr}"));
        let (reachable, auth_ok) = client.check_health().await;
        assert!(!reachable);
        assert!(!auth_ok, "auth must not be probed when unreachable");
    }

    #[tokio::test]
    async fn check_health_reports_reachable_and_authed() {
        let app = Router::new()
            .route("/auth/token", post(token_handler))
            .route("/health", get(|| async { "ok" }));
        let client = client_for(spawn_stub(app).await);

        let (reachable, auth_ok) = client.check_health().await;
        assert!(reachable);
        assert!(auth_ok);
    }
}
