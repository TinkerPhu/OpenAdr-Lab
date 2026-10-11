//! How a client of the VTN authenticates: one OAuth client-credentials token, cached until
//! shortly before it expires, and replaced once when the VTN refuses it.
//!
//! The VEN and the BFF both talk to the VTN this way, and each had its own copy of the token
//! fetch, the cache and the re-authenticate-and-retry block (R-135), so the rule lives here.
//! Owns the token and when it is fetched again. Reads nothing but the VTN's token endpoint.
//! Never interprets the response to an authorized request: status and body go back to the
//! caller, whose error vocabulary differs per service.

use std::fmt;
use std::sync::Arc;
use std::time::Instant;

use chrono::{DateTime, Utc};
use reqwest::{RequestBuilder, Response, StatusCode};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

/// A cached token is replaced this long before the VTN would stop accepting it, so a request
/// never leaves with a token that expires on the way.
pub const TOKEN_EXPIRY_MARGIN_S: u64 = 60;

/// What a token response that names no `expires_in` is taken to mean.
pub const DEFAULT_TOKEN_LIFETIME_S: u64 = 3600;

/// Why an authorized request did not produce a response.
#[derive(Debug)]
pub enum VtnAuthError {
    /// The token endpoint could not be reached.
    TokenRequest(reqwest::Error),
    /// The token endpoint answered, and refused.
    TokenRejected { status: StatusCode, body: String },
    /// The token endpoint answered 200 with a body that is not a token response.
    TokenParse(reqwest::Error),
    /// The authorized request itself could not be sent. `retried` is true when it was the
    /// second attempt, after the VTN refused the first token.
    Send {
        retried: bool,
        source: reqwest::Error,
    },
}

impl VtnAuthError {
    /// The transport failure behind this error, if it is one (so a caller can tell an
    /// unreachable VTN from a refusing one).
    pub fn transport(&self) -> Option<&reqwest::Error> {
        match self {
            Self::TokenRequest(e) | Self::Send { source: e, .. } => Some(e),
            Self::TokenRejected { .. } | Self::TokenParse(_) => None,
        }
    }

    /// True when the failure is in getting a token, not in the request it was for.
    pub fn is_token_failure(&self) -> bool {
        !matches!(self, Self::Send { .. })
    }
}

impl fmt::Display for VtnAuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TokenRequest(_) => write!(f, "token request failed"),
            Self::TokenRejected { status, body } => {
                write!(f, "token endpoint returned {status}: {body}")
            }
            Self::TokenParse(_) => write!(f, "parse token response"),
            Self::Send { retried: false, .. } => write!(f, "request failed"),
            Self::Send { retried: true, .. } => {
                write!(f, "request failed on the retry after re-authenticating")
            }
        }
    }
}

impl std::error::Error for VtnAuthError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::TokenRequest(e) | Self::TokenParse(e) | Self::Send { source: e, .. } => Some(e),
            Self::TokenRejected { .. } => None,
        }
    }
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: Option<u64>,
}

#[derive(Serialize)]
struct TokenForm<'a> {
    grant_type: &'a str,
    client_id: &'a str,
    client_secret: &'a str,
}

struct Token {
    access_token: String,
    acquired_at: Instant,
    expires_in_s: u64,
}

type OnNewToken = Arc<dyn Fn(&str) + Send + Sync>;

/// One client's credentials at one VTN, with the token they currently hold. Cloning shares the
/// token.
#[derive(Clone)]
pub struct ClientCredentials {
    http: reqwest::Client,
    token_url: String,
    client_id: String,
    client_secret: String,
    token: Arc<RwLock<Option<Token>>>,
    on_new_token: Option<OnNewToken>,
}

impl ClientCredentials {
    pub fn new(
        http: reqwest::Client,
        base_url: &str,
        client_id: String,
        client_secret: String,
    ) -> Self {
        Self {
            http,
            token_url: format!("{}/auth/token", base_url.trim_end_matches('/')),
            client_id,
            client_secret,
            token: Arc::new(RwLock::new(None)),
            on_new_token: None,
        }
    }

    /// Call `inspect` with every token the VTN issues, as it is issued (the VEN reads its own
    /// grant from it).
    pub fn on_new_token(mut self, inspect: impl Fn(&str) + Send + Sync + 'static) -> Self {
        self.on_new_token = Some(Arc::new(inspect));
        self
    }

    /// The token to send: the cached one while it is good for longer than
    /// `TOKEN_EXPIRY_MARGIN_S`, otherwise a new one.
    pub async fn token(&self) -> Result<String, VtnAuthError> {
        if let Some(t) = self.token.read().await.as_ref() {
            if t.acquired_at.elapsed().as_secs() + TOKEN_EXPIRY_MARGIN_S < t.expires_in_s {
                return Ok(t.access_token.clone());
            }
        }
        self.fetch().await
    }

