use anyhow::{Context, Result};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Instant;

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
    pub fn new(base_url: String, client_id: String, client_secret: String, ven_name: String) -> Self {
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
            if elapsed + 60 < t.expires_in_secs {
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

        let resp = self.http
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

        let resp = self.http
            .get(&url)
            .bearer_auth(&token)
            .send()
            .await
            .context(format!("GET {path} failed"))?;

        if resp.status() == StatusCode::UNAUTHORIZED || resp.status() == StatusCode::FORBIDDEN {
            // Token invalid or VTN restarted with new signing key; refresh once and retry
            self.invalidate_token().await;
            let new_token = self.ensure_token().await?;
            let resp = self.http
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

    /// POST JSON to a VTN endpoint with automatic 401-retry.
    async fn post_json(&self, path: &str, body: serde_json::Value) -> Result<serde_json::Value> {
        let token = self.ensure_token().await?;
        let url = format!("{}{}", self.base_url.trim_end_matches('/'), path);

        let resp = self.http
            .post(&url)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await
            .context(format!("POST {path} failed"))?;

        if resp.status() == StatusCode::UNAUTHORIZED || resp.status() == StatusCode::FORBIDDEN {
            self.invalidate_token().await;
            let new_token = self.ensure_token().await?;
            let resp = self.http
                .post(&url)
                .bearer_auth(&new_token)
                .json(&body)
                .send()
                .await
                .context(format!("POST {path} retry failed"))?;

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

        let resp = self.http
            .put(&url)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await
            .context(format!("PUT {path} failed"))?;

        if resp.status() == StatusCode::UNAUTHORIZED || resp.status() == StatusCode::FORBIDDEN {
            self.invalidate_token().await;
            let new_token = self.ensure_token().await?;
            let resp = self.http
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
    async fn post_json_raw(&self, path: &str, body: &serde_json::Value) -> Result<(StatusCode, String)> {
        let token = self.ensure_token().await?;
        let url = format!("{}{}", self.base_url.trim_end_matches('/'), path);

        let resp = self.http
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

    pub async fn fetch_programs(&self) -> Result<Vec<serde_json::Value>> {
        let raw = self.get_json("/programs").await?;
        Ok(raw.as_array().cloned().unwrap_or_default())
    }

    pub async fn fetch_events(&self) -> Result<Vec<serde_json::Value>> {
        let raw = self.get_json("/events?active=true").await?;
        Ok(raw.as_array().cloned().unwrap_or_default())
    }

    pub async fn fetch_reports(&self) -> Result<Vec<serde_json::Value>> {
        let path = format!("/reports?clientName={}", self.ven_name);
        let raw = self.get_json(&path).await?;
        Ok(raw.as_array().cloned().unwrap_or_default())
    }

    pub async fn submit_report(&self, body: serde_json::Value) -> Result<serde_json::Value> {
        self.post_json("/reports", body).await
    }

    /// Submit a report with upsert semantics: on 409 Conflict, find the existing
    /// report by name and update it instead.
    pub async fn upsert_report(&self, body: serde_json::Value) -> Result<serde_json::Value> {
        let (status, text) = self.post_json_raw("/reports", &body).await?;

        if status == StatusCode::CONFLICT {
            // Extract reportName from the request body
            let report_name = body.get("reportName")
                .and_then(|v| v.as_str())
                .context("409 Conflict but no reportName in body")?;

            let id = self.find_report_by_name(report_name).await?;
            return self.update_report(&id, body).await;
        }

        if !status.is_success() {
            anyhow::bail!("/reports returned {status}: {text}");
        }

        serde_json::from_str(&text).context("parse report response")
    }

    pub async fn update_report(&self, id: &str, body: serde_json::Value) -> Result<serde_json::Value> {
        let path = format!("/reports/{id}");
        self.put_json(&path, body).await
    }

    /// Search own reports (already filtered by client_name) for a matching reportName.
    async fn find_report_by_name(&self, report_name: &str) -> Result<String> {
        let reports = self.fetch_reports().await?;
        for r in &reports {
            if r.get("reportName").and_then(|v| v.as_str()) == Some(report_name) {
                if let Some(id) = r.get("id").and_then(|v| v.as_str()) {
                    return Ok(id.to_string());
                }
            }
        }
        anyhow::bail!("no report found with name '{report_name}'")
    }
}
