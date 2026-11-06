//! MQTT subscriber for the forecasts the lab-forecaster publishes (openspec `lab-forecaster`,
//! `docs/architecture/forecast_publication.md`). Infra-layer adapter, same shape as `weather.rs`: a background
//! task owns the subscription and writes what arrives into `AppState`, where the per-tick feed resolution reads
//! it. It decides nothing about a series: `lab_forecast::received_forecasts` validates and stores, the PV and
//! base-load assets interpret.
//!
//! Active only when `FORECAST_MQTT_HOST` is set; without it the VEN computes its own forecasts exactly as it did
//! before the forecaster existed.

use std::time::Duration;

use lab_forecast::forecast_payload::ForecastTopics;
use lab_forecast::received_forecasts::subscriptions;
use rumqttc::{AsyncClient, Event, MqttOptions, Packet, QoS};
use tracing::{info, warn};

use crate::state::AppState;

#[derive(Clone, Debug)]
pub struct ForecastFeedConfig {
    pub broker_host: String,
    pub broker_port: u16,
    /// `<root>` in `<root>/forecast/<site_id>/...`
    pub root: String,
    pub site_id: String,
}

impl ForecastFeedConfig {
    pub fn from_env() -> Option<Self> {
        let broker_host = std::env::var("FORECAST_MQTT_HOST").ok()?;
        Some(Self {
            broker_host,
            broker_port: std::env::var("FORECAST_MQTT_PORT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(1883),
            root: std::env::var("FORECAST_MQTT_ROOT").unwrap_or_else(|_| "openadr-lab".into()),
            site_id: std::env::var("FORECAST_MQTT_SITE_ID").unwrap_or_else(|_| "default".into()),
        })
    }
}

/// Spawn the subscription task: one topic per PV array plus the base load.
pub fn spawn(config: ForecastFeedConfig, pv_array_ids: Vec<String>, state: AppState) {
    let topics = match ForecastTopics::new(&config.root, &config.site_id) {
        Ok(t) => t,
        Err(e) => {
            warn!(error = %e, "forecast feed: root or site id unusable, not subscribing");
            return;
        }
    };
    let wanted = subscriptions(&topics, &pv_array_ids);
    // A client id must be unique per broker connection; several VENs share one site, so add the VEN name.
    let ven_name = std::env::var("VEN_NAME").unwrap_or_else(|_| "ven-1".into());
    let mut options = MqttOptions::new(
        format!("ven-forecast-{}-{}", ven_name, config.site_id),
        config.broker_host.clone(),
        config.broker_port,
    );
    options.set_keep_alive(Duration::from_secs(30));
    // 48 hourly values are a few KB; rumqttc's 10 KB default is too close.
    options.set_max_packet_size(256 * 1024, 10 * 1024);
    let (client, mut eventloop) = AsyncClient::new(options, 32);
    info!(
        broker = %config.broker_host,
        site_id = %config.site_id,
        topics = wanted.len(),
        "forecast feed: MQTT subscriber configured"
    );
    tokio::spawn(async move {
        loop {
            match eventloop.poll().await {
                // Clean session: the broker forgets the subscriptions on every disconnect, so subscribe on
                // every ConnAck (the retained messages then arrive at once).
                Ok(Event::Incoming(Packet::ConnAck(_))) => {
                    for (topic, _) in &wanted {
                        if let Err(e) = client.subscribe(topic, QoS::AtLeastOnce).await {
                            warn!(error = %e, topic = %topic, "forecast feed: subscribe failed");
                        }
                    }
                }
                Ok(Event::Incoming(Packet::Publish(p))) => {
                    if let Some((_, key)) = wanted.iter().find(|(topic, _)| *topic == p.topic) {
                        state
                            .apply_received_forecast(key, &p.payload, chrono::Utc::now())
                            .await;
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    warn!(error = %e, "forecast feed: mqtt connection error, retrying");
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }
    });
}
