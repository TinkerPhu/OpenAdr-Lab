use anyhow::{Context, Result};

#[derive(Clone, Debug)]
pub struct Config {
    pub listen_addr: String,
    pub vtn_base_url: String,
    pub client_id: String,
    pub client_secret: String,
    pub cache_ttl_programs: u64,
    pub cache_ttl_events: u64,
    pub cache_ttl_vens: u64,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let listen_addr =
            std::env::var("BFF_LISTEN_ADDR").unwrap_or_else(|_| "0.0.0.0:8090".into());
        let vtn_base_url = std::env::var("VTN_BASE_URL").context("VTN_BASE_URL missing")?;
        let client_id = std::env::var("VTN_CLIENT_ID").context("VTN_CLIENT_ID missing")?;
        let client_secret =
            std::env::var("VTN_CLIENT_SECRET").context("VTN_CLIENT_SECRET missing")?;

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

        Ok(Self {
            listen_addr,
            vtn_base_url,
            client_id,
            client_secret,
            cache_ttl_programs,
            cache_ttl_events,
            cache_ttl_vens,
        })
    }
}
