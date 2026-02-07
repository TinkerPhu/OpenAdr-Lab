use anyhow::{Context, Result};

#[derive(Clone, Debug)]
pub struct Config {
    pub listen_addr: String,
    pub vtn_base_url: String,
    pub business_client_id: String,
    pub business_client_secret: String,
    pub ven_mgr_client_id: String,
    pub ven_mgr_client_secret: String,
    pub cache_ttl_programs: u64,
    pub cache_ttl_events: u64,
    pub cache_ttl_vens: u64,
    pub cache_ttl_reports: u64,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let listen_addr =
            std::env::var("BFF_LISTEN_ADDR").unwrap_or_else(|_| "0.0.0.0:8090".into());
        let vtn_base_url = std::env::var("VTN_BASE_URL").context("VTN_BASE_URL missing")?;

        let business_client_id =
            std::env::var("VTN_BUSINESS_CLIENT_ID").context("VTN_BUSINESS_CLIENT_ID missing")?;
        let business_client_secret = std::env::var("VTN_BUSINESS_CLIENT_SECRET")
            .context("VTN_BUSINESS_CLIENT_SECRET missing")?;

        let ven_mgr_client_id =
            std::env::var("VTN_VEN_MGR_CLIENT_ID").context("VTN_VEN_MGR_CLIENT_ID missing")?;
        let ven_mgr_client_secret = std::env::var("VTN_VEN_MGR_CLIENT_SECRET")
            .context("VTN_VEN_MGR_CLIENT_SECRET missing")?;

        let cache_ttl_programs = std::env::var("CACHE_TTL_PROGRAMS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(30);

        let cache_ttl_events = std::env::var("CACHE_TTL_EVENTS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(10);

        let cache_ttl_vens = std::env::var("CACHE_TTL_VENS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(10);

        let cache_ttl_reports = std::env::var("CACHE_TTL_REPORTS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(10);

        Ok(Self {
            listen_addr,
            vtn_base_url,
            business_client_id,
            business_client_secret,
            ven_mgr_client_id,
            ven_mgr_client_secret,
            cache_ttl_programs,
            cache_ttl_events,
            cache_ttl_vens,
            cache_ttl_reports,
        })
    }
}
