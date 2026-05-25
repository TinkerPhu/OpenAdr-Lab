use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::StatusCode;

const TOKEN_EXPIRY_MARGIN_S: u64 = 60;
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
        let raw = self.get_json("/programs").await?;
        let items = raw.as_array().cloned().unwrap_or_default();
        items
            .iter()
            .map(|v| serde_json::from_value(v.clone()).map_err(anyhow::Error::from))
            .collect()
    }

    async fn fetch_events(&self) -> Result<Vec<OadrEvent>> {
        let raw = self.get_json("/events?active=true").await?;
        let items = raw.as_array().cloned().unwrap_or_default();
        items
            .iter()
            .map(|v| serde_json::from_value(v.clone()).map_err(anyhow::Error::from))
            .collect()
    }

    async fn fetch_reports(&self) -> Result<Vec<OadrReport>> {
        let path = format!("/reports?clientName={}", self.ven_name);
        let raw = self.get_json(&path).await?;
        let items = raw.as_array().cloned().unwrap_or_default();
        // Skip reports that can't deserialize (e.g. absent reportName) — they are
        // irrelevant to find_report_by_name which requires a non-null name.
        Ok(items
            .iter()
            .filter_map(|v| serde_json::from_value(v.clone()).ok())
            .collect())
    }

    async fn fetch_reports_raw(&self) -> Result<Vec<serde_json::Value>> {
        let path = format!("/reports?clientName={}", self.ven_name);
        let raw = self.get_json(&path).await?;
        Ok(raw.as_array().cloned().unwrap_or_default())
    }

    async fn upsert_report(&self, body: OadrReportBody) -> Result<()> {
        // Delegates to the inherent method which handles 409 upsert semantics.
        self.upsert_report(body).await
    }
}
