use anyhow::{Context, Result};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Instant;

use crate::models::{Event, Program};

#[derive(Clone)]
pub struct VtnClient {
    http: reqwest::Client,
    base_url: String,
    client_id: String,
    client_secret: String,
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
    pub fn new(base_url: String, client_id: String, client_secret: String) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url,
            client_id,
            client_secret,
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

    pub async fn fetch_programs(&self) -> Result<Vec<Program>> {
        let raw = self.get_json("/programs").await?;
        Ok(parse_programs_loose(raw))
    }

    pub async fn fetch_events(&self) -> Result<Vec<Event>> {
        let raw = self.get_json("/events").await?;
        Ok(parse_events_loose(raw))
    }
}

// Parsers aligned with actual openleadr-rs VTN response shapes.
fn parse_programs_loose(raw: serde_json::Value) -> Vec<Program> {
    let arr = raw.as_array().cloned().unwrap_or_default();
    arr.into_iter()
        .filter_map(|v| {
            let id = v.get("id")?.as_str()?.to_string();
            let name = v.get("programName").and_then(|n| n.as_str().map(|s| s.to_string()));
            Some(Program { id, name })
        })
        .collect()
}

fn parse_events_loose(raw: serde_json::Value) -> Vec<Event> {
    let arr = raw.as_array().cloned().unwrap_or_default();
    arr.into_iter()
        .filter_map(|v| {
            let id = v.get("id")?.as_str()?.to_string();
            let program_id = v.get("programID").and_then(|p| p.as_str().map(|s| s.to_string()));
            let created_at = v.get("createdDateTime")
                .and_then(|d| d.as_str())
                .and_then(|s| s.parse().ok());
            Some(Event {
                id,
                program_id,
                created_at,
                status: None, // openleadr-rs events don't have a status field; derive from intervals if needed
                raw: v,
            })
        })
        .collect()
}