    async fn fetch(&self) -> Result<String, VtnAuthError> {
        let resp = self
            .http
            .post(&self.token_url)
            .form(&TokenForm {
                grant_type: "client_credentials",
                client_id: &self.client_id,
                client_secret: &self.client_secret,
            })
            .send()
            .await
            .map_err(VtnAuthError::TokenRequest)?;
        let status = resp.status();
        if status != StatusCode::OK {
            let body = resp.text().await.unwrap_or_default();
            return Err(VtnAuthError::TokenRejected { status, body });
        }
        let issued: TokenResponse = resp.json().await.map_err(VtnAuthError::TokenParse)?;
        if let Some(inspect) = &self.on_new_token {
            inspect(&issued.access_token);
        }
        *self.token.write().await = Some(Token {
            access_token: issued.access_token.clone(),
            acquired_at: Instant::now(),
            expires_in_s: issued.expires_in.unwrap_or(DEFAULT_TOKEN_LIFETIME_S),
        });
        Ok(issued.access_token)
    }

    /// Forget the cached token, so the next request fetches a new one.
    pub async fn invalidate(&self) {
        *self.token.write().await = None;
    }

    /// Wall-clock expiry of the cached token, for a status display. The cache itself runs on the
    /// monotonic clock; this derives a `DateTime` from the time elapsed since the token was
    /// issued. Deliberately unclamped: an expired token answers a time in the past (how long
    /// ago), because clamping to `now` would make it look like one about to expire.
    pub async fn expires_at(&self, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
        let guard = self.token.read().await;
        let t = guard.as_ref()?;
        let elapsed = chrono::Duration::from_std(t.acquired_at.elapsed()).unwrap_or_default();
        Some(now - elapsed + chrono::Duration::seconds(t.expires_in_s as i64))
    }

    /// Send the request `build` makes, with the token as its bearer. If the VTN answers 401 or
    /// 403 the token is replaced and the request sent once more: the token may have been revoked,
    /// or the VTN restarted with a new signing key. Whatever comes back then is the answer,
    /// success or not.
    pub async fn send(&self, build: impl Fn() -> RequestBuilder) -> Result<Response, VtnAuthError> {
        let resp = self.send_once(&build, false).await?;
        if !matches!(
            resp.status(),
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
        ) {
            return Ok(resp);
        }
        self.invalidate().await;
        self.send_once(&build, true).await
    }

