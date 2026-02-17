use anyhow::{Context, Result};

#[derive(Clone, Debug)]
pub struct Config {
    pub listen_addr: String,
    pub vtn_base_url: String,
    pub client_id: String,
    pub client_secret: String,
    pub ven_name: String,
    pub poll_events_secs: u64,
    pub poll_programs_secs: u64,
    pub poll_reports_secs: u64,
    pub persist_path: Option<String>,
    pub profile_path: Option<String>,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let listen_addr = std::env::var("LISTEN_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".into());
        let vtn_base_url = std::env::var("VTN_BASE_URL").context("VTN_BASE_URL missing")?;
        let client_id = std::env::var("CLIENT_ID").context("CLIENT_ID missing")?;
        let client_secret = std::env::var("CLIENT_SECRET").context("CLIENT_SECRET missing")?;
        let ven_name = std::env::var("VEN_NAME").unwrap_or_else(|_| "ven-1".into());

        let poll_events_secs = std::env::var("POLL_EVENTS_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(30);

        let poll_programs_secs = std::env::var("POLL_PROGRAMS_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(30);

        let poll_reports_secs = std::env::var("POLL_REPORTS_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(60);

        let persist_path = std::env::var("PERSIST_PATH").ok();
        let profile_path = std::env::var("PROFILE_PATH").ok();

        Ok(Self {
            listen_addr,
            vtn_base_url,
            client_id,
            client_secret,
            ven_name,
            poll_events_secs,
            poll_programs_secs,
            poll_reports_secs,
            persist_path,
            profile_path,
        })
    }
}
