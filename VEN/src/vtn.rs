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
                .context(format!("GET {path} retry failed"))?;

            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                anyhow::bail!("{path} returned {status}: {body}");
            }
            return Ok(resp.json().await?);
        }

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("{path} returned {status}: {body}");
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
                .context(format!("PUT {path} retry failed"))?;

            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                anyhow::bail!("{path} returned {status}: {body}");
            }
            return Ok(resp.json().await?);
        }

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("{path} returned {status}: {body}");
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
            if let Some(name) = body.reportName.as_deref() {
                let id = self.find_report_by_name(name).await?;
                self.update_report(&id, value).await?;
                return Ok(());
            }
            anyhow::bail!("409 Conflict but reportName is absent — cannot upsert");
        }

        if !status.is_success() {
            anyhow::bail!("/reports returned {status}: {text}");
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
    /// Uses typed VtnPort::fetch_reports() so only id + reportName are accessed.
    async fn find_report_by_name(&self, report_name: &str) -> Result<String> {
        let reports = VtnPort::fetch_reports(self).await?;
        for r in &reports {
            if r.reportName == report_name {
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
        // Skip reports that can't deserialize (e.g. absent reportName) — they are
        // irrelevant to find_report_by_name which requires a non-null name.
        Ok(items
            .iter()
            .filter_map(|v| serde_json::from_value(v.clone()).ok())
            .collect())
    }

    async fn fetch_reports_raw(&self) -> Result<Vec<serde_json::Value>> {
        let path = format!("/reports?clientName={}", self.ven_name);
        self.get_json_paginated(&path).await
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
}
