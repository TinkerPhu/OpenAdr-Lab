use anyhow::{Context, Result};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};

use crate::models::{Event, Program};

#[derive(Clone)]
pub struct VtnClient {
    http: reqwest::Client,
    base_url: String,
    client_id: String,
    client_secret: String,
    token: tokio::sync::RwLock<Option<Token>>,
}

#[derive(Clone, Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    token_type: Option<String>,
    expires_in: Option<u64>,
}

#[derive(Clone, Debug)]
struct Token {
    access_token: String,
    // very rough; you can add expiry tracking later
}

impl VtnClient {
    pub fn new(base_url: String, client_id: String, client_secret: String) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url,
            client_id,
            client_secret,
            token: tokio::sync::RwLock::new(None),
        }
    }

    async fn ensure_token(&self) -> Result<String> {
        if let Some(t) = self.token.read().await.clone() {
            return Ok(t.access_token);
        }

        // You MUST set this to your VTN token endpoint.
        // Many setups use something like: {base}/oauth/token
        let token_url = format!("{}/oauth/token", self.base_url.trim_end_matches('/'));

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
        let token = Token { access_token: tr.access_token };
        *self.token.write().await = Some(token.clone());
        Ok(token.access_token)
    }

    pub async fn fetch_programs(&self) -> Result<Vec<Program>> {
        let token = self.ensure_token().await?;

        let url = format!("{}/programs", self.base_url.trim_end_matches('/'));
        let resp = self.http
            .get(url)
            .bearer_auth(token)
            .send()
            .await
            .context("fetch programs failed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("programs returned {status}: {body}");
        }

        // Adjust to match actual VTN response shape.
        let raw: serde_json::Value = resp.json().await?;
        let programs = parse_programs_loose(raw);
        Ok(programs)
    }

    pub async fn fetch_events(&self) -> Result<Vec<Event>> {
        let token = self.ensure_token().await?;

        let url = format!("{}/events", self.base_url.trim_end_matches('/'));
        let resp = self.http
            .get(url)
            .bearer_auth(token)
            .send()
            .await
            .context("fetch events failed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("events returned {status}: {body}");
        }

        // Adjust to match actual VTN response shape.
        let raw: serde_json::Value = resp.json().await?;
        let events = parse_events_loose(raw);
        Ok(events)
    }
}

// These parsers keep you moving quickly while you confirm exact API responses.
fn parse_programs_loose(raw: serde_json::Value) -> Vec<Program> {
    let arr = raw.as_array().cloned().unwrap_or_default();
    arr.into_iter()
        .filter_map(|v| {
            let id = v.get("id")?.as_str()?.to_string();
            let name = v.get("name").and_then(|n| n.as_str().map(|s| s.to_string()));
            Some(Program { id, name })
        })
        .collect()
}

fn parse_events_loose(raw: serde_json::Value) -> Vec<Event> {
    let arr = raw.as_array().cloned().unwrap_or_default();
    arr.into_iter()
        .filter_map(|v| {
            let id = v.get("id")?.as_str()?.to_string();
            let program_id = v.get("program_id").and_then(|p| p.as_str().map(|s| s.to_string()));
            let status = v.get("status").and_then(|s| s.as_str().map(|s| s.to_string()));
            Some(Event {
                id,
                program_id,
                created_at: None,
                status,
                raw: v,
            })
        })
        .collect()
}
