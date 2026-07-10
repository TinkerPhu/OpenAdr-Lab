use anyhow::{Context, Result};
use reqwest::StatusCode;
use serde::Deserialize;
use std::sync::Arc;
use std::time::Instant;

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

        #[derive(serde::Serialize)]
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

    fn apply_request_id(
        &self,
        builder: reqwest::RequestBuilder,
        request_id: Option<&str>,
    ) -> reqwest::RequestBuilder {
        if let Some(rid) = request_id {
            builder.header("x-request-id", rid)
        } else {
            builder
        }
    }

    /// GET a VTN endpoint with automatic 401-retry.
    pub async fn get_json(
        &self,
        path: &str,
        request_id: Option<&str>,
    ) -> Result<serde_json::Value> {
        let token = self.ensure_token().await?;
        let url = format!("{}{}", self.base_url.trim_end_matches('/'), path);

        let resp = self
            .apply_request_id(self.http.get(&url).bearer_auth(&token), request_id)
            .send()
            .await
            .context(format!("GET {path} failed"))?;

        if resp.status() == StatusCode::UNAUTHORIZED || resp.status() == StatusCode::FORBIDDEN {
            self.invalidate_token().await;
            let new_token = self.ensure_token().await?;
            let resp = self
                .apply_request_id(self.http.get(&url).bearer_auth(&new_token), request_id)
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
    pub async fn post_json(
        &self,
        path: &str,
        body: serde_json::Value,
        request_id: Option<&str>,
    ) -> Result<serde_json::Value> {
        let token = self.ensure_token().await?;
        let url = format!("{}{}", self.base_url.trim_end_matches('/'), path);

        let resp = self
            .apply_request_id(
                self.http.post(&url).bearer_auth(&token).json(&body),
                request_id,
            )
            .send()
            .await
            .context(format!("POST {path} failed"))?;

        if resp.status() == StatusCode::UNAUTHORIZED || resp.status() == StatusCode::FORBIDDEN {
            self.invalidate_token().await;
            let new_token = self.ensure_token().await?;
            let resp = self
                .apply_request_id(
                    self.http.post(&url).bearer_auth(&new_token).json(&body),
                    request_id,
                )
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
    pub async fn put_json(
        &self,
        path: &str,
        body: serde_json::Value,
        request_id: Option<&str>,
    ) -> Result<serde_json::Value> {
        let token = self.ensure_token().await?;
        let url = format!("{}{}", self.base_url.trim_end_matches('/'), path);

        let resp = self
            .apply_request_id(
                self.http.put(&url).bearer_auth(&token).json(&body),
                request_id,
            )
            .send()
            .await
            .context(format!("PUT {path} failed"))?;

        if resp.status() == StatusCode::UNAUTHORIZED || resp.status() == StatusCode::FORBIDDEN {
            self.invalidate_token().await;
            let new_token = self.ensure_token().await?;
            let resp = self
                .apply_request_id(
                    self.http.put(&url).bearer_auth(&new_token).json(&body),
                    request_id,
                )
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

    /// DELETE a VTN endpoint with automatic 401-retry.
    pub async fn delete_json(&self, path: &str, request_id: Option<&str>) -> Result<()> {
        let token = self.ensure_token().await?;
        let url = format!("{}{}", self.base_url.trim_end_matches('/'), path);

        let resp = self
            .apply_request_id(self.http.delete(&url).bearer_auth(&token), request_id)
            .send()
            .await
            .context(format!("DELETE {path} failed"))?;

        if resp.status() == StatusCode::UNAUTHORIZED || resp.status() == StatusCode::FORBIDDEN {
            self.invalidate_token().await;
            let new_token = self.ensure_token().await?;
            let resp = self
                .apply_request_id(self.http.delete(&url).bearer_auth(&new_token), request_id)
                .send()
                .await
                .context(format!("DELETE {path} retry failed"))?;

            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                anyhow::bail!("{path} returned {status}: {body}");
            }
            return Ok(());
        }

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("{path} returned {status}: {body}");
        }

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
            self.ensure_token().await.is_ok()
        } else {
            false
        };

        (reachable, auth_ok)
    }
}
