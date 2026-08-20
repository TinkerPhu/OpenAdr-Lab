use anyhow::{Context, Result};

#[derive(Clone, Debug)]
pub struct Config {
    pub listen_addr: String,
    pub vtn_base_url: String,
    pub client_id: String,
    pub client_secret: String,
    pub ven_name: String,
    /// GB-09: poll cadence + startup jitter live in the profile
    /// (`Profile::polling`) since real VENs are deployed one profile per
    /// instance — these env vars are a test-only override on top of the
    /// profile value, `None` meaning "use the profile", not a deployment
    /// mechanism.
    pub poll_events_secs_override: Option<u64>,
    pub poll_programs_secs_override: Option<u64>,
    pub poll_reports_secs_override: Option<u64>,
    pub poll_startup_jitter_fixed_pct_override: Option<f64>,
    pub poll_startup_jitter_random_max_pct_override: Option<f64>,
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

        let poll_events_secs_override = std::env::var("POLL_EVENTS_SECS")
            .ok()
            .and_then(|v| v.parse().ok());
        let poll_programs_secs_override = std::env::var("POLL_PROGRAMS_SECS")
            .ok()
            .and_then(|v| v.parse().ok());
        let poll_reports_secs_override = std::env::var("POLL_REPORTS_SECS")
            .ok()
            .and_then(|v| v.parse().ok());
        let poll_startup_jitter_fixed_pct_override = std::env::var("POLL_STARTUP_JITTER_FIXED_PCT")
            .ok()
            .and_then(|v| v.parse().ok());
        let poll_startup_jitter_random_max_pct_override =
            std::env::var("POLL_STARTUP_JITTER_RANDOM_MAX_PCT")
                .ok()
                .and_then(|v| v.parse().ok());

        let persist_path = std::env::var("PERSIST_PATH").ok();
        let profile_path = std::env::var("PROFILE_PATH").ok();

        Ok(Self {
            listen_addr,
            vtn_base_url,
            client_id,
            client_secret,
            ven_name,
            poll_events_secs_override,
            poll_programs_secs_override,
            poll_reports_secs_override,
            poll_startup_jitter_fixed_pct_override,
            poll_startup_jitter_random_max_pct_override,
            persist_path,
            profile_path,
        })
    }
}