    async fn send_once(
        &self,
        build: &impl Fn() -> RequestBuilder,
        retried: bool,
    ) -> Result<Response, VtnAuthError> {
        let token = self.token().await?;
        build()
            .bearer_auth(token)
            .send()
            .await
            .map_err(|source| VtnAuthError::Send { retried, source })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::State;
    use axum::http::{HeaderMap, StatusCode as AxStatus};
    use axum::response::IntoResponse;
    use axum::routing::{get, post};
    use axum::{Form, Json, Router};
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    /// A VTN that issues `token-1`, `token-2`, ... and counts what it is asked.
    #[derive(Default)]
    struct StubVtn {
        tokens_issued: AtomicUsize,
        resource_calls: AtomicUsize,
        /// `expires_in` of every issued token; `None` leaves the field out.
        expires_in: Option<u64>,
        /// The resource accepts only this bearer; `None` accepts any.
        accepted_bearer: Option<&'static str>,
        /// What the resource answers to a bearer it does not accept.
        refusal: u16,
        token_forms: Mutex<Vec<HashMap<String, String>>>,
    }

    async fn issue_token(
        State(vtn): State<Arc<StubVtn>>,
        Form(form): Form<HashMap<String, String>>,
    ) -> Json<serde_json::Value> {
        vtn.token_forms.lock().unwrap().push(form);
        let n = vtn.tokens_issued.fetch_add(1, Ordering::SeqCst) + 1;
        let mut body = json!({"access_token": format!("token-{n}"), "token_type": "bearer"});
        if let Some(s) = vtn.expires_in {
            body["expires_in"] = json!(s);
        }
        Json(body)
    }

    async fn resource(State(vtn): State<Arc<StubVtn>>, headers: HeaderMap) -> impl IntoResponse {
        vtn.resource_calls.fetch_add(1, Ordering::SeqCst);
        let bearer = headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string();
        match vtn.accepted_bearer {
            Some(accepted) if bearer != format!("Bearer {accepted}") => {
                AxStatus::from_u16(vtn.refusal).unwrap().into_response()
            }
            _ => Json(json!({"bearer": bearer})).into_response(),
        }
    }

    /// Serve `vtn` on an ephemeral local port; returns its credentials client and resource URL.
    async fn spawn(vtn: Arc<StubVtn>) -> (ClientCredentials, String) {
        let app = Router::new()
            .route("/auth/token", post(issue_token))
            .route("/programs", get(resource))
            .with_state(vtn);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let credentials = ClientCredentials::new(
            reqwest::Client::new(),
            // A trailing slash on the configured base URL must not double up.
            &format!("{base_url}/"),
            "ven-1".into(),
            "s3cret".into(),
        );
        (credentials, format!("{base_url}/programs"))
    }

    fn stub(expires_in: Option<u64>) -> Arc<StubVtn> {
        Arc::new(StubVtn {
            expires_in,
            refusal: 401,
            ..Default::default()
        })
    }

    #[tokio::test]
    async fn token_is_fetched_once_and_then_served_from_the_cache() {
        let vtn = stub(Some(3600));
        let (credentials, _) = spawn(vtn.clone()).await;

        assert_eq!(credentials.token().await.unwrap(), "token-1");
        assert_eq!(credentials.token().await.unwrap(), "token-1");

        assert_eq!(vtn.tokens_issued.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn token_request_is_a_client_credentials_form() {
        let vtn = stub(Some(3600));
        let (credentials, _) = spawn(vtn.clone()).await;

        credentials.token().await.unwrap();

        let forms = vtn.token_forms.lock().unwrap();
        assert_eq!(forms[0]["grant_type"], "client_credentials");
        assert_eq!(forms[0]["client_id"], "ven-1");
        assert_eq!(forms[0]["client_secret"], "s3cret");
    }

    #[tokio::test]
    async fn token_inside_the_expiry_margin_is_fetched_again() {
        let vtn = stub(Some(TOKEN_EXPIRY_MARGIN_S - 1));
        let (credentials, _) = spawn(vtn.clone()).await;

        assert_eq!(credentials.token().await.unwrap(), "token-1");
        assert_eq!(credentials.token().await.unwrap(), "token-2");
    }

    #[tokio::test]
    async fn token_without_expires_in_is_cached_for_the_default_lifetime() {
        let vtn = stub(None);
        let (credentials, _) = spawn(vtn.clone()).await;
        let now = Utc::now();

        credentials.token().await.unwrap();
        credentials.token().await.unwrap();

        assert_eq!(vtn.tokens_issued.load(Ordering::SeqCst), 1);
        let expires_at = credentials.expires_at(now).await.unwrap();
        let lifetime_s = (expires_at - now).num_seconds();
        assert!(
            (DEFAULT_TOKEN_LIFETIME_S as i64 - lifetime_s).abs() <= 2,
            "{lifetime_s}"
        );
    }

    #[tokio::test]
    async fn invalidate_makes_the_next_token_a_new_one() {
        let vtn = stub(Some(3600));
        let (credentials, _) = spawn(vtn.clone()).await;

        credentials.token().await.unwrap();
        credentials.invalidate().await;

        assert_eq!(credentials.token().await.unwrap(), "token-2");
    }

    #[tokio::test]
    async fn token_refused_by_the_endpoint_carries_status_and_body() {
        let app = Router::new().route(
            "/auth/token",
            post(|| async { (AxStatus::UNAUTHORIZED, "bad client secret") }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let credentials =
            ClientCredentials::new(reqwest::Client::new(), &base_url, "id".into(), "x".into());

        let err = credentials.token().await.unwrap_err();

        assert!(
            matches!(&err, VtnAuthError::TokenRejected { status, body }
                if *status == StatusCode::UNAUTHORIZED && body == "bad client secret"),
            "{err:?}"
        );
        assert_eq!(
            err.to_string(),
            "token endpoint returned 401 Unauthorized: bad client secret"
        );
        assert!(err.is_token_failure());
        assert!(err.transport().is_none(), "the VTN answered");
    }

    #[tokio::test]
    async fn token_request_to_an_unreachable_vtn_is_a_transport_failure() {
        // Nothing listens on this port (bound, then dropped).
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        drop(listener);
        let credentials =
            ClientCredentials::new(reqwest::Client::new(), &base_url, "id".into(), "x".into());

        let err = credentials.token().await.unwrap_err();

        assert!(matches!(err, VtnAuthError::TokenRequest(_)), "{err:?}");
        assert!(err.transport().is_some_and(|e| e.is_connect()));
    }

    #[tokio::test]
    async fn on_new_token_sees_every_issued_token_and_no_cached_one() {
        let vtn = stub(Some(3600));
        let (credentials, _) = spawn(vtn.clone()).await;
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = seen.clone();
        let credentials =
            credentials.on_new_token(move |token| sink.lock().unwrap().push(token.to_string()));

        credentials.token().await.unwrap();
        credentials.token().await.unwrap();
        credentials.invalidate().await;
        credentials.token().await.unwrap();

        assert_eq!(*seen.lock().unwrap(), vec!["token-1", "token-2"]);
    }

    #[tokio::test]
    async fn expires_at_is_none_before_any_token() {
        let (credentials, _) = spawn(stub(Some(3600))).await;
        assert_eq!(credentials.expires_at(Utc::now()).await, None);
    }

    #[tokio::test]
    async fn expires_at_is_the_issue_time_plus_expires_in() {
        let (credentials, _) = spawn(stub(Some(1800))).await;
        let now = Utc::now();
        credentials.token().await.unwrap();

        let expires_at = credentials.expires_at(now).await.unwrap();

        let lifetime_s = (expires_at - now).num_seconds();
        assert!((1800 - lifetime_s).abs() <= 2, "{lifetime_s}");
    }

    #[tokio::test]
    async fn expires_at_of_an_expired_token_is_in_the_past_not_clamped_to_now() {
        let (credentials, _) = spawn(stub(Some(0))).await;
        credentials.token().await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        let now = Utc::now();

        let expires_at = credentials.expires_at(now).await.unwrap();

        assert!(expires_at < now, "{expires_at} vs {now}");
    }

    #[tokio::test]
    async fn send_carries_the_token_as_bearer() {
        let vtn = stub(Some(3600));
        let (credentials, url) = spawn(vtn.clone()).await;
        let http = reqwest::Client::new();

        let resp = credentials.send(|| http.get(&url)).await.unwrap();

        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body, json!({"bearer": "Bearer token-1"}));
    }

    async fn send_after_a_refusal(refusal: u16) {
        let vtn = Arc::new(StubVtn {
            expires_in: Some(3600),
            accepted_bearer: Some("token-2"),
            refusal,
            ..Default::default()
        });
        let (credentials, url) = spawn(vtn.clone()).await;
        let http = reqwest::Client::new();

        let resp = credentials.send(|| http.get(&url)).await.unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(vtn.tokens_issued.load(Ordering::SeqCst), 2);
        assert_eq!(vtn.resource_calls.load(Ordering::SeqCst), 2);
        assert_eq!(
            credentials.token().await.unwrap(),
            "token-2",
            "the new token is the cached one"
        );
    }

    #[tokio::test]
    async fn send_retries_once_with_a_new_token_after_401() {
        send_after_a_refusal(401).await;
    }

    #[tokio::test]
    async fn send_retries_once_with_a_new_token_after_403() {
        send_after_a_refusal(403).await;
    }

    #[tokio::test]
    async fn send_returns_a_second_refusal_without_a_third_attempt() {
        let vtn = Arc::new(StubVtn {
            expires_in: Some(3600),
            accepted_bearer: Some("never issued"),
            refusal: 403,
            ..Default::default()
        });
        let (credentials, url) = spawn(vtn.clone()).await;
        let http = reqwest::Client::new();

        let resp = credentials.send(|| http.get(&url)).await.unwrap();

        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        assert_eq!(vtn.resource_calls.load(Ordering::SeqCst), 2);
        assert_eq!(vtn.tokens_issued.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn send_returns_any_other_status_untouched_and_keeps_the_token() {
        let vtn = Arc::new(StubVtn {
            expires_in: Some(3600),
            accepted_bearer: Some("never issued"),
            refusal: 500,
            ..Default::default()
        });
        let (credentials, url) = spawn(vtn.clone()).await;
        let http = reqwest::Client::new();

        let resp = credentials.send(|| http.get(&url)).await.unwrap();

        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(vtn.resource_calls.load(Ordering::SeqCst), 1);
        assert_eq!(vtn.tokens_issued.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn send_to_an_unreachable_host_is_a_send_failure_not_a_token_failure() {
        let (credentials, _) = spawn(stub(Some(3600))).await;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let dead_url = format!("http://{}/programs", listener.local_addr().unwrap());
        drop(listener);
        let http = reqwest::Client::new();

        let err = credentials.send(|| http.get(&dead_url)).await.unwrap_err();

        assert!(
            matches!(err, VtnAuthError::Send { retried: false, .. }),
            "{err:?}"
        );
        assert!(!err.is_token_failure());
        assert!(err.transport().is_some_and(|e| e.is_connect()));
    }
}
